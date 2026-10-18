//! Modelo (LLM) — porta de `agents.cjs:656-728`.
//!
//! Expõe:
//! - [`normalize_model`] — valida/trima input, retorna `None` se "limpar".
//! - [`CLI_MODEL_MAP`] — aliases versionados → IDs completos.
//! - [`resolve_cli_model`] — converte modelo armazenado no que o
//!   `claude --model` aceita.
//! - [`update_model`] / [`propagate_model_to_seed`] — mutam manifest/seed.

use crate::{load_manifest, manifest_path, save_manifest};
use serde_json::{json, Value};
use std::fs;
use std::io;
use std::path::Path;
use thiserror::Error;

/// Modelo de todo agente novo (registro, defaults e seeds) quando nada
/// for informado.
pub const DEFAULT_MODEL: &str = "opus-5.5";

/// Aliases versionados → IDs completos pro `claude --model`. Porta de
/// `agents.cjs:672-684`. Ordem preservada pro caso de alguma serialização
/// iterar; em dicionário Rust usaríamos `HashMap`, mas aqui uso array
/// de pares pra manter determinismo e ordem byte-paridade.
pub const CLI_MODEL_MAP: &[(&str, &str)] = &[
    ("opus-5.5", "claude-opus-5-5"),
    ("sonnet-5.5", "claude-sonnet-5-5"),
    ("haiku-5.5", "claude-haiku-5-5"),
    ("opus-4.7", "claude-opus-4-7"),
    ("opus-4.6", "claude-opus-4-6"),
    ("opus-4.5", "claude-opus-4-5"),
    ("opus-4.1", "claude-opus-4-1"),
    ("opus-4", "claude-opus-4-0"),
    ("sonnet-4.6", "claude-sonnet-4-6"),
    ("sonnet-4.5", "claude-sonnet-4-5"),
    ("sonnet-4", "claude-sonnet-4-0"),
    ("sonnet-3.7", "claude-3-7-sonnet-latest"),
    ("haiku-4.5", "claude-haiku-4-5-20251001"),
    ("haiku-3.5", "claude-3-5-haiku-latest"),
];

/// Erros de [`normalize_model`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum NormalizeModelError {
    #[error(r#"model precisa ser string (ex: "opus", "opus-4.7", "claude-sonnet-4-6") ou null"#)]
    NotString,
    #[error("model muito longo (máx 80 chars)")]
    TooLong,
}

/// Normaliza input de modelo. Porta de `normalizeModel` em
/// `agents.cjs:658`.
///
/// - `null` ou string vazia → `None` (limpa o campo).
/// - Não-string → erro [`NotString`](NormalizeModelError::NotString).
/// - String com > 80 UTF-16 code units após trim → erro [`TooLong`].
/// - Senão → `Some(trimmed.to_string())`.
pub fn normalize_model(input: &Value) -> Result<Option<String>, NormalizeModelError> {
    match input {
        Value::Null => return Ok(None),
        Value::String(s) if s.is_empty() => return Ok(None),
        Value::String(_) => {}
        _ => return Err(NormalizeModelError::NotString),
    }

    let s = input.as_str().unwrap();
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    // JS `string.length` é UTF-16 code units; `encode_utf16().count()` bate.
    if trimmed.encode_utf16().count() > 80 {
        return Err(NormalizeModelError::TooLong);
    }
    Ok(Some(trimmed.to_string()))
}

/// Converte modelo armazenado no que o `claude --model` aceita.
/// Porta de `resolveCliModel` em `agents.cjs:686`.
///
/// - `None` / vazio / não-string → `None`.
/// - `"opus"`, `"sonnet"`, `"haiku"` → passthrough.
/// - Prefixo `"claude-"` → passthrough.
/// - Em [`CLI_MODEL_MAP`] → mapeado.
/// - Fallback regex `^(opus|sonnet|haiku)-(\d+)\.(\d+)$` →
///   `claude-X-Y-Z`.
/// - Senão → passthrough (deixa o CLI errar).
pub fn resolve_cli_model(model: &Value) -> Option<String> {
    let s = model.as_str()?;
    let v = s.trim();
    if v.is_empty() {
        return None;
    }
    if v == "opus" || v == "sonnet" || v == "haiku" {
        return Some(v.to_string());
    }
    if v.starts_with("claude-") {
        return Some(v.to_string());
    }
    for (k, target) in CLI_MODEL_MAP {
        if *k == v {
            return Some((*target).to_string());
        }
    }
    // fallback regex hand-rolled: `^(opus|sonnet|haiku)-(\d+)\.(\d+)$`
    if let Some(out) = try_fallback_pattern(v) {
        return Some(out);
    }
    Some(v.to_string())
}

fn try_fallback_pattern(v: &str) -> Option<String> {
    // extrai família
    let (family, rest) = if let Some(r) = v.strip_prefix("opus-") {
        ("opus", r)
    } else if let Some(r) = v.strip_prefix("sonnet-") {
        ("sonnet", r)
    } else if let Some(r) = v.strip_prefix("haiku-") {
        ("haiku", r)
    } else {
        return None;
    };
    // resto deve casar `\d+\.\d+` exato
    let (major, minor) = rest.split_once('.')?;
    if major.is_empty() || !major.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if minor.is_empty() || !minor.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("claude-{family}-{major}-{minor}"))
}

/// Erro de [`update_model`].
#[derive(Debug, Error)]
pub enum UpdateModelError {
    #[error(r#"agente "{name}" não existe em {path}"#)]
    AgentMissing { name: String, path: String },
    #[error("{0}")]
    Normalize(#[from] NormalizeModelError),
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

/// Atualiza `model` de um agente — porta de `updateModel`
/// em `agents.cjs:699`. Se `model` normaliza para `None`, remove
/// o campo.
pub fn update_model(
    mem_dir: &Path,
    name: &str,
    model: &Value,
) -> Result<Value, UpdateModelError> {
    let mut cfg = load_manifest(mem_dir).ok_or_else(|| UpdateModelError::AgentMissing {
        name: name.to_string(),
        path: manifest_path(mem_dir).display().to_string(),
    })?;

    let agent = cfg
        .get_mut("agents")
        .and_then(Value::as_object_mut)
        .and_then(|m| m.get_mut(name))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| UpdateModelError::AgentMissing {
            name: name.to_string(),
            path: manifest_path(mem_dir).display().to_string(),
        })?;

    match normalize_model(model)? {
        None => {
            agent.remove("model");
        }
        Some(v) => {
            agent.insert("model".to_string(), Value::String(v));
        }
    }

    let snapshot = Value::Object(agent.clone());
    save_manifest(mem_dir, &cfg)?;
    Ok(snapshot)
}

/// Erro de [`propagate_model_to_seed`].
#[derive(Debug, Error)]
pub enum PropagateModelError {
    #[error("seed agents.json não encontrado: {0}")]
    SeedMissing(String),
    #[error("seed agents.json inválido: {0}")]
    SeedParse(#[from] serde_json::Error),
    #[error("{0}")]
    Normalize(#[from] NormalizeModelError),
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

/// Propaga `model` para o seed. Porta de `propagateModelToSeed`
/// em `agents.cjs:714`. Shape de retorno:
/// - `Err(SeedMissing)` se arquivo ausente.
/// - `Ok(None)` se o seed não tem `agents[name]`.
/// - `Ok(Some(agent))` após gravar.
pub fn propagate_model_to_seed(
    seed_path: &Path,
    name: &str,
    model: &Value,
) -> Result<Option<Value>, PropagateModelError> {
    if !seed_path.exists() {
        return Err(PropagateModelError::SeedMissing(
            seed_path.display().to_string(),
        ));
    }
    let raw = fs::read_to_string(seed_path)?;
    let mut seed: Value = serde_json::from_str(&raw)?;

    let agents = match seed.get_mut("agents").and_then(Value::as_object_mut) {
        Some(a) => a,
        None => return Ok(None),
    };
    if !agents.contains_key(name) {
        return Ok(None);
    }

    let normalized = normalize_model(model)?;
    let agent = agents.get_mut(name).and_then(Value::as_object_mut).unwrap();
    match normalized {
        None => {
            agent.remove("model");
        }
        Some(v) => {
            agent.insert("model".to_string(), Value::String(v));
        }
    }

    let snapshot = Value::Object(agent.clone());
    let mut serialized = serde_json::to_string_pretty(&seed)?;
    serialized.push('\n');
    fs::write(seed_path, serialized)?;
    Ok(Some(snapshot))
}

#[allow(dead_code)]
fn _touch(_: &Path) -> Value {
    json!(null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── normalize_model ────────────────────────────────────────────────

    #[test]
    fn normalize_null_is_none() {
        assert_eq!(normalize_model(&json!(null)).unwrap(), None);
    }

    #[test]
    fn normalize_empty_string_is_none() {
        assert_eq!(normalize_model(&json!("")).unwrap(), None);
    }

    #[test]
    fn normalize_whitespace_string_is_none() {
        assert_eq!(normalize_model(&json!("   ")).unwrap(), None);
        assert_eq!(normalize_model(&json!("\t\n")).unwrap(), None);
    }

    #[test]
    fn normalize_non_string_errors() {
        assert!(matches!(
            normalize_model(&json!(42)),
            Err(NormalizeModelError::NotString)
        ));
        assert!(matches!(
            normalize_model(&json!(true)),
            Err(NormalizeModelError::NotString)
        ));
        assert!(matches!(
            normalize_model(&json!([])),
            Err(NormalizeModelError::NotString)
        ));
        assert!(matches!(
            normalize_model(&json!({})),
            Err(NormalizeModelError::NotString)
        ));
    }

    #[test]
    fn normalize_trims() {
        assert_eq!(
            normalize_model(&json!("  opus  ")).unwrap(),
            Some("opus".to_string())
        );
    }

    #[test]
    fn normalize_rejects_too_long() {
        let s = "a".repeat(81);
        assert!(matches!(
            normalize_model(&json!(s)),
            Err(NormalizeModelError::TooLong)
        ));
    }

    #[test]
    fn normalize_accepts_80_chars() {
        let s = "a".repeat(80);
        assert_eq!(normalize_model(&json!(s)).unwrap(), Some("a".repeat(80)));
    }

    // ── resolve_cli_model ──────────────────────────────────────────────

    #[test]
    fn resolve_null_and_non_string() {
        assert_eq!(resolve_cli_model(&json!(null)), None);
        assert_eq!(resolve_cli_model(&json!(42)), None);
        assert_eq!(resolve_cli_model(&json!("")), None);
        assert_eq!(resolve_cli_model(&json!("   ")), None);
    }

    #[test]
    fn resolve_pure_aliases() {
        assert_eq!(resolve_cli_model(&json!("opus")).unwrap(), "opus");
        assert_eq!(resolve_cli_model(&json!("sonnet")).unwrap(), "sonnet");
        assert_eq!(resolve_cli_model(&json!("haiku")).unwrap(), "haiku");
    }

    #[test]
    fn resolve_claude_prefix_passthrough() {
        assert_eq!(
            resolve_cli_model(&json!("claude-opus-4-7")).unwrap(),
            "claude-opus-4-7"
        );
        assert_eq!(
            resolve_cli_model(&json!("claude-weird-future-model")).unwrap(),
            "claude-weird-future-model"
        );
    }

    #[test]
    fn resolve_explicit_map() {
        assert_eq!(
            resolve_cli_model(&json!("opus-4.7")).unwrap(),
            "claude-opus-4-7"
        );
        assert_eq!(
            resolve_cli_model(&json!("sonnet-4.6")).unwrap(),
            "claude-sonnet-4-6"
        );
        assert_eq!(
            resolve_cli_model(&json!("haiku-4.5")).unwrap(),
            "claude-haiku-4-5-20251001"
        );
    }

    #[test]
    fn resolve_fallback_pattern() {
        // não está no map, mas casa regex → claude-X-Y-Z
        assert_eq!(
            resolve_cli_model(&json!("opus-9.9")).unwrap(),
            "claude-opus-9-9"
        );
        assert_eq!(
            resolve_cli_model(&json!("sonnet-10.0")).unwrap(),
            "claude-sonnet-10-0"
        );
    }

    #[test]
    fn resolve_unknown_passthrough() {
        assert_eq!(resolve_cli_model(&json!("foo-bar")).unwrap(), "foo-bar");
        assert_eq!(resolve_cli_model(&json!("random")).unwrap(), "random");
    }

    #[test]
    fn resolve_trims_before_matching() {
        assert_eq!(resolve_cli_model(&json!("  opus  ")).unwrap(), "opus");
        assert_eq!(
            resolve_cli_model(&json!("  opus-4.7  ")).unwrap(),
            "claude-opus-4-7"
        );
    }

    // ── update_model ───────────────────────────────────────────────────

    fn setup(dir: &Path, name: &str) {
        save_manifest(
            dir,
            &json!({
                "version": 1,
                "agents": {name: {"type": "specialist"}}
            }),
        )
        .unwrap();
    }

    #[test]
    fn update_model_sets_field() {
        let tmp = TempDir::new().unwrap();
        setup(tmp.path(), "m");
        let r = update_model(tmp.path(), "m", &json!("opus-4.7")).unwrap();
        assert_eq!(r["model"], "opus-4.7");
    }

    #[test]
    fn update_model_null_removes() {
        let tmp = TempDir::new().unwrap();
        save_manifest(
            tmp.path(),
            &json!({
                "version": 1,
                "agents": {"m": {"type": "specialist", "model": "opus"}}
            }),
        )
        .unwrap();
        let r = update_model(tmp.path(), "m", &json!(null)).unwrap();
        assert!(r.get("model").is_none());
    }

    #[test]
    fn update_model_agent_missing_errors() {
        let tmp = TempDir::new().unwrap();
        save_manifest(tmp.path(), &json!({"version":1,"agents":{}})).unwrap();
        let err = update_model(tmp.path(), "m", &json!("opus")).unwrap_err();
        assert!(matches!(err, UpdateModelError::AgentMissing { .. }));
    }

    #[test]
    fn update_model_manifest_missing_errors() {
        let tmp = TempDir::new().unwrap();
        let err = update_model(tmp.path(), "m", &json!("opus")).unwrap_err();
        assert!(matches!(err, UpdateModelError::AgentMissing { .. }));
    }

    #[test]
    fn update_model_invalid_propagates() {
        let tmp = TempDir::new().unwrap();
        setup(tmp.path(), "m");
        let err = update_model(tmp.path(), "m", &json!(42)).unwrap_err();
        assert!(matches!(err, UpdateModelError::Normalize(_)));
    }

    // ── propagate_model_to_seed ───────────────────────────────────────

    #[test]
    fn propagate_model_writes() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("seed.json");
        fs::write(&p, r#"{"agents": {"m": {"type": "specialist"}}}"#).unwrap();
        let r = propagate_model_to_seed(&p, "m", &json!("opus"))
            .unwrap()
            .unwrap();
        assert_eq!(r["model"], "opus");
        let on_disk = fs::read_to_string(&p).unwrap();
        assert!(on_disk.ends_with('\n'));
        assert!(on_disk.contains(r#""model": "opus""#));
    }

    #[test]
    fn propagate_model_null_removes() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("seed.json");
        fs::write(
            &p,
            r#"{"agents": {"m": {"type": "specialist", "model": "opus"}}}"#,
        )
        .unwrap();
        let r = propagate_model_to_seed(&p, "m", &json!(null))
            .unwrap()
            .unwrap();
        assert!(r.get("model").is_none());
    }

    #[test]
    fn propagate_model_no_agent_returns_none() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("seed.json");
        fs::write(&p, r#"{"agents": {"other": {}}}"#).unwrap();
        let r = propagate_model_to_seed(&p, "m", &json!("opus")).unwrap();
        assert_eq!(r, None);
    }

    #[test]
    fn propagate_model_seed_missing_errors() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("nope.json");
        let err = propagate_model_to_seed(&p, "m", &json!("opus")).unwrap_err();
        assert!(matches!(err, PropagateModelError::SeedMissing(_)));
    }
}
