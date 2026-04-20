//! Rotas `/api/pipelines/*` — paridade com `server.cjs:2489-2580` +
//! `server.cjs:3428-3458`.
//!
//! 10 rotas:
//! - `GET  /api/pipelines`         — lista installed + templates
//! - `POST /api/pipelines/install` — instala template no `mem_dir`
//! - `POST /api/pipelines/create`  — cria pipeline (scope template|project)
//! - `GET  /api/pipelines/state`   — estado da run ativa
//! - `POST /api/pipelines/start`   — inicia run
//! - `POST /api/pipelines/advance` — avança step (aceita `reject`)
//! - `POST /api/pipelines/cancel`  — cancela run ativa
//! - `POST /api/pipelines/clear`   — remove `pipeline-state.json`
//! - `GET  /api/pipelines/runs`    — histórico
//! - `GET  /api/pipelines/:id`     — def completa (installed | template)

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use domain_pipelines::{
    advance, cancel, clear, create, install, list as pipelines_list, list_runs, load_def,
    load_state, start, InstallResult, SystemClock as PipelinesClock,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

use crate::{json_error, json_response, AppState};

pub fn pipelines_router() -> Router<AppState> {
    Router::new()
        .route("/api/pipelines", get(list_handler))
        .route("/api/pipelines/install", post(install_handler))
        .route("/api/pipelines/create", post(create_handler))
        .route("/api/pipelines/state", get(state_handler))
        .route("/api/pipelines/start", post(start_handler))
        .route("/api/pipelines/advance", post(advance_handler))
        .route("/api/pipelines/cancel", post(cancel_handler))
        .route("/api/pipelines/clear", post(clear_handler))
        .route("/api/pipelines/runs", get(runs_handler))
        .route("/api/pipelines/:id", get(get_handler))
}

fn templates_dir_of(st: &AppState) -> PathBuf {
    st.plugin_dir.join("pipelines")
}

async fn list_handler(State(st): State<AppState>) -> Response {
    let items = pipelines_list(&templates_dir_of(&st), &st.mem_dir);
    json_response(StatusCode::OK, json!({ "pipelines": items }))
}

async fn get_handler(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    // Filtro regex do .cjs: `^[a-z0-9-]+$`. Se não bate → rota não matcha
    // (404 "not found" genérico). Mantemos paridade manualmente.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return json_error(StatusCode::NOT_FOUND, "not found");
    }
    match load_def(&templates_dir_of(&st), &st.mem_dir, &id) {
        Some(def) => json_response(StatusCode::OK, def),
        None => json_error(StatusCode::NOT_FOUND, "pipeline not found"),
    }
}

async fn install_handler(State(st): State<AppState>, body: Bytes) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let id = v
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let Some(id) = id else {
        return json_error(StatusCode::BAD_REQUEST, "need id");
    };
    let overwrite = v.get("overwrite").and_then(Value::as_bool).unwrap_or(false);
    match install(&templates_dir_of(&st), &st.mem_dir, id, overwrite) {
        Ok(InstallResult::Installed) => json_response(
            StatusCode::OK,
            json!({ "ok": true, "installed": true }),
        ),
        Ok(InstallResult::Exists) => json_response(
            StatusCode::OK,
            json!({ "ok": true, "installed": false, "reason": "exists" }),
        ),
        Err(e) => json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

async fn create_handler(State(st): State<AppState>, body: Bytes) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    // `.cjs` destrutura `{ scope, overwrite, ...input }` — input = body sem
    // esses campos. Reproduzimos clonando e removendo.
    let scope = v
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let overwrite = v.get("overwrite").and_then(Value::as_bool).unwrap_or(false);
    let mut input = v.clone();
    if let Value::Object(m) = &mut input {
        m.remove("scope");
        m.remove("overwrite");
    }
    match create(&templates_dir_of(&st), &st.mem_dir, &scope, &input, overwrite) {
        Ok(r) => {
            let mut out = Map::new();
            out.insert("ok".into(), Value::Bool(true));
            out.insert("id".into(), Value::String(r.id));
            out.insert("scope".into(), Value::String(r.scope));
            out.insert(
                "path".into(),
                Value::String(r.path.to_string_lossy().into_owned()),
            );
            json_response(StatusCode::OK, Value::Object(out))
        }
        Err(e) => json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

async fn state_handler(State(st): State<AppState>) -> Response {
    match load_state(&st.mem_dir) {
        Some(state) => json_response(StatusCode::OK, json!({ "state": state })),
        None => json_response(StatusCode::OK, json!({ "state": Value::Null })),
    }
}

async fn start_handler(State(st): State<AppState>, body: Bytes) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let id = v
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let Some(id) = id else {
        return json_error(StatusCode::BAD_REQUEST, "need id");
    };
    let title = v.get("title").and_then(Value::as_str);
    let started_by = v.get("started_by").and_then(Value::as_str);
    match start(
        &templates_dir_of(&st),
        &st.mem_dir,
        id,
        title,
        started_by,
        &PipelinesClock,
    ) {
        Ok(state) => json_response(StatusCode::OK, json!({ "state": state })),
        Err(e) => json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

async fn advance_handler(State(st): State<AppState>, body: Bytes) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let step_id = v
        .get("step_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let Some(step_id) = step_id else {
        return json_error(StatusCode::BAD_REQUEST, "need step_id");
    };
    let note = v.get("note").and_then(Value::as_str);
    let decided_by = v.get("decided_by").and_then(Value::as_str);
    let decision = v.get("decision").and_then(Value::as_str);
    match advance(&st.mem_dir, step_id, note, decided_by, decision, &PipelinesClock) {
        Ok(state) => json_response(StatusCode::OK, json!({ "state": state })),
        Err(e) => json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

async fn cancel_handler(State(st): State<AppState>, body: Bytes) -> Response {
    // `.cjs` faz `try { body = JSON.parse(raw) } catch { body = {} }` — JSON
    // inválido não falha, só ignora reason.
    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Object(Map::new()));
    let reason = v.get("reason").and_then(Value::as_str);
    match cancel(&st.mem_dir, reason, &PipelinesClock) {
        Ok(Some(state)) => json_response(StatusCode::OK, json!({ "state": state })),
        Ok(None) => json_response(StatusCode::OK, json!({ "state": Value::Null })),
        Err(e) => json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

async fn clear_handler(State(st): State<AppState>) -> Response {
    match clear(&st.mem_dir) {
        Ok(()) => json_response(StatusCode::OK, json!({ "ok": true })),
        Err(e) => json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

#[derive(Debug, Deserialize)]
struct RunsQuery {
    limit: Option<String>,
}

async fn runs_handler(State(st): State<AppState>, Query(q): Query<RunsQuery>) -> Response {
    // `.cjs`: `Number.parseInt(limitRaw || '50', 10)` → usa 50 se NaN.
    let limit = q
        .limit
        .as_deref()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|n| *n > 0)
        .map(|n| n as usize)
        .unwrap_or(50);
    let runs = list_runs(&st.mem_dir, limit);
    json_response(StatusCode::OK, json!({ "runs": runs }))
}
