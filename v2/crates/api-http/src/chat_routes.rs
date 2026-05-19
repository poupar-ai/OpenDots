//! Rotas `/api/chat/*` — paridade com `server.cjs:1902-1987`.
//!
//! Endpoints:
//! - `GET /api/chat/history?agent=X` — retorna mensagens do JSONL do agente
//! - `GET /api/chat/stream?agent=X&since_idx=N` — SSE com snapshot + delta
//!
//! Também exporta helpers de escrita (`append_chat`, `ensure_agent_channels`)
//! usados por `handoff_routes` e `agents_routes`.

use std::convert::Infallible;
use std::fs;
use std::path::Path;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use domain_graph::SystemClock as GraphSystemClock;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::{json_error, json_response, AppState};

pub fn chat_router() -> Router<AppState> {
    Router::new()
        .route("/api/chat/history", get(history_handler))
        .route("/api/chat/stream", get(stream_handler))
}

pub(crate) fn chat_log_path(mem_dir: &Path, agent: &str) -> std::path::PathBuf {
    mem_dir.join("agent_chats").join(format!("{agent}.jsonl"))
}

pub(crate) fn chat_dir(mem_dir: &Path) -> std::path::PathBuf {
    mem_dir.join("agent_chats")
}

/// Lê `agent_chats/<agent>.jsonl` (+ partes rotacionadas), filtra
/// linhas vazias e JSON inválido. Concat preserva ordem cronológica.
/// Migrado pra core-storage::JsonlRepository — linhas malformadas
/// agora logam warning em vez de descartar silencioso.
pub(crate) fn read_chat(mem_dir: &Path, agent: &str) -> Vec<Value> {
    let p = chat_log_path(mem_dir, agent);
    match core_storage::JsonlRepository::<Value>::open_default(p) {
        Ok(r) => r.read_all(),
        Err(_) => Vec::new(),
    }
}

/// Append entry no JSONL via core_storage (rotação automática a 50k
/// linhas / 10MB) + dispara `graph::on_agent_chat_row` + indexa no
/// search se `search` foi passado. Throughput alvo: thousands of
/// lines/min — handler hot path.
///
/// `search` opcional porque alguns callers (paridade .cjs, tests)
/// não têm AppState. Quando passado, content é indexado em FTS5 com
/// source_type="chat" e ref_id=agent.
pub(crate) fn append_chat(
    mem_dir: &Path,
    agent: &str,
    entry: &Value,
    search: Option<&std::sync::Arc<core_search::SearchIndex>>,
) {
    let path = chat_log_path(mem_dir, agent);
    if let Ok(r) = core_storage::JsonlRepository::<Value>::open_default(path) {
        let _ = r.append(entry);
    }
    domain_graph::on_agent_chat_row(mem_dir, agent, entry, &GraphSystemClock);
    if let Some(idx) = search {
        let content = entry.get("content").and_then(Value::as_str).unwrap_or("");
        if !content.is_empty() {
            let ts = entry
                .get("ts")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let _ = idx.add(&core_search::SearchDoc {
                content: content.to_string(),
                source_type: "chat".to_string(),
                ref_id: agent.to_string(),
                ts,
            });
        }
    }
}

/// Garante que todo agente do manifest tenha arquivo de canal.
/// Paridade com `ensureAgentChannels` em `server.cjs:378-385`.
pub(crate) fn ensure_agent_channels(mem_dir: &Path, manifest: &Value) {
    let Some(agents) = manifest.get("agents").and_then(Value::as_object) else {
        return;
    };
    let dir = chat_dir(mem_dir);
    let _ = fs::create_dir_all(&dir);
    for name in agents.keys() {
        let p = chat_log_path(mem_dir, name);
        if !p.exists() {
            let _ = fs::write(&p, "");
        }
    }
}


#[derive(Debug, Deserialize)]
struct HistoryQuery {
    agent: Option<String>,
}

async fn history_handler(State(st): State<AppState>, Query(q): Query<HistoryQuery>) -> Response {
    let Some(agent) = q.agent.as_deref().filter(|s| !s.is_empty()) else {
        return json_error(StatusCode::BAD_REQUEST, "missing agent");
    };
    let messages = read_chat(&st.mem_dir, agent);
    json_response(
        StatusCode::OK,
        json!({ "agent": agent, "messages": messages }),
    )
}

// ── SSE: /api/chat/stream ───────────────────────────────────────────────────
// Paridade com `handleChatStream` em `server.cjs:1916-1987`.
// - Snapshot inicial das linhas a partir de `since_idx`
// - Evento `ready` depois do snapshot
// - Polling 500ms pro delta (nova linha → evento `msg`)
// - Heartbeat `ping` cada 25s (via Axum KeepAlive)

#[derive(Debug, Deserialize)]
struct StreamQuery {
    agent: Option<String>,
    since_idx: Option<String>,
}

async fn stream_handler(
    State(st): State<AppState>,
    Query(q): Query<StreamQuery>,
) -> Response {
    let Some(agent) = q.agent.as_deref().filter(|s| !s.is_empty()) else {
        return json_error(StatusCode::BAD_REQUEST, "missing agent");
    };
    let since_idx: usize = q
        .since_idx
        .as_deref()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|n| *n >= 0)
        .map(|n| n as usize)
        .unwrap_or(0);

    let agent = agent.to_string();
    let mem_dir = st.mem_dir.clone();
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);

    tokio::spawn(async move {
        let file_path = chat_log_path(&mem_dir, &agent);
        let mut sent_lines: usize = 0;
        let mut last_size: u64 = 0;

        // Snapshot inicial
        if let Ok(content) = fs::read_to_string(&file_path) {
            let lines: Vec<&str> = content.split('\n').filter(|l| !l.is_empty()).collect();
            for (i, line) in lines.iter().enumerate() {
                if i < since_idx {
                    continue;
                }
                if let Ok(msg) = serde_json::from_str::<Value>(line) {
                    let data =
                        serde_json::to_string(&json!({ "idx": i, "msg": msg })).unwrap_or_default();
                    let ev = Event::default().event("msg").data(data);
                    if tx.send(Ok(ev)).await.is_err() {
                        return;
                    }
                }
            }
            sent_lines = lines.len();
            last_size = fs::metadata(&file_path).map(|m| m.len()).unwrap_or(content.len() as u64);
        }

        let ready = serde_json::to_string(&json!({ "agent": agent, "idx": sent_lines }))
            .unwrap_or_default();
        if tx
            .send(Ok(Event::default().event("ready").data(ready)))
            .await
            .is_err()
        {
            return;
        }

        // Polling loop 500ms
        let mut ticker = tokio::time::interval(Duration::from_millis(500));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if tx.is_closed() {
                return;
            }
            let Ok(meta) = fs::metadata(&file_path) else {
                continue;
            };
            let size = meta.len();
            if size <= last_size {
                continue;
            }
            let Ok(content) = fs::read_to_string(&file_path) else {
                continue;
            };
            let lines: Vec<&str> = content.split('\n').filter(|l| !l.is_empty()).collect();
            for i in sent_lines..lines.len() {
                if let Ok(msg) = serde_json::from_str::<Value>(lines[i]) {
                    let data =
                        serde_json::to_string(&json!({ "idx": i, "msg": msg })).unwrap_or_default();
                    let ev = Event::default().event("msg").data(data);
                    if tx.send(Ok(ev)).await.is_err() {
                        return;
                    }
                }
            }
            sent_lines = lines.len();
            last_size = size;
        }
    });

    let stream: ReceiverStream<Result<Event, Infallible>> = ReceiverStream::new(rx);
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(25)).event(
            Event::default().event("ping").data("{}"),
        ))
        .into_response()
}
