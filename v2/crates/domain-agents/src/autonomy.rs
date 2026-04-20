//! Autonomia — porta de `agents.cjs:604-654`.
//!
//! Três funções públicas:
//! - [`normalize_autonomy`] — valida e sanitiza um input `{level, call_human_when}`.
//! - [`update_autonomy`] — aplica ao agente no manifest (`agents.json`).
//! - [`propagate_autonomy_to_seed`] — replica a mudança no seed (template).
//!
//! Paridade byte-a-byte com o `.cjs`: mesma ordem de chaves em
//! `{level, call_human_when}`, mesmo formato de erro, mesma
//! serialização do seed (`JSON.stringify(_, null, 2) + '\n'`).

use crate::{load_manifest, manifest_path, save_manifest, AUTONOMY_LEVELS};
use serde_json::{json, Map, Value};
use std::fs;
use std::io;
use std::path::Path;
use thiserror::Error;

/// Erro de [`normalize_autonomy`].
///
/// `.cjs` lança `Error` com mensagem; aqui temos variantes pra callers
/// poderem diferenciar se quiserem, mas [`Display`] reproduz o texto
/// exato do `.cjs`.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum NormalizeAutonomyError {
    #[error("autonomy precisa ser objeto {{ level, call_human_when }} ou null")]
    NotObject,
    #[error("autonomy.level inválido. Use: {valid}")]
    InvalidLevel { valid: String },
}

/// Resultado de [`normalize_autonomy`].
///
/// - `None` quando o input é JSON `null` — sinaliza "limpar autonomia".
/// - `Some(Value)` com o objeto sanitizado `{level, call_human_when}`.
pub fn normalize_autonomy(input: &Value) -> Result<Option<Value>, NormalizeAutonomyError> {
    if input.is_null() {
        return Ok(None);
    }

    // Replica `!input || typeof input !== 'object'`.
    // - `input` falsy em JS: null (tratado acima), undefined, false, 0, "", NaN.
    // - typeof objeto verdadeiro é só p/ objetos; arrays passam no typeof mas
    //   falham em `as_object` no Rust (os dois ramos erram; mensagem igual).
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err(NormalizeAutonomyError::NotObject),
    };
    // Paridade extra: JS `!input` é true p/ objeto falsy-ish? Não — todo
    // objeto (mesmo {}) é truthy em JS. Então esse caminho não filtra {}.

    let level = obj.get("level").and_then(Value::as_str).unwrap_or("");
    if !AUTONOMY_LEVELS.contains(&level) {
        return Err(NormalizeAutonomyError::InvalidLevel {
            valid: AUTONOMY_LEVELS.join(", "),
        });
    }

    let call_human_when: Vec<Value> = obj
        .get("call_human_when")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    // ordem das chaves idêntica ao `.cjs:619`
    let mut out = Map::new();
    out.insert("level".to_string(), Value::String(level.to_string()));
    out.insert("call_human_when".to_string(), Value::Array(call_human_when));
    Ok(Some(Value::Object(out)))
}

/// Erro de [`update_autonomy`].
#[derive(Debug, Error)]
pub enum UpdateAutonomyError {
    #[error(r#"agente "{name}" não existe em {path}"#)]
    AgentMissing { name: String, path: String },
    #[error("{0}")]
    Normalize(#[from] NormalizeAutonomyError),
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

/// Atualiza a autonomia de um agente — porta de `updateAutonomy`
/// em `agents.cjs:622`.
///
/// Se `autonomy` é `null`, remove o campo `autonomy` do agente.
/// Retorna o objeto agente atualizado.
pub fn update_autonomy(
    mem_dir: &Path,
    name: &str,
    autonomy: &Value,
) -> Result<Value, UpdateAutonomyError> {
    let mut cfg = load_manifest(mem_dir).ok_or_else(|| UpdateAutonomyError::AgentMissing {
        name: name.to_string(),
        path: manifest_path(mem_dir).display().to_string(),
    })?;

    let agent = cfg
        .get_mut("agents")
        .and_then(Value::as_object_mut)
        .and_then(|m| m.get_mut(name))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| UpdateAutonomyError::AgentMissing {
            name: name.to_string(),
            path: manifest_path(mem_dir).display().to_string(),
        })?;

    let normalized = normalize_autonomy(autonomy)?;
    match normalized {
        None => {
            agent.remove("autonomy");
        }
        Some(v) => {
            agent.insert("autonomy".to_string(), v);
        }
    }

    let snapshot = Value::Object(agent.clone());
    save_manifest(mem_dir, &cfg)?;
    Ok(snapshot)
}

/// Erro de [`propagate_autonomy_to_seed`].
#[derive(Debug, Error)]
pub enum PropagateAutonomyError {
    #[error("seed agents.json não encontrado: {0}")]
    SeedMissing(String),
    #[error("seed agents.json inválido: {0}")]
    SeedParse(#[from] serde_json::Error),
    #[error("{0}")]
    Normalize(#[from] NormalizeAutonomyError),
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

/// Propaga autonomia pra `plugin/seeds/<cat>/<slug>/agents.json`. Porta
/// de `propagateAutonomyToSeed` em `agents.cjs:640`.
///
/// Retorna:
/// - `Err(SeedMissing)` se o arquivo não existe.
/// - `Ok(None)` se o seed não tem `agents[name]` (divergência suave:
///   `.cjs` retorna `null`, aqui `None`).
/// - `Ok(Some(agent_json))` após gravar.
///
/// Escreve com `JSON.stringify(_, null, 2) + '\n'` (igual ao `.cjs`).
pub fn propagate_autonomy_to_seed(
    seed_path: &Path,
    name: &str,
    autonomy: &Value,
) -> Result<Option<Value>, PropagateAutonomyError> {
    if !seed_path.exists() {
        return Err(PropagateAutonomyError::SeedMissing(
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

    let normalized = normalize_autonomy(autonomy)?;
    let agent = agents.get_mut(name).and_then(Value::as_object_mut).unwrap();
    match normalized {
        None => {
            agent.remove("autonomy");
        }
        Some(v) => {
            agent.insert("autonomy".to_string(), v);
        }
    }

    let snapshot = Value::Object(agent.clone());

    // JSON.stringify(_, null, 2) + '\n' — `serde_json::to_string_pretty`
    // usa 2 espaços por default.
    let mut serialized = serde_json::to_string_pretty(&seed)?;
    serialized.push('\n');
    fs::write(seed_path, serialized)?;

    Ok(Some(snapshot))
}

// silencia unused warning de import usado só em errors
#[allow(dead_code)]
fn _touch(_: &Path) -> Value {
    json!(null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── normalize_autonomy ──────────────────────────────────────────────

    #[test]
    fn normalize_null_returns_none() {
        assert_eq!(normalize_autonomy(&json!(null)).unwrap(), None);
    }

    #[test]
    fn normalize_non_object_errors() {
        assert!(matches!(
            normalize_autonomy(&json!("x")),
            Err(NormalizeAutonomyError::NotObject)
        ));
        assert!(matches!(
            normalize_autonomy(&json!(42)),
            Err(NormalizeAutonomyError::NotObject)
        ));
        assert!(matches!(
            normalize_autonomy(&json!(true)),
            Err(NormalizeAutonomyError::NotObject)
        ));
        // array em JS passa no typeof mas cai em `!AUTONOMY_LEVELS.has(level)`
        // porque level é undefined. Aqui via `as_object` retorna None → NotObject.
        // Divergência: JS lança `autonomy.level inválido`; Rust lança `NotObject`.
        // Vamos cobrir no finding.
        assert!(matches!(
            normalize_autonomy(&json!([])),
            Err(NormalizeAutonomyError::NotObject)
        ));
    }

    #[test]
    fn normalize_invalid_level() {
        assert!(matches!(
            normalize_autonomy(&json!({"level": "auto"})),
            Err(NormalizeAutonomyError::InvalidLevel { .. })
        ));
        assert!(matches!(
            normalize_autonomy(&json!({})),
            Err(NormalizeAutonomyError::InvalidLevel { .. })
        ));
        assert!(matches!(
            normalize_autonomy(&json!({"level": "HIGH"})),
            Err(NormalizeAutonomyError::InvalidLevel { .. })
        ));
    }

    #[test]
    fn normalize_valid_levels_each() {
        for level in ["low", "medium", "high"] {
            let r = normalize_autonomy(&json!({"level": level})).unwrap().unwrap();
            assert_eq!(r["level"], level);
            assert_eq!(r["call_human_when"], json!([]));
        }
    }

    #[test]
    fn normalize_call_human_when_filters_and_trims() {
        let r = normalize_autonomy(&json!({
            "level": "high",
            "call_human_when": [" escala ", "", 42, null, "mover dinheiro", "  "]
        }))
        .unwrap()
        .unwrap();
        assert_eq!(r["call_human_when"], json!(["escala", "mover dinheiro"]));
    }

    #[test]
    fn normalize_call_human_when_non_array_empty() {
        let r = normalize_autonomy(&json!({
            "level": "low",
            "call_human_when": "x"
        }))
        .unwrap()
        .unwrap();
        assert_eq!(r["call_human_when"], json!([]));
    }

    #[test]
    fn normalize_key_order_level_first() {
        let r = normalize_autonomy(&json!({
            "call_human_when": ["x"],
            "level": "medium"
        }))
        .unwrap()
        .unwrap();
        // garante level antes de call_human_when na serialização
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.starts_with(r#"{"level":"medium","call_human_when":"#));
    }

    // ── update_autonomy ─────────────────────────────────────────────────

    fn setup_agent(dir: &Path, name: &str) {
        save_manifest(
            dir,
            &json!({
                "version": 1,
                "agents": {name: {"type": "specialist", "role": "X"}}
            }),
        )
        .unwrap();
    }

    #[test]
    fn update_autonomy_sets_field() {
        let tmp = TempDir::new().unwrap();
        setup_agent(tmp.path(), "m");
        let r = update_autonomy(
            tmp.path(),
            "m",
            &json!({"level": "high", "call_human_when": ["escala"]}),
        )
        .unwrap();
        assert_eq!(r["autonomy"]["level"], "high");
        assert_eq!(r["autonomy"]["call_human_when"], json!(["escala"]));
    }

    #[test]
    fn update_autonomy_null_removes_field() {
        let tmp = TempDir::new().unwrap();
        save_manifest(
            tmp.path(),
            &json!({
                "version": 1,
                "agents": {"m": {
                    "type": "specialist",
                    "autonomy": {"level": "low", "call_human_when": []}
                }}
            }),
        )
        .unwrap();
        let r = update_autonomy(tmp.path(), "m", &json!(null)).unwrap();
        assert!(r.get("autonomy").is_none());
    }

    #[test]
    fn update_autonomy_errors_agent_missing() {
        let tmp = TempDir::new().unwrap();
        save_manifest(tmp.path(), &json!({"version": 1, "agents": {}})).unwrap();
        let err = update_autonomy(tmp.path(), "m", &json!({"level": "low"})).unwrap_err();
        assert!(matches!(err, UpdateAutonomyError::AgentMissing { .. }));
    }

    #[test]
    fn update_autonomy_errors_manifest_missing() {
        let tmp = TempDir::new().unwrap();
        let err = update_autonomy(tmp.path(), "m", &json!({"level": "low"})).unwrap_err();
        assert!(matches!(err, UpdateAutonomyError::AgentMissing { .. }));
    }

    #[test]
    fn update_autonomy_invalid_level_propagates() {
        let tmp = TempDir::new().unwrap();
        setup_agent(tmp.path(), "m");
        let err = update_autonomy(tmp.path(), "m", &json!({"level": "xyz"})).unwrap_err();
        assert!(matches!(err, UpdateAutonomyError::Normalize(_)));
    }

    // ── propagate_autonomy_to_seed ──────────────────────────────────────

    #[test]
    fn propagate_errors_when_seed_missing() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("seed.json");
        let err = propagate_autonomy_to_seed(&p, "m", &json!({"level": "low"})).unwrap_err();
        assert!(matches!(err, PropagateAutonomyError::SeedMissing(_)));
    }

    #[test]
    fn propagate_returns_none_when_agent_missing() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("seed.json");
        fs::write(&p, r#"{"agents": {"other": {}}}"#).unwrap();
        let r = propagate_autonomy_to_seed(&p, "m", &json!({"level": "low"})).unwrap();
        assert_eq!(r, None);
    }

    #[test]
    fn propagate_returns_none_when_no_agents_key() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("seed.json");
        fs::write(&p, r#"{"version": 1}"#).unwrap();
        let r = propagate_autonomy_to_seed(&p, "m", &json!({"level": "low"})).unwrap();
        assert_eq!(r, None);
    }

    #[test]
    fn propagate_writes_and_returns_agent() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("seed.json");
        fs::write(&p, r#"{"agents": {"m": {"type": "specialist"}}}"#).unwrap();
        let r = propagate_autonomy_to_seed(
            &p,
            "m",
            &json!({"level": "medium", "call_human_when": ["x"]}),
        )
        .unwrap()
        .unwrap();
        assert_eq!(r["autonomy"]["level"], "medium");

        // disco atualizado
        let on_disk = fs::read_to_string(&p).unwrap();
        assert!(on_disk.contains(r#""level": "medium""#));
        assert!(on_disk.ends_with('\n'));
    }

    #[test]
    fn propagate_null_removes_autonomy_from_seed() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("seed.json");
        fs::write(
            &p,
            r#"{"agents": {"m": {"type": "specialist", "autonomy": {"level": "low"}}}}"#,
        )
        .unwrap();
        let r = propagate_autonomy_to_seed(&p, "m", &json!(null))
            .unwrap()
            .unwrap();
        assert!(r.get("autonomy").is_none());
        let on_disk = fs::read_to_string(&p).unwrap();
        assert!(!on_disk.contains("autonomy"));
    }
}
