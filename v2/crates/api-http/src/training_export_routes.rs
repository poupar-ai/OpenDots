//! `GET /api/training-export` — exporta chat history de um agente como
//! dataset SFT pra fine-tune de modelos menores.
//!
//! Camada thin: parse de querystring, delega pra `domain_training_export`,
//! devolve JSONL stream (uma sample por linha).
//!
//! Query params:
//! - `agent` (obrigatório) — qual agente exportar
//! - `format` (opcional, default `anthropic-sft`) — formato do output
//! - `from_ms` / `to_ms` (opcional) — janela temporal
//! - `max_gap_ms` (opcional, default 600000) — gap entre sessions
//! - `min_turns` (opcional, default 2) — descarta sessions pequenas
//! - `system_prompt` (opcional) — adiciona role=system no início de cada sample
//! - `clean_only` (opcional, default true) — descarta turns blocked/error

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Response,
    routing::get,
    Router,
};
use domain_training_export::{export, ExportOptions, Format};
use serde::Deserialize;

use crate::{json_error, json_response, AppState};

pub fn training_export_router() -> Router<AppState> {
    Router::new().route("/api/training-export", get(handler))
}

#[derive(Deserialize)]
struct Q {
    agent: String,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    from_ms: Option<u64>,
    #[serde(default)]
    to_ms: Option<u64>,
    #[serde(default)]
    max_gap_ms: Option<u64>,
    #[serde(default)]
    min_turns: Option<usize>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default)]
    clean_only: Option<bool>,
}

async fn handler(State(st): State<AppState>, Query(q): Query<Q>) -> Response {
    let format = match q.format.as_deref() {
        None => Format::AnthropicSft,
        Some(s) => match Format::from_str(s) {
            Some(f) => f,
            None => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    &format!("format inválido: {s} (use anthropic-sft|openai-sft|messages)"),
                );
            }
        },
    };
    let opts = ExportOptions {
        agent: q.agent,
        format,
        from_ms: q.from_ms,
        to_ms: q.to_ms,
        max_gap_ms: q.max_gap_ms.unwrap_or(600_000),
        min_turns_per_sample: q.min_turns.unwrap_or(2),
        system_prompt: q.system_prompt.filter(|s| !s.is_empty()),
        clean_only: q.clean_only.unwrap_or(true),
    };
    match export(&st.mem_dir, &opts) {
        Ok(samples) => json_response(StatusCode::OK, serde_json::Value::Array(samples)),
        Err(domain_training_export::ExportError::NoChatHistory(a)) => {
            json_error(StatusCode::NOT_FOUND, &format!("sem histórico pro agente: {a}"))
        }
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}
