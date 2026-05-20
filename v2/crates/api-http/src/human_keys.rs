//! Identidade e autenticação de humanos — paridade com `agent_keys`.
//!
//! - **Senha** (gate de auth, trocável por Firebase no futuro): hash
//!   PBKDF2-HMAC-SHA256 guardado no cofre como secret `HUMAN_<ID>_PWHASH`.
//! - **Keypair Ed25519** (identidade do handoff, igual agente): privada no
//!   filesystem (`<mem_dir>/human_keys/<id>.priv`, 0600), pública em hex no
//!   `humans.json`. Login bem-sucedido emite JWT assinado com essa privada.
//! - **Handoff**: middleware valida o Bearer com a pública do humano do
//!   claim e usa `sub` do token como `from` autoritativo (anti-impersonação).
//!
//! A cripto (Ed25519/JWT) é reaproveitada de `agent_keys` — sem duplicar.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use pbkdf2::pbkdf2_hmac;
use rand_core::{OsRng, RngCore};
use sha2::Sha256;

use crate::agent_keys::{bytes_to_hex, generate_keypair, hex_to_bytes, sign_jwt};

const PBKDF2_ROUNDS: u32 = 100_000;
const SALT_LEN: usize = 16;
const HASH_LEN: usize = 32;
const TOKEN_TTL_SECS: u64 = 3600; // 1h — sessão de UI

// ── Senha (PBKDF2) ───────────────────────────────────────────────────────────

/// Hash PBKDF2-HMAC-SHA256 com salt aleatório. Formato:
/// `pbkdf2-sha256$<rounds>$<salt_b64>$<hash_b64>`.
pub fn hash_password(password: &str) -> String {
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let mut out = [0u8; HASH_LEN];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, PBKDF2_ROUNDS, &mut out);
    format!(
        "pbkdf2-sha256${}${}${}",
        PBKDF2_ROUNDS,
        B64.encode(salt),
        B64.encode(out)
    )
}

/// Compara `password` com um hash gerado por `hash_password`. Comparação
/// em tempo constante. False em qualquer formato inválido.
pub fn verify_password(password: &str, stored: &str) -> bool {
    let parts: Vec<&str> = stored.split('$').collect();
    if parts.len() != 4 || parts[0] != "pbkdf2-sha256" {
        return false;
    }
    let Ok(rounds) = parts[1].parse::<u32>() else {
        return false;
    };
    let (Ok(salt), Ok(expected)) = (B64.decode(parts[2]), B64.decode(parts[3])) else {
        return false;
    };
    let mut out = vec![0u8; expected.len()];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, rounds, &mut out);
    constant_time_eq(&out, &expected)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Nome do secret da senha no cofre. Normaliza pra bater a regex do vault
/// (`[A-Z][A-Z0-9_]{0,63}`).
fn pwhash_secret_name(id: &str) -> String {
    let normalized: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
        .collect();
    format!("HUMAN_{normalized}_PWHASH")
}

/// Guarda o hash da senha no cofre. Requer cofre destrancado.
pub fn set_human_password(
    vault: &Arc<dyn core_ports::VaultStore>,
    mem_dir: &Path,
    id: &str,
    password: &str,
) -> Result<(), core_ports::VaultError> {
    vault.set_secret(mem_dir, &pwhash_secret_name(id), &hash_password(password))
}

/// True se existe hash de senha pro humano (cofre precisa estar destrancado
/// pra ler). Erros/lock → false.
pub fn has_human_password(
    vault: &Arc<dyn core_ports::VaultStore>,
    mem_dir: &Path,
    id: &str,
) -> bool {
    matches!(vault.get_secret(mem_dir, &pwhash_secret_name(id)), Ok(Some(_)))
}

/// Valida a senha contra o hash do cofre.
pub fn verify_human_password(
    vault: &Arc<dyn core_ports::VaultStore>,
    mem_dir: &Path,
    id: &str,
    password: &str,
) -> bool {
    match vault.get_secret(mem_dir, &pwhash_secret_name(id)) {
        Ok(Some(stored)) => verify_password(password, &stored),
        _ => false,
    }
}

// ── Keypair Ed25519 + JWT (identidade do handoff) ──────────────────────────

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

pub fn human_priv_path(mem_dir: &Path, id: &str) -> PathBuf {
    mem_dir.join("human_keys").join(format!("{}.priv", sanitize_id(id)))
}

/// Garante o keypair do humano: gera se não existir, grava privada (0600) e
/// escreve a pública (hex) no `humans.json`. Retorna a privada (32 bytes).
fn ensure_human_keypair(mem_dir: &Path, id: &str) -> Option<[u8; 32]> {
    let priv_path = human_priv_path(mem_dir, id);
    if let Ok(bytes) = std::fs::read(&priv_path) {
        if bytes.len() == 32 {
            let mut out = [0u8; 32];
            out.copy_from_slice(&bytes);
            return Some(out);
        }
    }
    let (priv_b, pub_b) = generate_keypair();
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
    // pubkey hex → humans.json
    let mut data = domain_humans::load_humans(mem_dir);
    if let Some(h) = data
        .get_mut("humans")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|m| m.get_mut(id))
        .and_then(serde_json::Value::as_object_mut)
    {
        h.insert("pubkey".into(), serde_json::Value::String(bytes_to_hex(&pub_b)));
        let _ = domain_humans::save_humans(mem_dir, &data);
    }
    Some(priv_b)
}

/// Lê a pública do humano do `humans.json` (hex → bytes). Usada na verificação
/// do JWT no handoff.
pub fn lookup_human_pubkey(mem_dir: &Path, id: &str) -> Option<[u8; 32]> {
    let data = domain_humans::load_humans(mem_dir);
    let hex = data.get("humans")?.get(id)?.get("pubkey")?.as_str()?;
    hex_to_bytes(hex).ok()
}

/// Emite um JWT (1h) assinado com a privada do humano, auto-provisionando o
/// keypair se necessário. Chamado após login/criação bem-sucedidos.
pub fn mint_token_for_human(mem_dir: &Path, id: &str) -> Option<String> {
    let priv_bytes = ensure_human_keypair(mem_dir, id)?;
    Some(sign_jwt(&priv_bytes, id, TOKEN_TTL_SECS))
}
