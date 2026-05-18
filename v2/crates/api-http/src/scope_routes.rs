//! Rotas `/api/scope` — leitura/edição do `project_root` (escopo do
//! agente) sem mexer em env. Persiste em `<mem_dir>/scope.json` via
//! [`core_paths::write_scope_project_root`].
//!
//! - `GET  /api/scope` → `{ project_root, mem_dir, default_project_root, source }`
//! - `PUT  /api/scope` → body `{ project_root: "<abs path>" }`, valida
//!   que existe e é dir, grava em `scope.json`, indica restart.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use core_paths::{read_scope_project_root, write_scope_project_root};
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::{json_error, json_response, AppState};

pub fn scope_router() -> Router<AppState> {
    Router::new().route("/api/scope", get(get_scope).put(put_scope))
}

async fn get_scope(State(st): State<AppState>) -> Response {
    let from_scope = read_scope_project_root(&st.mem_dir);
    let source = if from_scope.is_some() { "scope.json" } else { "default" };
    json_response(
        StatusCode::OK,
        json!({
            "project_root": st.project_root.to_string_lossy(),
            "mem_dir":      st.mem_dir.to_string_lossy(),
            "source":       source,
        }),
    )
}

async fn put_scope(State(st): State<AppState>, body: Bytes) -> Response {
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let pr = parsed
        .get("project_root")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(pr) = pr else {
        return json_error(StatusCode::BAD_REQUEST, "project_root é obrigatório");
    };
    let path = PathBuf::from(pr);
    if !path.is_absolute() {
        return json_error(StatusCode::BAD_REQUEST, "project_root precisa ser caminho absoluto");
    }
    if !path.exists() {
        return json_error(StatusCode::BAD_REQUEST, "pasta não existe");
    }
    if !path.is_dir() {
        return json_error(StatusCode::BAD_REQUEST, "caminho não é uma pasta");
    }
    if let Err(e) = write_scope_project_root(&st.mem_dir, &path) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("falha ao gravar: {e}"));
    }
    json_response(
        StatusCode::OK,
        json!({
            "ok": true,
            "project_root": path.to_string_lossy(),
            "restart_required": true,
        }),
    )
}
