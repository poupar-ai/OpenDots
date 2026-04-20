//! Vault access — porta de `agents.cjs:781-813`.
//!
//! - [`SECRET_NAME_RE`] — regex do nome de secret (UPPER_SNAKE, até 64 chars).
//! - [`is_valid_secret_name`] — match helper.
//! - [`sanitize_secret_names`] — sanitiza array de nomes.
//! - [`update_vault_access`] — aplica ao agente no manifest.
//!
//! A lista de secrets que o agente recebe como env vars no spawn — é o
//! ACL. Se a lista sanitizada ficar vazia, o campo `secrets` é REMOVIDO
//! (não gravado como `[]`).

use crate::{load_manifest, manifest_path, save_manifest};
use serde_json::{json, Value};
use std::io;
use std::path::Path;
use thiserror::Error;

/// Forma canônica do regex usado pra validar nomes de secret.
/// Textual só — Rust não tem regex na stdlib; veja [`is_valid_secret_name`]
/// pra checagem efetiva.
///
/// Replica `SECRET_NAME_RE = /^[A-Z][A-Z0-9_]{0,63}$/` em
/// `agents.cjs:784`.
pub const SECRET_NAME_RE: &str = r"^[A-Z][A-Z0-9_]{0,63}$";

/// `true` se `name` bate o padrão `^[A-Z][A-Z0-9_]{0,63}$`.
///
/// - Primeiro caractere: letra ASCII maiúscula.
/// - Caracteres 2-64: letra ASCII maiúscula, dígito ou `_`.
/// - Tamanho total entre 1 e 64 caracteres (inclusive).
pub fn is_valid_secret_name(name: &str) -> bool {
    let len = name.len();
    if !(1..=64).contains(&len) {
        return false;
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_uppercase() {
        return false;
    }
    chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Sanitiza lista de nomes de secret. Porta de `sanitizeSecretNames`
/// em `agents.cjs:786`.
///
/// - Input não-array → `[]`.
/// - Filtra strings, trima, exige match do regex, dedupe preservando
///   ordem de primeira aparição.
/// - **Para na 50ª entrada** (antes de ordenar).
/// - Ordena o resultado alfabeticamente (`.sort()` do JS = lexicográfico
///   por UTF-16 / byte em ASCII).
pub fn sanitize_secret_names(input: &Value) -> Vec<String> {
    let arr = match input.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };

    let mut out: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in arr {
        let s = match item.as_str() {
            Some(s) => s.trim(),
            None => continue,
        };
        if s.is_empty() || !is_valid_secret_name(s) || seen.contains(s) {
            continue;
        }
        seen.insert(s.to_string());
        out.push(s.to_string());
        if out.len() >= 50 {
            break;
        }
    }
    out.sort();
    out
}

/// Erro de [`update_vault_access`].
#[derive(Debug, Error)]
pub enum UpdateVaultAccessError {
    #[error(r#"agente "{name}" não existe em {path}"#)]
    AgentMissing { name: String, path: String },
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

/// Atualiza os secrets de um agente. Porta de `updateVaultAccess`
/// em `agents.cjs:803`.
///
/// - Se lista sanitizada é vazia → remove o campo `secrets`.
/// - Senão → grava o array sanitizado (deduplicado, ordenado).
pub fn update_vault_access(
    mem_dir: &Path,
    name: &str,
    secrets: &Value,
) -> Result<Value, UpdateVaultAccessError> {
    let mut cfg =
        load_manifest(mem_dir).ok_or_else(|| UpdateVaultAccessError::AgentMissing {
            name: name.to_string(),
            path: manifest_path(mem_dir).display().to_string(),
        })?;

    let agent = cfg
        .get_mut("agents")
        .and_then(Value::as_object_mut)
        .and_then(|m| m.get_mut(name))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| UpdateVaultAccessError::AgentMissing {
            name: name.to_string(),
            path: manifest_path(mem_dir).display().to_string(),
        })?;

    let clean = sanitize_secret_names(secrets);
    if clean.is_empty() {
        agent.remove("secrets");
    } else {
        agent.insert(
            "secrets".to_string(),
            Value::Array(clean.into_iter().map(Value::String).collect()),
        );
    }

    let snapshot = Value::Object(agent.clone());
    save_manifest(mem_dir, &cfg)?;
    Ok(snapshot)
}

#[allow(dead_code)]
fn _touch(_: &Path) -> Value {
    json!(null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── is_valid_secret_name ────────────────────────────────────────────

    #[test]
    fn valid_secret_names() {
        assert!(is_valid_secret_name("API_KEY"));
        assert!(is_valid_secret_name("X"));
        assert!(is_valid_secret_name("DD_SITE"));
        assert!(is_valid_secret_name("FOO_123_BAR"));
        assert!(is_valid_secret_name("A".repeat(64).as_str()));
    }

    #[test]
    fn invalid_secret_names() {
        assert!(!is_valid_secret_name(""));
        assert!(!is_valid_secret_name("lowercase"));
        assert!(!is_valid_secret_name("_LEADING")); // first must be letter
        assert!(!is_valid_secret_name("1_LEAD")); // digit first not allowed
        assert!(!is_valid_secret_name("HAS-DASH"));
        assert!(!is_valid_secret_name("HAS SPACE"));
        assert!(!is_valid_secret_name("A".repeat(65).as_str()));
        assert!(!is_valid_secret_name("api_key")); // lowercase
    }

    // ── sanitize_secret_names ───────────────────────────────────────────

    #[test]
    fn sanitize_non_array_empty() {
        assert!(sanitize_secret_names(&json!("x")).is_empty());
        assert!(sanitize_secret_names(&json!(null)).is_empty());
        assert!(sanitize_secret_names(&json!(42)).is_empty());
        assert!(sanitize_secret_names(&json!({})).is_empty());
    }

    #[test]
    fn sanitize_empty_array() {
        assert!(sanitize_secret_names(&json!([])).is_empty());
    }

    #[test]
    fn sanitize_filters_and_trims() {
        let r = sanitize_secret_names(&json!([
            "  API_KEY  ",
            "dd_site",
            "DD_SITE",
            "",
            42,
            null,
            "HAS-DASH",
            "DB_URL"
        ]));
        assert_eq!(r, vec!["API_KEY", "DB_URL", "DD_SITE"]);
    }

    #[test]
    fn sanitize_dedupes() {
        let r = sanitize_secret_names(&json!(["DB_URL", "DB_URL", "API_KEY", "DB_URL"]));
        assert_eq!(r, vec!["API_KEY", "DB_URL"]);
    }

    #[test]
    fn sanitize_sorts_output() {
        let r = sanitize_secret_names(&json!(["Z", "A", "M"]));
        assert_eq!(r, vec!["A", "M", "Z"]);
    }

    #[test]
    fn sanitize_caps_at_50_before_sort() {
        // 60 itens já UPPERCASE; só os 50 primeiros entram, depois sorted.
        let many: Vec<String> = (0..60).map(|i| format!("K{i:02}")).collect();
        let r = sanitize_secret_names(&json!(many));
        assert_eq!(r.len(), 50);
        // sorted
        let mut sorted = r.clone();
        sorted.sort();
        assert_eq!(r, sorted);
    }

    // ── update_vault_access ─────────────────────────────────────────────

    fn setup(dir: &Path, name: &str) {
        save_manifest(
            dir,
            &json!({
                "version": 1,
                "agents": {name: {"type": "integrator"}}
            }),
        )
        .unwrap();
    }

    #[test]
    fn update_vault_sets_secrets() {
        let tmp = TempDir::new().unwrap();
        setup(tmp.path(), "m");
        let r =
            update_vault_access(tmp.path(), "m", &json!(["API_KEY", "DB_URL"])).unwrap();
        assert_eq!(r["secrets"], json!(["API_KEY", "DB_URL"]));
    }

    #[test]
    fn update_vault_empty_removes_field() {
        let tmp = TempDir::new().unwrap();
        save_manifest(
            tmp.path(),
            &json!({
                "version": 1,
                "agents": {"m": {"type":"integrator", "secrets": ["X"]}}
            }),
        )
        .unwrap();
        let r = update_vault_access(tmp.path(), "m", &json!([])).unwrap();
        assert!(r.get("secrets").is_none());
    }

    #[test]
    fn update_vault_all_invalid_removes_field() {
        let tmp = TempDir::new().unwrap();
        save_manifest(
            tmp.path(),
            &json!({
                "version": 1,
                "agents": {"m": {"type":"integrator", "secrets": ["X"]}}
            }),
        )
        .unwrap();
        let r = update_vault_access(tmp.path(), "m", &json!(["lowercase", "1BAD", ""])).unwrap();
        assert!(r.get("secrets").is_none());
    }

    #[test]
    fn update_vault_agent_missing() {
        let tmp = TempDir::new().unwrap();
        save_manifest(tmp.path(), &json!({"version":1,"agents":{}})).unwrap();
        let err =
            update_vault_access(tmp.path(), "m", &json!(["API_KEY"])).unwrap_err();
        assert!(matches!(err, UpdateVaultAccessError::AgentMissing { .. }));
    }

    #[test]
    fn update_vault_manifest_missing() {
        let tmp = TempDir::new().unwrap();
        let err =
            update_vault_access(tmp.path(), "m", &json!(["API_KEY"])).unwrap_err();
        assert!(matches!(err, UpdateVaultAccessError::AgentMissing { .. }));
    }
}
