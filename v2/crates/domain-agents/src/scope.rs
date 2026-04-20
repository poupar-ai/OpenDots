//! Escopo (sandbox) — porta de `agents.cjs:730-779`.
//!
//! O campo `scope` complementa `tools` com controles finos:
//! - `bash_allow`: padrões permitidos pro Bash (ex: `"git *"`). Sem
//!   esta lista, Bash é efetivamente desabilitado no runtime mesmo
//!   que `'Bash'` apareça em `tools`.
//!
//! Denylist global de Bash (rm, sudo, …) fica no server — não aqui.

use crate::{load_manifest, manifest_path, save_manifest};
use serde_json::{json, Map, Value};
use std::io;
use std::path::Path;
use thiserror::Error;

/// Nomes de ferramentas aceitos. Porta literal de
/// `agents.cjs:739-742`.
pub const KNOWN_TOOL_NAMES: &[&str] = &[
    "Read",
    "Write",
    "Edit",
    "Bash",
    "Grep",
    "Glob",
    "WebFetch",
    "WebSearch",
    "Task",
    "TodoWrite",
    "NotebookEdit",
];

/// `true` se `name` é uma ferramenta reconhecida.
pub fn is_known_tool(name: &str) -> bool {
    KNOWN_TOOL_NAMES.contains(&name)
}

/// Sanitiza uma lista de tools do input do usuário.
///
/// Retorna `None` (equivalente a `undefined` do JS) se `input` não for
/// array — o caller usa isso como sentinela "não tocar em `tools`".
///
/// Caso contrário, retorna `Some(Vec<String>)` com:
/// - somente strings
/// - trimmed
/// - apenas as que estão em [`KNOWN_TOOL_NAMES`]
/// - deduplicadas preservando ordem de primeira aparição
///
/// Paridade com `sanitizeTools` em `agents.cjs:744`.
pub fn sanitize_tools(input: &Value) -> Option<Vec<String>> {
    let arr = input.as_array()?;
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in arr {
        let s = match item.as_str() {
            Some(s) => s.trim(),
            None => continue,
        };
        if s.is_empty() || !is_known_tool(s) || seen.contains(s) {
            continue;
        }
        seen.insert(s.to_string());
        out.push(s.to_string());
    }
    Some(out)
}

/// Sanitiza o objeto `scope`.
///
/// - Se `input` não for objeto, retorna `{ bash_allow: [] }`.
/// - Se `input.bash_allow` não for array, retorna `{ bash_allow: [] }`.
/// - Caso contrário: filtra strings não-vazias ≤ 200 chars, trim,
///   limita a 50 entradas preservando ordem.
///
/// Paridade com `sanitizeScope` em `agents.cjs:758`.
pub fn sanitize_scope(input: &Value) -> Value {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return json!({ "bash_allow": [] }),
    };
    let bash_allow = match obj.get("bash_allow").and_then(Value::as_array) {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .filter(|p| {
                let t = p.trim();
                !t.is_empty() && p.len() <= 200
            })
            .map(|p| p.trim().to_string())
            .take(50)
            .collect::<Vec<_>>(),
        None => Vec::new(),
    };
    json!({ "bash_allow": bash_allow })
}

/// Erros de [`update_scope`].
#[derive(Debug, Error)]
pub enum UpdateScopeError {
    /// Manifest ausente, sem `agents`, ou agente não existe.
    #[error(r#"agente "{name}" não existe em {path}"#)]
    AgentMissing { name: String, path: String },
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

/// Atualiza `tools` e/ou `scope` de um agente. Ambos opcionais.
///
/// - Se `tools` passa em `sanitize_tools` (ou seja, é array) → sobrescreve.
/// - Se `scope` for `Some(_)` → sobrescreve com `sanitize_scope`.
///
/// Paridade com `updateScope` em `agents.cjs:769`.
pub fn update_scope(
    mem_dir: &Path,
    name: &str,
    tools: Option<&Value>,
    scope: Option<&Value>,
) -> Result<Value, UpdateScopeError> {
    let mut cfg = load_manifest(mem_dir).ok_or_else(|| UpdateScopeError::AgentMissing {
        name: name.to_string(),
        path: manifest_path(mem_dir).display().to_string(),
    })?;

    let agent = cfg
        .get_mut("agents")
        .and_then(Value::as_object_mut)
        .and_then(|m| m.get_mut(name))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| UpdateScopeError::AgentMissing {
            name: name.to_string(),
            path: manifest_path(mem_dir).display().to_string(),
        })?;

    if let Some(t_input) = tools {
        if let Some(clean) = sanitize_tools(t_input) {
            let arr: Vec<Value> = clean.into_iter().map(Value::String).collect();
            agent.insert("tools".to_string(), Value::Array(arr));
        }
    }
    if let Some(s_input) = scope {
        agent.insert("scope".to_string(), sanitize_scope(s_input));
    }

    let snapshot: Map<String, Value> = agent.clone();
    save_manifest(mem_dir, &cfg)?;
    Ok(Value::Object(snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    // ── sanitize_tools ──────────────────────────────────────────────────

    #[test]
    fn sanitize_tools_non_array_returns_none() {
        assert!(sanitize_tools(&json!("x")).is_none());
        assert!(sanitize_tools(&json!(42)).is_none());
        assert!(sanitize_tools(&json!(null)).is_none());
        assert!(sanitize_tools(&json!({})).is_none());
    }

    #[test]
    fn sanitize_tools_filters_known() {
        let r = sanitize_tools(&json!(["Read", "Write", "Unknown", "Grep"])).unwrap();
        assert_eq!(r, vec!["Read", "Write", "Grep"]);
    }

    #[test]
    fn sanitize_tools_dedupes_preserving_order() {
        let r = sanitize_tools(&json!(["Read", "Read", "Write", "Read"])).unwrap();
        assert_eq!(r, vec!["Read", "Write"]);
    }

    #[test]
    fn sanitize_tools_trims_and_filters_non_string() {
        let r = sanitize_tools(&json!([" Read ", 42, null, "Write"])).unwrap();
        assert_eq!(r, vec!["Read", "Write"]);
    }

    #[test]
    fn sanitize_tools_empty_array() {
        let r = sanitize_tools(&json!([])).unwrap();
        assert!(r.is_empty());
    }

    // ── sanitize_scope ──────────────────────────────────────────────────

    #[test]
    fn sanitize_scope_non_object_returns_empty() {
        assert_eq!(sanitize_scope(&json!(null)), json!({"bash_allow": []}));
        assert_eq!(sanitize_scope(&json!("x")), json!({"bash_allow": []}));
        assert_eq!(sanitize_scope(&json!(42)), json!({"bash_allow": []}));
    }

    #[test]
    fn sanitize_scope_missing_bash_allow_defaults_to_empty() {
        assert_eq!(sanitize_scope(&json!({})), json!({"bash_allow": []}));
    }

    #[test]
    fn sanitize_scope_filters_non_string_and_empty() {
        let r = sanitize_scope(&json!({
            "bash_allow": ["git *", "", 42, null, "npm run *"]
        }));
        assert_eq!(r, json!({"bash_allow": ["git *", "npm run *"]}));
    }

    #[test]
    fn sanitize_scope_trims_entries() {
        let r = sanitize_scope(&json!({
            "bash_allow": ["  git *  ", " ls -la "]
        }));
        assert_eq!(r, json!({"bash_allow": ["git *", "ls -la"]}));
    }

    #[test]
    fn sanitize_scope_rejects_entry_over_200_chars() {
        let long = "a".repeat(201);
        let short = "a".repeat(200);
        let r = sanitize_scope(&json!({
            "bash_allow": [long, short]
        }));
        assert_eq!(r["bash_allow"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn sanitize_scope_caps_at_50_entries() {
        let many: Vec<String> = (0..60).map(|i| format!("cmd{i}")).collect();
        let r = sanitize_scope(&json!({"bash_allow": many}));
        assert_eq!(r["bash_allow"].as_array().unwrap().len(), 50);
    }

    #[test]
    fn sanitize_scope_bash_allow_not_array_defaults_empty() {
        let r = sanitize_scope(&json!({"bash_allow": "git *"}));
        assert_eq!(r, json!({"bash_allow": []}));
    }

    // ── update_scope ────────────────────────────────────────────────────

    #[test]
    fn update_scope_errors_when_manifest_missing() {
        let tmp = TempDir::new().unwrap();
        let err = update_scope(tmp.path(), "m", None, None).unwrap_err();
        assert!(matches!(err, UpdateScopeError::AgentMissing { .. }));
    }

    #[test]
    fn update_scope_errors_when_agent_missing() {
        let tmp = TempDir::new().unwrap();
        save_manifest(
            tmp.path(),
            &json!({"version": 1, "agents": {"other": {}}}),
        )
        .unwrap();
        let err = update_scope(tmp.path(), "m", None, None).unwrap_err();
        assert!(matches!(err, UpdateScopeError::AgentMissing { .. }));
    }

    #[test]
    fn update_scope_updates_tools() {
        let tmp = TempDir::new().unwrap();
        save_manifest(
            tmp.path(),
            &json!({"version": 1, "agents": {"m": {"type": "core"}}}),
        )
        .unwrap();
        let new_tools = json!(["Read", "Write"]);
        update_scope(tmp.path(), "m", Some(&new_tools), None).unwrap();
        let cfg = load_manifest(tmp.path()).unwrap();
        assert_eq!(
            cfg["agents"]["m"]["tools"],
            json!(["Read", "Write"])
        );
    }

    #[test]
    fn update_scope_updates_scope_field() {
        let tmp = TempDir::new().unwrap();
        save_manifest(
            tmp.path(),
            &json!({"version": 1, "agents": {"m": {"type": "core"}}}),
        )
        .unwrap();
        let scope = json!({"bash_allow": ["git *"]});
        update_scope(tmp.path(), "m", None, Some(&scope)).unwrap();
        let cfg = load_manifest(tmp.path()).unwrap();
        assert_eq!(
            cfg["agents"]["m"]["scope"],
            json!({"bash_allow": ["git *"]})
        );
    }

    #[test]
    fn update_scope_non_array_tools_does_not_touch() {
        let tmp = TempDir::new().unwrap();
        save_manifest(
            tmp.path(),
            &json!({
                "version": 1,
                "agents": {"m": {"type": "core", "tools": ["Read"]}}
            }),
        )
        .unwrap();
        let bogus = json!("not-array");
        update_scope(tmp.path(), "m", Some(&bogus), None).unwrap();
        let cfg = load_manifest(tmp.path()).unwrap();
        // tools não mudou
        assert_eq!(cfg["agents"]["m"]["tools"], json!(["Read"]));
    }
}
