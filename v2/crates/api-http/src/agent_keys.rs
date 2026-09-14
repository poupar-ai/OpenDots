//! Identidade dos agentes — keypair Ed25519 + JWT por spawn.
//!
//! - **Geração**: na criação do agente. Pública vai pro `agents.json`
//!   (em hex), privada vai pro vault como secret `agent.<name>.privkey`.
//! - **Spawn**: servidor lê a privada, monta JWT `{agent, exp}` (10 min),
//!   passa pro processo do agente via env `POUPAR_AGENT_TOKEN`.
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

/// Versão do formato do claim. Permite evoluir o JWT sem invalidar tokens
/// antigos cegamente — futuro: verificar/migrar conforme `ver`.
pub const CLAIMS_VER: u32 = 1;

fn default_claims_ver() -> u32 {
    1
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Nome do agente — ID autoritativo do `from` num handoff.
    pub agent: String,
    /// Unix epoch seconds — expiração. Validade típica: 10 min.
    pub exp: u64,
    /// Versão do formato do claim (default 1 p/ tokens pré-versionamento).
    #[serde(default = "default_claims_ver")]
    pub ver: u32,
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
        ver: CLAIMS_VER,
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

/// Nome canônico do secret da privkey do agente no vault. Uppercase +
/// underscore pra bater a regex de validação `[A-Z][A-Z0-9_]{0,63}` —
/// vide vault_routes::secret_put. Caracteres não-alfanuméricos no nome
/// do agente viram `_`. Single source of truth pra evitar drift entre
/// `create_handler` (escreve) e `mint_token_for` (lê).
pub fn privkey_secret_name(agent: &str) -> String {
    let normalized: String = agent
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("AGENT_{normalized}_PRIVKEY")
}

/// Path do arquivo da privada de um agente no filesystem (fora do vault).
/// Armazenado raw (32 bytes), perms 0600. Identidade do agente é
/// machine-local — não precisa de master password pra ser usada.
pub fn agent_priv_path(mem_dir: &std::path::Path, agent: &str) -> std::path::PathBuf {
    let normalized = privkey_secret_name(agent); // já normaliza pra A-Z0-9_
    mem_dir.join("agent_keys").join(format!("{normalized}.priv"))
}

/// Garante que o agente tem keypair acessível e retorna a privada (32 bytes).
///
/// Auto-provisioning: se a chave ainda não existe (ex.: agente foi
/// instalado por seed pack sem keypair), gera Ed25519 fresco, salva a
/// privada em `<mem_dir>/agent_keys/<NAME>.priv` (chmod 0600 em Unix) e
/// escreve a pública no `agents.json` no campo `pubkey` (hex). Isso
/// elimina a dependência de vault unlocked pra handoff entre agentes —
/// o vault continua existindo pra **secrets do usuário**, mas
/// identidade de agente é tratada como recurso da máquina, não segredo.
fn ensure_agent_keypair(mem_dir: &std::path::Path, agent: &str) -> Option<[u8; 32]> {
    let priv_path = agent_priv_path(mem_dir, agent);
    if let Ok(bytes) = std::fs::read(&priv_path) {
        if bytes.len() == 32 {
            let mut out = [0u8; 32];
            out.copy_from_slice(&bytes);
            return Some(out);
        }
        // Arquivo corrompido — gera de novo abaixo
    }
    // Gera novo par
    let (priv_b, pub_b) = generate_keypair();

    // Persiste priv no fs (mkdir + write + chmod). Falhas aqui significam
    // que mint não vai funcionar — propaga None pro caller fallback.
    if let Some(parent) = priv_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::write(&priv_path, priv_b).is_err() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&priv_path, std::fs::Permissions::from_mode(0o600));
    }

    // Escreve pubkey no agents.json. Idempotente — se já tinha algo,
    // sobrescreve. Importante pro handoff middleware verificar JWT.
    if let Some(mut manifest) = domain_agents::load_manifest(mem_dir) {
        if let Some(agents) = manifest
            .get_mut("agents")
            .and_then(serde_json::Value::as_object_mut)
        {
            if let Some(entry) = agents.get_mut(agent).and_then(serde_json::Value::as_object_mut) {
                entry.insert(
                    "pubkey".to_string(),
                    serde_json::Value::String(bytes_to_hex(&pub_b)),
                );
                let _ = domain_agents::save_manifest(mem_dir, &manifest);
            }
        }
    }
    Some(priv_b)
}

/// Carrega (ou gera) a privada do agente e monta um JWT de 10min pronto
/// pra ser injetado em `POUPAR_AGENT_TOKEN` do spawn.
///
/// Vault continua aceitando privada legada (compat) — agentes criados
/// antes desta mudança têm a chave lá. Ordem de lookup:
///   1. Filesystem (`agent_priv_path`) — caminho novo, sem unlock
///   2. Vault (legacy) — se filesystem vazio e vault unlocked
///   3. Auto-gera no filesystem — quando nada existe
pub fn mint_token_for(
    agent: &str,
    vault: &std::sync::Arc<dyn core_ports::VaultStore>,
    mem_dir: &std::path::Path,
) -> Option<String> {
    // 1) Filesystem — caminho novo, sem dependência de vault unlocked
    let priv_path = agent_priv_path(mem_dir, agent);
    if let Ok(bytes) = std::fs::read(&priv_path) {
        if bytes.len() == 32 {
            let mut priv_bytes = [0u8; 32];
            priv_bytes.copy_from_slice(&bytes);
            return Some(sign_jwt(&priv_bytes, agent, 600));
        }
    }

    // 2) Vault — compat com agentes criados antes do fs-store. Tenta só
    //    se o vault está unlocked (silently falha senão).
    let secret_name = privkey_secret_name(agent);
    if let Ok(Some(hex)) = vault.get_secret(mem_dir, &secret_name) {
        if let Ok(priv_bytes) = hex_to_bytes(&hex) {
            return Some(sign_jwt(&priv_bytes, agent, 600));
        }
    }

    // 3) Nada → auto-provisiona no filesystem
    let priv_bytes = ensure_agent_keypair(mem_dir, agent)?;
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
