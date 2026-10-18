//! Add/remove agents — porta de `agents.cjs:551-602` e `:815-826`.
//!
//! - [`add_agent`] valida nome (regex `[a-z0-9_-]+`), tipo (contra
//!   [`crate::AGENT_TYPES`]), existência prévia; monta o agente com
//!   defaults idênticos ao `.cjs` e cria o mini-context se ausente.
//! - [`remove_agent`] tira o agente do manifest e apaga o mini-context.
//!
//! Ordem das chaves do objeto agente é preservada (IndexMap via
//! `serde_json` com `preserve_order`) para byte-paridade com o `.cjs`.

use crate::model::DEFAULT_MODEL;
use crate::{load_manifest, manifest_path, sanitize_scope, save_manifest, AGENT_TYPES};
use serde_json::{json, Map, Value};
use std::fs;
use std::io;
use std::path::Path;
use thiserror::Error;

/// `true` se `name` é um identificador válido de agente —
/// regex `^[a-z0-9_-]+$` do `.cjs:552`.
fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// JS `truthy` check pra strings: `undefined`, `null`, `""` → falsy.
/// Usado pra replicar `config.emoji || default` do `.cjs`.
fn js_str_or<'a>(config: &'a Value, key: &str, default: &'a str) -> String {
    config
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(default)
        .to_string()
}

/// JS `config.key || default` pra arrays. Arrays vazios `[]` são
/// **truthy** em JS (preservados), só `undefined`/`null` caem no default.
fn js_arr_or(config: &Value, key: &str, default: Value) -> Value {
    match config.get(key) {
        Some(v) if v.is_array() => v.clone(),
        _ => default,
    }
}

/// Emoji default por tipo de agente — `.cjs:566`.
fn default_emoji_for(type_: &str) -> &'static str {
    match type_ {
        "integrator" => "🔌",
        "motor" => "⚙️",
        "firefighter" => "🚒",
        _ => "•",
    }
}

/// Erros de [`add_agent`].
#[derive(Debug, Error)]
pub enum AddAgentError {
    #[error(r#"Nome inválido. Use só letras minúsculas, números, "-" ou "_"."#)]
    InvalidName,
    #[error("Tipo inválido: {got}. Use um de: {valid}")]
    InvalidType { got: String, valid: String },
    #[error(r#"Agente "{name}" já existe. Use edit ou remove primeiro."#)]
    AlreadyExists { name: String },
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

/// Adiciona um novo agente ao manifest. Porta de
/// `addAgent` em `agents.cjs:551`.
///
/// Se o manifest não existir, cria um mínimo
/// `{version:1, manager:"manager", oracle:"oracle", agents:{}}`.
///
/// Cria o mini-context (`memDir/<context_file>`) se ainda não existir,
/// com conteúdo vindo de `config.context` ou um template mínimo.
pub fn add_agent(mem_dir: &Path, name: &str, config: &Value) -> Result<Value, AddAgentError> {
    if !is_valid_name(name) {
        return Err(AddAgentError::InvalidName);
    }

    let type_ = config.get("type").and_then(Value::as_str).unwrap_or("");
    if !crate::is_valid_agent_type(type_) {
        return Err(AddAgentError::InvalidType {
            got: type_.to_string(),
            valid: AGENT_TYPES.join(", "),
        });
    }

    let mut cfg = load_manifest(mem_dir).unwrap_or_else(|| {
        json!({
            "version": 1,
            "manager": "manager",
            "oracle": "oracle",
            "agents": {}
        })
    });

    // garante que cfg.agents exista (evita panic se manifest externo
    // tiver shape estranho; .cjs crashava com TypeError aqui)
    if !cfg.get("agents").map(Value::is_object).unwrap_or(false) {
        cfg["agents"] = json!({});
    }

    if cfg["agents"]
        .as_object()
        .unwrap()
        .contains_key(name)
    {
        return Err(AddAgentError::AlreadyExists {
            name: name.to_string(),
        });
    }

    // context_file default
    let context_file = js_str_or(config, "context_file", &format!("agents/{name}.md"));

    // gender: só 'male' | 'female' | 'other'
    let gender_clean = config
        .get("gender")
        .and_then(Value::as_str)
        .filter(|g| matches!(*g, "male" | "female" | "other"))
        .map(str::to_string);

    let emoji_default = default_emoji_for(type_);
    let emoji = js_str_or(config, "emoji", emoji_default);
    let model = js_str_or(config, "model", DEFAULT_MODEL);
    let role = js_str_or(config, "role", "");
    let tools = js_arr_or(
        config,
        "tools",
        json!(["Read", "Grep", "Glob"]),
    );
    let mcps = js_arr_or(config, "mcps", json!([]));
    let can_talk_to = js_arr_or(config, "can_talk_to", json!(["manager"]));
    let scope = sanitize_scope(config.get("scope").unwrap_or(&Value::Null));

    // monta o agente na MESMA ORDEM de chaves do .cjs
    let mut agent = Map::new();
    agent.insert("type".to_string(), Value::String(type_.to_string()));
    agent.insert("emoji".to_string(), Value::String(emoji));
    if let Some(g) = gender_clean {
        agent.insert("gender".to_string(), Value::String(g));
    }
    agent.insert("model".to_string(), Value::String(model));
    agent.insert("role".to_string(), Value::String(role.clone()));
    agent.insert("context_file".to_string(), Value::String(context_file.clone()));
    agent.insert("tools".to_string(), tools);
    agent.insert("mcps".to_string(), mcps);
    agent.insert("can_talk_to".to_string(), can_talk_to);
    agent.insert("scope".to_string(), scope);

    if type_ == "firefighter" {
        if let Some(w) = config.get("watch").filter(|v| !v.is_null()) {
            let mut watch = Map::new();
            watch.insert(
                "source".to_string(),
                Value::String(js_str_or(w, "source", "datadog")),
            );
            watch.insert(
                "query".to_string(),
                Value::String(js_str_or(w, "query", "")),
            );
            watch.insert("threshold".to_string(), js_num_or(w, "threshold", 5));
            watch.insert(
                "window_minutes".to_string(),
                js_num_or(w, "window_minutes", 5),
            );
            watch.insert(
                "cooldown_seconds".to_string(),
                js_num_or(w, "cooldown_seconds", 900),
            );
            watch.insert(
                "max_triages_per_hour".to_string(),
                js_num_or(w, "max_triages_per_hour", 4),
            );
            watch.insert(
                "env_vars".to_string(),
                js_arr_or(
                    w,
                    "env_vars",
                    json!(["DD_API_KEY", "DD_APP_KEY", "DD_SITE"]),
                ),
            );
            agent.insert("watch".to_string(), Value::Object(watch));
        }
    }

    let agent_value = Value::Object(agent);
    cfg["agents"]
        .as_object_mut()
        .unwrap()
        .insert(name.to_string(), agent_value.clone());

    save_manifest(mem_dir, &cfg)?;

    // mini-context: cria se não existir (sem adicionar newline extra —
    // .cjs grava byte-a-byte o que veio em config.context)
    let ctx_path = mem_dir.join(&context_file);
    if !ctx_path.exists() {
        if let Some(parent) = ctx_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let default_ctx = format!(
            "# {name}\n\n## Responsabilidade\n{}\n",
            if role.is_empty() { "_A preencher_" } else { &role }
        );
        let content = config
            .get("context")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or(default_ctx);
        fs::write(&ctx_path, content)?;
    }

    Ok(agent_value)
}

/// JS `watch.threshold || 5` — trata `0` como falsy (cai no default),
/// igual ao `.cjs`. Valores numéricos são preservados como [`Value::Number`].
fn js_num_or(v: &Value, key: &str, default: i64) -> Value {
    match v.get(key) {
        Some(n) if n.is_number() => {
            // 0 é falsy em JS → default. Para float 0.0 também.
            if n.as_f64() == Some(0.0) {
                json!(default)
            } else {
                n.clone()
            }
        }
        _ => json!(default),
    }
}

/// Erros de [`remove_agent`].
#[derive(Debug, Error)]
pub enum RemoveAgentError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

/// Remove agente do manifest e apaga seu mini-context.
///
/// Retorna `false` se manifest/agente não existe (sem erro), `true`
/// em caso de remoção bem-sucedida. Paridade com `removeAgent`
/// em `agents.cjs:815`, exceto por um ponto defensivo:
/// se `cfg.agents` não for objeto, Rust retorna `Ok(false)` em vez de
/// panic (ver F3-09 em `docs/migration_findings.md`).
pub fn remove_agent(mem_dir: &Path, name: &str) -> Result<bool, RemoveAgentError> {
    let mut cfg = match load_manifest(mem_dir) {
        Some(c) => c,
        None => return Ok(false),
    };

    let agents = match cfg.get_mut("agents").and_then(Value::as_object_mut) {
        Some(a) => a,
        None => return Ok(false),
    };

    let removed = match agents.remove(name) {
        Some(v) => v,
        None => return Ok(false),
    };

    let ctx_file = removed
        .get("context_file")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("agents/{name}.md"));

    save_manifest(mem_dir, &cfg)?;

    let ctx_path = mem_dir.join(&ctx_file);
    if ctx_path.exists() {
        let _ = fs::remove_file(&ctx_path); // .cjs engole erros
    }

    Ok(true)
}

// Re-export for convenience
pub use crate::AGENT_TYPES as _AGENT_TYPES;

// Silencia warning de import não usado — `manifest_path` só aparece na mensagem
// de erro interna (o `.cjs` cita o path). Como não temos essa mensagem aqui,
// mantemos o import pra consistência com o resto do módulo.
#[allow(dead_code)]
fn _touch_imports(p: &Path) -> std::path::PathBuf {
    manifest_path(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── is_valid_name ──────────────────────────────────────────────────

    #[test]
    fn valid_name_accepts_lowercase_digits_underscore_dash() {
        assert!(is_valid_name("foo"));
        assert!(is_valid_name("a_b-c_123"));
        assert!(is_valid_name("x"));
        assert!(is_valid_name("test-agent_1"));
    }

    #[test]
    fn valid_name_rejects_uppercase_and_special() {
        assert!(!is_valid_name("Foo"));
        assert!(!is_valid_name("foo bar"));
        assert!(!is_valid_name("foo.bar"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("foo/bar"));
    }

    // ── add_agent ──────────────────────────────────────────────────────

    #[test]
    fn add_agent_rejects_invalid_name() {
        let tmp = TempDir::new().unwrap();
        let err = add_agent(tmp.path(), "Foo", &json!({"type": "specialist"})).unwrap_err();
        assert!(matches!(err, AddAgentError::InvalidName));
    }

    #[test]
    fn add_agent_rejects_invalid_type() {
        let tmp = TempDir::new().unwrap();
        let err = add_agent(tmp.path(), "foo", &json!({"type": "xyz"})).unwrap_err();
        assert!(matches!(err, AddAgentError::InvalidType { .. }));
    }

    #[test]
    fn add_agent_creates_manifest_if_missing() {
        let tmp = TempDir::new().unwrap();
        let r = add_agent(
            tmp.path(),
            "foo",
            &json!({"type": "specialist", "role": "Tester"}),
        )
        .unwrap();
        assert_eq!(r["type"], "specialist");
        let cfg = load_manifest(tmp.path()).unwrap();
        assert_eq!(cfg["manager"], "manager");
        assert_eq!(cfg["oracle"], "oracle");
        assert_eq!(cfg["agents"]["foo"]["role"], "Tester");
    }

    #[test]
    fn add_agent_defaults_applied() {
        let tmp = TempDir::new().unwrap();
        let r = add_agent(tmp.path(), "foo", &json!({"type": "specialist"})).unwrap();
        assert_eq!(r["emoji"], "•");
        assert_eq!(r["model"], "opus-5.5");
        assert_eq!(r["role"], "");
        assert_eq!(r["context_file"], "agents/foo.md");
        assert_eq!(r["tools"], json!(["Read", "Grep", "Glob"]));
        assert_eq!(r["mcps"], json!([]));
        assert_eq!(r["can_talk_to"], json!(["manager"]));
        assert_eq!(r["scope"], json!({"bash_allow": []}));
    }

    #[test]
    fn add_agent_emoji_default_per_type() {
        let tmp = TempDir::new().unwrap();
        let motor = add_agent(tmp.path(), "m1", &json!({"type": "motor"})).unwrap();
        assert_eq!(motor["emoji"], "⚙️");
        let ff = add_agent(tmp.path(), "f1", &json!({"type": "firefighter"})).unwrap();
        assert_eq!(ff["emoji"], "🚒");
        let integ = add_agent(tmp.path(), "i1", &json!({"type": "integrator"})).unwrap();
        assert_eq!(integ["emoji"], "🔌");
    }

    #[test]
    fn add_agent_gender_filter() {
        let tmp = TempDir::new().unwrap();
        let ok = add_agent(
            tmp.path(),
            "a",
            &json!({"type": "specialist", "gender": "female"}),
        )
        .unwrap();
        assert_eq!(ok["gender"], "female");

        let bad = add_agent(
            tmp.path(),
            "b",
            &json!({"type": "specialist", "gender": "xyz"}),
        )
        .unwrap();
        assert!(bad.get("gender").is_none(), "gender inválido não é gravado");
    }

    #[test]
    fn add_agent_rejects_duplicate() {
        let tmp = TempDir::new().unwrap();
        add_agent(tmp.path(), "foo", &json!({"type": "specialist"})).unwrap();
        let err = add_agent(tmp.path(), "foo", &json!({"type": "specialist"})).unwrap_err();
        assert!(matches!(err, AddAgentError::AlreadyExists { .. }));
    }

    #[test]
    fn add_agent_creates_mini_context_default_template() {
        let tmp = TempDir::new().unwrap();
        add_agent(
            tmp.path(),
            "foo",
            &json!({"type": "specialist", "role": "Auditor"}),
        )
        .unwrap();
        let md = fs::read_to_string(tmp.path().join("agents/foo.md")).unwrap();
        assert!(md.contains("# foo"));
        assert!(md.contains("## Responsabilidade"));
        assert!(md.contains("Auditor"));
    }

    #[test]
    fn add_agent_uses_custom_context() {
        let tmp = TempDir::new().unwrap();
        add_agent(
            tmp.path(),
            "foo",
            &json!({"type": "specialist", "context": "custom body"}),
        )
        .unwrap();
        let md = fs::read_to_string(tmp.path().join("agents/foo.md")).unwrap();
        assert_eq!(md, "custom body");
    }

    #[test]
    fn add_agent_preserves_existing_context_file() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("agents")).unwrap();
        fs::write(tmp.path().join("agents/foo.md"), "pre-existing").unwrap();
        add_agent(tmp.path(), "foo", &json!({"type": "specialist"})).unwrap();
        let md = fs::read_to_string(tmp.path().join("agents/foo.md")).unwrap();
        assert_eq!(md, "pre-existing");
    }

    #[test]
    fn add_agent_firefighter_watch_defaults() {
        let tmp = TempDir::new().unwrap();
        let r = add_agent(
            tmp.path(),
            "ff",
            &json!({"type": "firefighter", "watch": {}}),
        )
        .unwrap();
        assert_eq!(r["watch"]["source"], "datadog");
        assert_eq!(r["watch"]["threshold"], 5);
        assert_eq!(r["watch"]["window_minutes"], 5);
        assert_eq!(r["watch"]["cooldown_seconds"], 900);
        assert_eq!(r["watch"]["max_triages_per_hour"], 4);
    }

    #[test]
    fn add_agent_firefighter_no_watch_if_not_firefighter() {
        let tmp = TempDir::new().unwrap();
        let r = add_agent(
            tmp.path(),
            "s",
            &json!({"type": "specialist", "watch": {"source": "x"}}),
        )
        .unwrap();
        assert!(r.get("watch").is_none());
    }

    #[test]
    fn add_agent_custom_tools_and_mcps() {
        let tmp = TempDir::new().unwrap();
        let r = add_agent(
            tmp.path(),
            "foo",
            &json!({
                "type": "motor",
                "tools": ["Read", "Bash"],
                "mcps": [{"name": "gh", "server": "x"}]
            }),
        )
        .unwrap();
        assert_eq!(r["tools"], json!(["Read", "Bash"]));
        assert_eq!(r["mcps"][0]["name"], "gh");
    }

    #[test]
    fn add_agent_empty_array_tools_preserved() {
        // JS: `[] || default` retorna `[]` (array é truthy)
        let tmp = TempDir::new().unwrap();
        let r = add_agent(
            tmp.path(),
            "foo",
            &json!({"type": "specialist", "tools": []}),
        )
        .unwrap();
        assert_eq!(r["tools"], json!([]));
    }

    #[test]
    fn add_agent_scope_sanitized() {
        let tmp = TempDir::new().unwrap();
        let r = add_agent(
            tmp.path(),
            "foo",
            &json!({
                "type": "specialist",
                "scope": {"bash_allow": ["git *", "", 42, "ls"]}
            }),
        )
        .unwrap();
        assert_eq!(r["scope"], json!({"bash_allow": ["git *", "ls"]}));
    }

    // ── remove_agent ────────────────────────────────────────────────────

    #[test]
    fn remove_agent_no_manifest_returns_false() {
        let tmp = TempDir::new().unwrap();
        assert!(!remove_agent(tmp.path(), "foo").unwrap());
    }

    #[test]
    fn remove_agent_missing_returns_false() {
        let tmp = TempDir::new().unwrap();
        save_manifest(
            tmp.path(),
            &json!({"version": 1, "agents": {"other": {"type": "specialist"}}}),
        )
        .unwrap();
        assert!(!remove_agent(tmp.path(), "foo").unwrap());
    }

    #[test]
    fn remove_agent_deletes_and_unlinks_context() {
        let tmp = TempDir::new().unwrap();
        add_agent(
            tmp.path(),
            "foo",
            &json!({"type": "specialist", "context": "body"}),
        )
        .unwrap();
        let ctx = tmp.path().join("agents/foo.md");
        assert!(ctx.exists());

        assert!(remove_agent(tmp.path(), "foo").unwrap());

        let cfg = load_manifest(tmp.path()).unwrap();
        assert!(cfg["agents"]["foo"].is_null());
        assert!(!ctx.exists(), "mini-context deve ser apagado");
    }

    #[test]
    fn remove_agent_respects_custom_context_file() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("custom")).unwrap();
        add_agent(
            tmp.path(),
            "foo",
            &json!({
                "type": "specialist",
                "context_file": "custom/place.md",
                "context": "body"
            }),
        )
        .unwrap();
        let ctx = tmp.path().join("custom/place.md");
        assert!(ctx.exists());

        remove_agent(tmp.path(), "foo").unwrap();
        assert!(!ctx.exists());
    }

    #[test]
    fn remove_agent_cfg_without_agents_returns_false() {
        // Divergência defensiva: .cjs panica, Rust retorna Ok(false).
        let tmp = TempDir::new().unwrap();
        save_manifest(tmp.path(), &json!({"version": 1})).unwrap();
        assert!(!remove_agent(tmp.path(), "foo").unwrap());
    }
}
