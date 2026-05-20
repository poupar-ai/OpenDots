//! Rotas `/api/memory/search` — busca full-text via SQLite FTS5.
//!
//! - `POST /api/memory/search`  body `{q, limit?, source?}` →
//!   `{results: [{score, snippet, source_type, ref_id, ts, content}]}`.
//! - `GET  /api/memory/search/stats` → `{count, indexed_sources: [...]}`.
//! - `POST /api/memory/search/rebuild` → re-indexa do zero a partir
//!   dos JSONLs em disco (chats + observations). Útil após restore
//!   de snapshot ou se índice ficou inconsistente.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use serde_json::{json, Value};

use crate::{json_error, json_response, AppState};

pub fn search_router() -> Router<AppState> {
    Router::new()
        .route("/api/memory/search", post(search_handler))
        .route("/api/memory/search/stats", get(stats_handler))
        .route("/api/memory/search/rebuild", post(rebuild_handler))
        .route("/api/memory/search/health", get(health_handler))
}

/// Reporta drift entre indexed (search.db) e source (JSONLs). Útil
/// pra detectar inconsistência (hooks falharam, restore sem rebuild,
/// etc.). Quando `in_sync: false`, caller pode chamar `/rebuild`.
async fn health_handler(State(st): State<AppState>) -> Response {
    let Some(idx) = st.search.as_ref() else {
        return json_response(
            StatusCode::OK,
            json!({
                "indexed_count": 0,
                "source_count": null,
                "in_sync": false,
                "drift": null,
                "search_available": false,
            }),
        );
    };
    let indexed = idx.count().unwrap_or(0);
    let source = count_jsonl_sources(&st.mem_dir);
    let drift = (source as i64) - (indexed as i64);
    json_response(
        StatusCode::OK,
        json!({
            "indexed_count": indexed,
            "source_count": source,
            "in_sync": drift == 0,
            "drift": drift,
            "search_available": true,
            "hint": if drift != 0 { Some("POST /api/memory/search/rebuild") } else { None },
        }),
    )
}

/// Conta total de itens indexáveis nos JSONLs (chats + observations).
/// Equivalente ao que `rebuild_search_from_jsonls` indexaria — usado
/// pra detectar drift no health check e no auto-rebuild do startup.
pub(crate) fn count_jsonl_sources(mem_dir: &std::path::Path) -> usize {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(mem_dir.join("agent_chats")) {
        for e in entries.flatten() {
            let path = e.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.ends_with(".jsonl") {
                continue;
            }
            if let Ok(r) = core_storage::JsonlRepository::<Value>::open_default(path) {
                for entry in r.read_all() {
                    if entry
                        .get("content")
                        .and_then(Value::as_str)
                        .map(|s| !s.is_empty())
                        .unwrap_or(false)
                    {
                        total += 1;
                    }
                }
            }
        }
    }
    let obs = mem_dir.join("observations.jsonl");
    if obs.exists() {
        if let Ok(r) = core_storage::JsonlRepository::<Value>::open_default(obs) {
            for entry in r.read_all() {
                if !compose_observation_content(&entry).is_empty() {
                    total += 1;
                }
            }
        }
    }
    total
}

async fn search_handler(State(st): State<AppState>, body: Bytes) -> Response {
    let Some(idx) = st.search.as_ref() else {
        return json_error(StatusCode::SERVICE_UNAVAILABLE, "search index indisponível (sqlite falhou ao abrir?)");
    };
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let q = v
        .get("q")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(q) = q else {
        return json_error(StatusCode::BAD_REQUEST, "q (query string) obrigatória");
    };
    let limit = v
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .min(200) as usize;
    let source = v.get("source").and_then(Value::as_str);

    match idx.search(q, limit, source) {
        Ok(results) => json_response(
            StatusCode::OK,
            json!({
                "query": q,
                "limit": limit,
                "source_filter": source,
                "count": results.len(),
                "results": results,
            }),
        ),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("search falhou: {e}")),
    }
}

async fn stats_handler(State(st): State<AppState>) -> Response {
    let Some(idx) = st.search.as_ref() else {
        return json_error(StatusCode::SERVICE_UNAVAILABLE, "search index indisponível");
    };
    match idx.count() {
        Ok(n) => json_response(StatusCode::OK, json!({ "count": n, "indexed_sources": ["chat", "observation"] })),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("stats falhou: {e}")),
    }
}

/// Reindexa do zero — varre todos os agent_chats/*.jsonl e
/// observations.jsonl. Após restore de snapshot, índice fica
/// desincronizado; chamar isto resolve.
async fn rebuild_handler(State(st): State<AppState>) -> Response {
    let Some(idx) = st.search.as_ref() else {
        return json_error(StatusCode::SERVICE_UNAVAILABLE, "search index indisponível");
    };
    match rebuild_search_from_jsonls(&st.mem_dir, idx) {
        Ok(n) => json_response(
            StatusCode::OK,
            json!({ "ok": true, "indexed": n, "sources": ["chat", "observation"] }),
        ),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("rebuild falhou: {e}")),
    }
}

/// Helper público pra usar em outros módulos (db_routes.rs faz isso
/// após restore). Limpa o índice + repopula a partir dos JSONLs no
/// disco. JSONLs são source-of-truth — search é cache derivado.
pub fn rebuild_search_from_jsonls(
    mem_dir: &std::path::Path,
    idx: &core_storage::SearchIndex,
) -> Result<usize, String> {
    idx.clear().map_err(|e| format!("clear: {e}"))?;

    let mut docs: Vec<core_storage::SearchDoc> = Vec::new();

    // Chats — 1 doc por linha de cada agent_chats/<agent>.jsonl.
    if let Ok(entries) = std::fs::read_dir(mem_dir.join("agent_chats")) {
        for e in entries.flatten() {
            let path = e.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) if n.ends_with(".jsonl") => n,
                _ => continue,
            };
            // Cuida de arquivos rotacionados (oracle.jsonl + oracle.<ts>.jsonl):
            // agent name é tudo até o primeiro `.`.
            let agent = name.split('.').next().unwrap_or("").to_string();
            if agent.is_empty() {
                continue;
            }
            let r = match core_storage::JsonlRepository::<Value>::open_default(path) {
                Ok(r) => r,
                Err(_) => continue,
            };
            for entry in r.read_all() {
                let content = entry.get("content").and_then(Value::as_str).unwrap_or("");
                if content.is_empty() {
                    continue;
                }
                docs.push(core_storage::SearchDoc {
                    content: content.to_string(),
                    source_type: "chat".into(),
                    ref_id: agent.clone(),
                    ts: entry.get("ts").and_then(Value::as_str).unwrap_or("").to_string(),
                });
            }
        }
    }

    // Observations.
    let obs_path = mem_dir.join("observations.jsonl");
    if obs_path.exists() {
        if let Ok(r) = core_storage::JsonlRepository::<Value>::open_default(obs_path) {
            for entry in r.read_all() {
                // Observation content costuma ser combinação de tool_name +
                // tool_input/tool_response. Concatenamos pro full-text.
                let composed = compose_observation_content(&entry);
                if composed.is_empty() {
                    continue;
                }
                docs.push(core_storage::SearchDoc {
                    content: composed,
                    source_type: "observation".into(),
                    ref_id: entry
                        .get("session_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    ts: entry.get("ts").and_then(Value::as_str).unwrap_or("").to_string(),
                });
            }
        }
    }

    let n = docs.len();
    idx.add_batch(&docs).map_err(|e| format!("add_batch: {e}"))?;
    Ok(n)
}

fn compose_observation_content(entry: &Value) -> String {
    let tool = entry.get("tool_name").and_then(Value::as_str).unwrap_or("");
    let input = entry
        .get("tool_input")
        .map(|v| v.to_string())
        .unwrap_or_default();
    let response = entry
        .get("tool_response")
        .map(|v| v.to_string())
        .unwrap_or_default();
    let note = entry.get("note").and_then(Value::as_str).unwrap_or("");
    let parts: Vec<String> = [tool, &input, &response, note]
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    parts.join(" \n ")
}
