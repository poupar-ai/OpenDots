//! Identidade dos agentes — keypair Ed25519 + JWT por spawn.
//!
//! - **Geração**: na criação do agente. Pública vai pro `agents.json`
//!   (em hex), privada vai pro vault como secret `agent.<name>.privkey`.
//! - **Spawn**: servidor lê a privada, monta JWT `{agent, exp}` (10 min),
//!   passa pro processo do agente via env `PROJECT_MEM_AGENT_TOKEN`.
//! - **Handoff**: middleware lê Bearer, valida assinatura com a pública
//!   do agente declarado no claim, usa `agent` do token como `from`
//!   autoritativo (ignora body).
//!
//! JWT é DIY (header.payload.signature em base64url) pra não trazer dep
//! pesada do `jsonwebtoken`. Algoritmo único: EdDSA. Header fixo.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum KeyError {
    #[error("hex inválido: {0}")]
    BadHex(String),
    #[error("comprimento de chave inválido (esperava 32 bytes)")]
    BadLength,
    #[error("formato JWT inválido")]
    BadJwt,
    #[error("base64 inválido: {0}")]
    BadBase64(String),
    #[error("claims JSON inválido: {0}")]
    BadClaims(String),
    #[error("assinatura inválida")]
    BadSignature,
    #[error("token expirado")]
    Expired,
    #[error("pubkey não encontrada pro agente '{0}'")]
    PubkeyMissing(String),
}

/// Gera um par Ed25519 novo. Retorna (priv_bytes, pub_bytes), 32 bytes
/// cada. Privada vai pro vault, pública pro `agents.json`.
pub fn generate_keypair() -> ([u8; 32], [u8; 32]) {
    let mut csprng = OsRng;
    let signing = SigningKey::generate(&mut csprng);
    let verifying = signing.verifying_key();
    (signing.to_bytes(), verifying.to_bytes())
}

/// Converte 32 bytes → string hex (64 chars). Usado pra serializar
/// chaves em JSON (vault, agents.json).
pub fn bytes_to_hex(b: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for byte in b {
        s.push_str(&format!("{:02x}", byte));
    }
    s
}

/// Inverso de `bytes_to_hex`. Aceita exatamente 64 chars hex.
pub fn hex_to_bytes(s: &str) -> Result<[u8; 32], KeyError> {
    if s.len() != 64 {
        return Err(KeyError::BadLength);
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let hex_str = std::str::from_utf8(chunk).map_err(|_| KeyError::BadHex(s.into()))?;
        out[i] = u8::from_str_radix(hex_str, 16).map_err(|_| KeyError::BadHex(s.into()))?;
    }
    Ok(out)
}

// ── JWT ────────────────────────────────────────────────────────────────────
//
// Header fixo: `{"alg":"EdDSA","typ":"JWT"}`. Pre-encoded pra evitar
// alocação por chamada.

const HEADER_B64: &str = "eyJhbGciOiJFZERTQSIsInR5cCI6IkpXVCJ9"; // {"alg":"EdDSA","typ":"JWT"}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Nome do agente — ID autoritativo do `from` num handoff.
    pub agent: String,
    /// Unix epoch seconds — expiração. Validade típica: 10 min.
    pub exp: u64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Cria JWT `EdDSA` assinado com a privada do agente. Default TTL: 600s.
pub fn sign_jwt(priv_bytes: &[u8; 32], agent: &str, ttl_seconds: u64) -> String {
    let signing = SigningKey::from_bytes(priv_bytes);
    let claims = Claims {
        agent: agent.to_string(),
        exp: now_secs() + ttl_seconds,
    };
    let payload_json = serde_json::to_string(&claims).unwrap_or_else(|_| "{}".into());
    let payload_b64 = B64.encode(payload_json.as_bytes());
    let signing_input = format!("{HEADER_B64}.{payload_b64}");
    let sig: Signature = signing.sign(signing_input.as_bytes());
    let sig_b64 = B64.encode(sig.to_bytes());
    format!("{signing_input}.{sig_b64}")
}

/// Valida JWT e retorna `Claims`. Caller passa um lookup que resolve
/// `agent_name → pubkey bytes` (lido de `agents.json`). Assinatura E
/// expiração checados.
pub fn verify_jwt<F>(token: &str, lookup_pubkey: F) -> Result<Claims, KeyError>
where
    F: Fn(&str) -> Option<[u8; 32]>,
{
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(KeyError::BadJwt);
    }
    let (header_b64, payload_b64, sig_b64) = (parts[0], parts[1], parts[2]);
    if header_b64 != HEADER_B64 {
        return Err(KeyError::BadJwt);
    }
    let payload_bytes = B64
        .decode(payload_b64)
        .map_err(|e| KeyError::BadBase64(e.to_string()))?;
    let claims: Claims = serde_json::from_slice(&payload_bytes)
        .map_err(|e| KeyError::BadClaims(e.to_string()))?;
    let sig_bytes_vec = B64
        .decode(sig_b64)
        .map_err(|e| KeyError::BadBase64(e.to_string()))?;
    if sig_bytes_vec.len() != 64 {
        return Err(KeyError::BadSignature);
    }
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes_vec);
    let signature = Signature::from_bytes(&sig_arr);
    let pubkey_bytes =
        lookup_pubkey(&claims.agent).ok_or_else(|| KeyError::PubkeyMissing(claims.agent.clone()))?;
    let verifying =
        VerifyingKey::from_bytes(&pubkey_bytes).map_err(|_| KeyError::BadSignature)?;
    let signing_input = format!("{header_b64}.{payload_b64}");
    verifying
        .verify(signing_input.as_bytes(), &signature)
        .map_err(|_| KeyError::BadSignature)?;
    if claims.exp < now_secs() {
        return Err(KeyError::Expired);
    }
    Ok(claims)
}

// ── Helpers de integração com vault + agents.json ──────────────────────────

/// Lê a privada do agente do vault, monta JWT pronto pra spawn. None
/// se vault locked, secret ausente, ou hex inválido — caller decide se
/// continua sem token (modo untrusted) ou aborta. TTL: 10 minutos.
pub fn mint_token_for(
    agent: &str,
    vault: &std::sync::Arc<dyn core_ports::VaultStore>,
    mem_dir: &std::path::Path,
) -> Option<String> {
    let secret_name = format!("agent.{agent}.privkey");
    let hex = vault.get_secret(mem_dir, &secret_name).ok()??;
    let priv_bytes = hex_to_bytes(&hex).ok()?;
    Some(sign_jwt(&priv_bytes, agent, 600))
}

/// Lookup de pubkey de um agente — lê `agents.json` e procura
/// `agents.<name>.pubkey` (hex). Usado pelo handoff middleware pra
/// verificar JWT. None se manifest ausente, agente desconhecido, ou
/// pubkey faltando/inválida.
pub fn lookup_pubkey(mem_dir: &std::path::Path, agent: &str) -> Option<[u8; 32]> {
    let manifest = domain_agents::load_manifest(mem_dir)?;
    let hex = manifest
        .get("agents")?
        .get(agent)?
        .get("pubkey")?
        .as_str()?;
    hex_to_bytes(hex).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_sign_verify() {
        let (priv_bytes, pub_bytes) = generate_keypair();
        let token = sign_jwt(&priv_bytes, "manager", 60);
        let claims = verify_jwt(&token, |name| {
            if name == "manager" { Some(pub_bytes) } else { None }
        })
        .expect("verify should pass");
        assert_eq!(claims.agent, "manager");
    }

    #[test]
    fn tampered_payload_rejected() {
        let (priv_bytes, pub_bytes) = generate_keypair();
        let token = sign_jwt(&priv_bytes, "oracle", 60);
        // Swap "oracle" → "manager" no payload (mantém base64 válido,
        // mas vai falhar a verificação porque assinatura não bate).
        let parts: Vec<&str> = token.split('.').collect();
        let mut tampered_payload = B64.decode(parts[1]).unwrap();
        let s = String::from_utf8(tampered_payload.clone()).unwrap();
        let bad = s.replace("oracle", "managr"); // mesmo tamanho
        tampered_payload.clear();
        tampered_payload.extend_from_slice(bad.as_bytes());
        let bad_payload_b64 = B64.encode(&tampered_payload);
        let bad_token = format!("{}.{}.{}", parts[0], bad_payload_b64, parts[2]);
        let res = verify_jwt(&bad_token, |_| Some(pub_bytes));
        assert!(matches!(res, Err(KeyError::BadSignature)));
    }

    #[test]
    fn expired_rejected() {
        let (priv_bytes, pub_bytes) = generate_keypair();
        // ttl 0 → exp == agora → expired
        let token = sign_jwt(&priv_bytes, "manager", 0);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let res = verify_jwt(&token, |_| Some(pub_bytes));
        assert!(matches!(res, Err(KeyError::Expired)));
    }

    #[test]
    fn hex_roundtrip() {
        let (priv_bytes, _) = generate_keypair();
        let s = bytes_to_hex(&priv_bytes);
        let back = hex_to_bytes(&s).unwrap();
        assert_eq!(priv_bytes, back);
    }
}
