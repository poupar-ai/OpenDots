//! Rotas `/api/best-practices/*` — paridade com `server.cjs:2411-2485` +
//! `server.cjs:3408-3425`.
//!
//! 6 rotas:
//! - `GET  /api/best-practices`              — lista catálogo + locais
//! - `POST /api/best-practices/apply`        — instala do catálogo pro mem_dir
//! - `POST /api/best-practices/create`       — cria local
//! - `GET  /api/best-practices/:id`          — conteúdo (local OU catálogo)
//! - `PUT  /api/best-practices/:id`          — update local
//! - `DELETE /api/best-practices/:id`        — remove local

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use domain_best_practices::{
    apply_practices, create_local_practice, delete_local_practice, list_local_practices,
    list_practices, load_practice_content, update_local_practice, CreateLocalInput,
    LocalPracticeError, UpdateLocalInput,
};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

use crate::{json_error, json_response, AppState};

pub fn best_practices_router() -> Router<AppState> {
    Router::new()
        .route("/api/best-practices", get(list))
        .route("/api/best-practices/apply", post(apply_handler))
        .route("/api/best-practices/create", post(create))
        .route(
            "/api/best-practices/:id",
            get(get_by_id).put(update).delete(delete_handler),
        )
}

fn bp_dir_of(st: &AppState) -> PathBuf {
    st.plugin_dir.join("best-practices")
}

async fn list(State(st): State<AppState>) -> Response {
    let bp_dir = bp_dir_of(&st);
    let catalog = list_practices(&bp_dir);
    let local = list_local_practices(&st.mem_dir);
    let installed: Vec<String> = local
        .iter()
        .filter_map(|p| p.get("id").and_then(Value::as_str).map(str::to_string))
        .collect();
    let installed_set: std::collections::HashSet<&str> =
        installed.iter().map(String::as_str).collect();
    let catalog_ids: std::collections::HashSet<String> = catalog
        .iter()
        .filter_map(|p| p.get("id").and_then(Value::as_str).map(str::to_string))
        .collect();

    let mut merged: Vec<Value> = Vec::new();
    for p in &catalog {
        let mut m = p.clone();
        let id = m.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        m.insert("installed".into(), Value::Bool(installed_set.contains(id.as_str())));
        m.insert("source".into(), Value::String("catalog".into()));
        merged.push(Value::Object(m));
    }
    for p in &local {
        let id = p.get("id").and_then(Value::as_str).unwrap_or("");
        if catalog_ids.contains(id) {
            continue;
        }
        let mut m = p.clone();
        m.insert("installed".into(), Value::Bool(true));
        merged.push(Value::Object(m));
    }

    json_response(
        StatusCode::OK,
        json!({
            "available": merged,
            "installed": installed,
        }),
    )
}

async fn apply_handler(State(st): State<AppState>, body: Bytes) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let ids: Vec<String> = match v.get("ids").and_then(Value::as_array) {
        Some(arr) if !arr.is_empty() => arr
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        _ => return json_error(StatusCode::BAD_REQUEST, "ids must be non-empty array"),
    };
    if ids.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "ids must be non-empty array");
    }
    let overwrite = v.get("overwrite").and_then(Value::as_bool).unwrap_or(false);
    let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    let bp_dir = bp_dir_of(&st);
    match apply_practices(&bp_dir, &st.mem_dir, &id_refs, overwrite) {
        Ok(r) => json_response(
            StatusCode::OK,
            json!({
                "ok": true,
                "copied": r.copied,
                "preserved": r.preserved,
                "missing": r.missing,
            }),
        ),
        Err(e) => json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

async fn create(State(st): State<AppState>, body: Bytes) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let id = v.get("id").and_then(Value::as_str).unwrap_or_default();
    let name = v.get("name").and_then(Value::as_str).unwrap_or_default();
    let scope = v.get("scope").and_then(Value::as_str);
    let when_to_use = v.get("when_to_use").and_then(Value::as_str);
    let not_for = v.get("not_for").and_then(Value::as_str);
    let content = v.get("content").and_then(Value::as_str).unwrap_or_default();
    let tags_owned = v.get("tags").and_then(Value::as_array).cloned();
    let overwrite = v.get("overwrite").and_then(Value::as_bool).unwrap_or(false);

    let input = CreateLocalInput {
        id,
        name,
        scope,
        tags: tags_owned.as_deref(),
        when_to_use,
        not_for,
        content,
    };
    match create_local_practice(&st.mem_dir, &input, overwrite) {
        Ok(r) => {
            let mut out = Map::new();
            out.insert("ok".into(), Value::Bool(true));
            out.insert("id".into(), Value::String(r.id));
            out.insert(
                "path".into(),
                Value::String(r.path.to_string_lossy().into_owned()),
            );
            out.insert("meta".into(), Value::Object(r.meta));
            json_response(StatusCode::OK, Value::Object(out))
        }
        Err(e) => json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

async fn get_by_id(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    // Filtro regex do .cjs: `^[a-z0-9-]+$`. Se não bate → 404 (rota não matcha).
    if !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') || id.is_empty() {
        return json_error(StatusCode::NOT_FOUND, "not found");
    }
    let bp_dir = bp_dir_of(&st);
    let installed_path = st.mem_dir.join("best-practices").join(&id).join("practice.md");
    let installed = installed_path.exists();
    let content = if installed {
        std::fs::read_to_string(&installed_path).ok()
    } else {
        load_practice_content(&bp_dir, &id)
    };
    match content {
        Some(c) if !c.is_empty() => json_response(
            StatusCode::OK,
            json!({ "id": id, "content": c, "installed": installed }),
        ),
        _ => json_error(StatusCode::NOT_FOUND, "practice not found"),
    }
}

async fn update(State(st): State<AppState>, Path(id): Path<String>, body: Bytes) -> Response {
    if !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') || id.is_empty() {
        return json_error(StatusCode::NOT_FOUND, "not found");
    }
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let tags_owned = v.get("tags").and_then(Value::as_array).cloned();
    let input = UpdateLocalInput {
        name: v.get("name").and_then(Value::as_str),
        scope: v.get("scope").and_then(Value::as_str),
        tags: tags_owned.as_deref(),
        when_to_use: v.get("when_to_use").and_then(Value::as_str),
        not_for: v.get("not_for").and_then(Value::as_str),
        content: v.get("content").and_then(Value::as_str),
    };
    match update_local_practice(&st.mem_dir, &id, &input) {
        Ok(r) => {
            let mut out = Map::new();
            out.insert("ok".into(), Value::Bool(true));
            out.insert("id".into(), Value::String(r.id));
            out.insert("meta".into(), Value::Object(r.meta));
            json_response(StatusCode::OK, Value::Object(out))
        }
        Err(e) => json_error(StatusCode::BAD_REQUEST, &map_local_err(&e, true)),
    }
}

async fn delete_handler(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    if !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') || id.is_empty() {
        return json_error(StatusCode::NOT_FOUND, "not found");
    }
    match delete_local_practice(&st.mem_dir, &id) {
        Ok(r) => json_response(
            StatusCode::OK,
            json!({ "ok": true, "id": r.id, "removed": r.removed }),
        ),
        Err(e) => json_error(StatusCode::BAD_REQUEST, &map_local_err(&e, true)),
    }
}

/// `.cjs` usa mensagem CURTA "id inválido" em update/delete, mas LONGA em
/// create. O tipo Rust único sempre devolve a longa — remapeamos aqui.
fn map_local_err(e: &LocalPracticeError, short_invalid_id: bool) -> String {
    if short_invalid_id && matches!(e, LocalPracticeError::InvalidId) {
        return "id inválido".to_string();
    }
    e.to_string()
}

