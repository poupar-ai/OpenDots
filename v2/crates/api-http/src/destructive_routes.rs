//! Rotas `/api/destructive/*` — paridade com `server.cjs:3071-3120`.
//!
//! 4 rotas:
//! - `GET  /api/destructive/patterns` — lista core + user
//! - `PUT  /api/destructive/patterns` — substitui patterns do usuário
//! - `POST /api/destructive/test` — roda `detect(text)`
//! - `POST /api/destructive/core/toggle` — liga/desliga pattern core

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use domain_destructive::{
    detect, list_patterns, save_user_patterns, set_core_disabled, RandomIdGen, SystemIsoClock,
};
use serde_json::{json, Value};

use crate::{json_error, json_response, AppState};

pub fn destructive_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/destructive/patterns",
            get(patterns_get).put(patterns_put),
        )
        .route("/api/destructive/test", post(test))
        .route("/api/destructive/core/toggle", post(core_toggle))
}

async fn patterns_get(State(st): State<AppState>) -> Response {
    let v = list_patterns(&st.mem_dir);
    json_response(StatusCode::OK, v)
}

async fn patterns_put(State(st): State<AppState>, body: Bytes) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let Some(arr) = v.get("patterns").and_then(Value::as_array) else {
        return json_error(StatusCode::BAD_REQUEST, "patterns must be array");
    };
    let saved = save_user_patterns(&st.mem_dir, arr, &RandomIdGen, &SystemIsoClock);
    json_response(StatusCode::OK, json!({ "ok": true, "patterns": saved }))
}

async fn test(State(st): State<AppState>, body: Bytes) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let text = v.get("text").and_then(Value::as_str).unwrap_or_default();
    let r = detect(&st.mem_dir, text);
    json_response(StatusCode::OK, r.to_json())
}

async fn core_toggle(State(st): State<AppState>, body: Bytes) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let id = match v.get("id").and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s,
        _ => return json_error(StatusCode::BAD_REQUEST, "need {id, disabled}"),
    };
    let disabled = v.get("disabled").and_then(Value::as_bool).unwrap_or(false);
    match set_core_disabled(&st.mem_dir, id, disabled) {
        Ok(list) => json_response(StatusCode::OK, json!({ "ok": true, "disabled_core": list })),
        Err(e) => json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}
