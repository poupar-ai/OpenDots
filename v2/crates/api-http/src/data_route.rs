//! `GET /api/data` — agregador para a UI moderna.
//!
//! Paridade com `plugin/ui/server.cjs::getData()` (linha 118). Lê
//! arquivos do `mem_dir` e devolve o shape `ServerData` que `useData`
//! (ui-next) espera.
//!
//! Esse endpoint existe pra fechar o gap durante a migração Node → Rust:
//! a UI moderna ainda pede o agregado, em vez de N endpoints granulares.

use std::path::Path;

use axum::{extract::State, http::StatusCode, response::Response, routing::get, Router};
use core_paths::jsonl::read_jsonl;
use domain_agents::{count_invocations, load_manifest, load_mini_context};
use serde_json::{json, Map, Value};

use crate::{json_response, AppState};

pub fn data_router() -> Router<AppState> {
    Router::new().route("/api/data", get(handler))
}

async fn handler(State(st): State<AppState>) -> Response {
    json_response(StatusCode::OK, build_payload(&st))
}

fn build_payload(st: &AppState) -> Value {
    let mem = &st.mem_dir;

    let sessions_raw = read_jsonl(&mem.join("sessions.jsonl")).unwrap_or_default();
    let observations = read_jsonl(&mem.join("observations.jsonl")).unwrap_or_default();
    let decisions = read_jsonl(&mem.join("decisions.jsonl")).unwrap_or_default();
    let corrections = read_jsonl(&mem.join("corrections.jsonl")).unwrap_or_default();
    let gotchas = read_jsonl(&mem.join("gotchas.jsonl")).unwrap_or_default();

    let soul_md = read_md(mem, "SOUL.md");
    let project_md = read_md(mem, "PROJECT.md");
    let decisions_md = read_md(mem, "DECISIONS.md");
    let gotchas_md = read_md(mem, "GOTCHAS.md");
    let glossary_md = read_md(mem, "GLOSSARY.md");
    let code_index_md = read_md(mem, "CODE_INDEX.md");

    let code_index = read_json(&mem.join("code_index.json"));
    let stats_raw = read_json(&mem.join("stats.json"));

    let sessions = build_sessions(&sessions_raw);
    let agents_data = build_agents(st);

    let project_name = st
        .project_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let mut stats = Map::new();
    stats.insert("sessions".into(), json!(sessions.len()));
    stats.insert("observations".into(), json!(observations.len()));
    stats.insert("decisions".into(), json!(decisions.len()));
    stats.insert("corrections".into(), json!(corrections.len()));
    stats.insert("gotchas".into(), json!(gotchas.len()));
    stats.insert("hasSoul".into(), json!(soul_md.is_some()));
    stats.insert("hasProjectMd".into(), json!(project_md.is_some()));
    stats.insert("codeIndex".into(), code_index_summary(&code_index));
    stats.insert("tokens".into(), tokens_block(&stats_raw));

    json!({
        "project": {
            "name": project_name,
            "root": st.project_root.to_string_lossy(),
        },
        "soulMd":      soul_md,
        "projectMd":   project_md,
        "decisionsMd": decisions_md,
        "gotchasMd":   gotchas_md,
        "glossaryMd":  glossary_md,
        "codeIndexMd": code_index_md,
        "agents":      agents_data,
        "sessions":    sessions,
        "observations": reverse(observations),
        "decisions":    reverse(decisions),
        "corrections":  reverse(corrections),
        "gotchas":      reverse(gotchas),
        "stats":        Value::Object(stats),
    })
}

fn read_md(mem: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(mem.join(name)).ok()
}

fn read_json(path: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn reverse(mut v: Vec<Value>) -> Vec<Value> {
    v.reverse();
    v
}

/// Paridade com o trecho `sessionMap`/sort em `getData()`:
/// merge por `session_id`, filtra `ended_at` presente, ordena
/// desc por `ended_at`.
fn build_sessions(raw: &[Value]) -> Vec<Value> {
    let mut by_id: std::collections::HashMap<String, Map<String, Value>> =
        std::collections::HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for s in raw {
        let Some(obj) = s.as_object() else { continue };
        let Some(id) = obj.get("session_id").and_then(|v| v.as_str()) else {
            continue;
        };
        let entry = by_id.entry(id.to_string()).or_insert_with(|| {
            order.push(id.to_string());
            Map::new()
        });
        for (k, v) in obj {
            entry.insert(k.clone(), v.clone());
        }
    }
    let mut out: Vec<Value> = order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .filter(|s| s.get("ended_at").and_then(|v| v.as_str()).is_some())
        .map(Value::Object)
        .collect();
    out.sort_by(|a, b| {
        let aa = a.get("ended_at").and_then(|v| v.as_str()).unwrap_or("");
        let bb = b.get("ended_at").and_then(|v| v.as_str()).unwrap_or("");
        bb.cmp(aa)
    });
    out
}

fn build_agents(st: &AppState) -> Value {
    let Some(manifest) = load_manifest(&st.mem_dir) else {
        return Value::Null;
    };
    let mut contexts = Map::new();
    if let Some(agents) = manifest.get("agents").and_then(|v| v.as_object()) {
        for (name, agent) in agents {
            let ctx_file = agent
                .get("context_file")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ctx = load_mini_context(&st.mem_dir, ctx_file).unwrap_or_default();
            contexts.insert(name.clone(), Value::String(ctx));
        }
    }
    let inv = count_invocations(&st.mem_dir);
    let counts = inv.get("counts").cloned().unwrap_or_else(|| json!({}));
    let recent = inv.get("recent").cloned().unwrap_or_else(|| json!({}));

    json!({
        "manifest": manifest,
        "contexts": Value::Object(contexts),
        "counts":   counts,
        "recent":   recent,
        // `logs` é opcional no shape (AgentLogSummary). Mantém objeto vazio
        // — porte do `agentLogSummary()` legacy pode entrar em outro PR.
        "logs":     json!({}),
    })
}

fn code_index_summary(code_index: &Option<Value>) -> Value {
    let Some(ci) = code_index else {
        return Value::Null;
    };
    let files = ci.get("files").and_then(|v| v.as_object());
    let files_count = files.map(|f| f.len()).unwrap_or(0);
    let symbols_count = files
        .map(|f| {
            f.values()
                .filter_map(|v| v.get("symbols").and_then(|s| s.as_array()))
                .map(|s| s.len())
                .sum::<usize>()
        })
        .unwrap_or(0);
    let generated_at = ci.get("generated_at").cloned().unwrap_or(Value::Null);
    json!({
        "files":        files_count,
        "symbols":      symbols_count,
        "generated_at": generated_at,
    })
}

fn tokens_block(stats: &Option<Value>) -> Value {
    let Some(s) = stats else {
        return Value::Null;
    };
    json!({
        "last":    s.get("last_injection_tokens").cloned().unwrap_or(json!(0)),
        "budget":  s.get("last_injection_budget").cloned().unwrap_or(json!(0)),
        "blocks":  s.get("last_injection_blocks").cloned().unwrap_or(json!([])),
        "skipped": s.get("last_injection_skipped").cloned().unwrap_or(json!([])),
        "at":      s.get("last_injection_at").cloned().unwrap_or(Value::Null),
    })
}
