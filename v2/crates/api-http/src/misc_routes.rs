//! Rotas `/api/architect/*`, `/api/skills`, `/api/ides/*` — paridade com
//! `server.cjs:2584-2632` + `server.cjs:3460-3479`.
//!
//! 5 rotas:
//! - `POST /api/architect/ensure`    — instala meta-agente architect
//! - `POST /api/architect/generate`  — gera seed a partir do spec
//! - `GET  /api/skills`              — lista skills de um pack
//! - `GET  /api/ides`                — lista IDEs suportadas
//! - `POST /api/ides/apply`          — aplica config pra IDE escolhida

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use domain_architect::{ensure_installed, generate_seed, SystemClock as ArchitectClock};
use domain_ide_adapters::{apply as ide_apply, SUPPORTED as IDE_SUPPORTED};
use domain_skills_install::list_skills_in_pack;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

use crate::{json_error, json_response, AppState};

pub fn misc_router() -> Router<AppState> {
    Router::new()
        .route("/api/architect/ensure", post(architect_ensure))
        .route("/api/architect/generate", post(architect_generate))
        .route("/api/skills", get(skills_list))
        .route("/api/ides", get(ides_list))
        .route("/api/ides/apply", post(ides_apply_handler))
}

fn seeds_dir_of(st: &AppState) -> PathBuf {
    st.plugin_dir.join("seeds")
}

// ── architect ───────────────────────────────────────────────────────────────

async fn architect_ensure(State(st): State<AppState>) -> Response {
    match ensure_installed(&st.mem_dir) {
        Ok(_) => json_response(StatusCode::OK, json!({ "ok": true })),
        Err(e) => json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

async fn architect_generate(State(st): State<AppState>, body: Bytes) -> Response {
    let spec: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let seeds_dir = seeds_dir_of(&st);
    match generate_seed(&seeds_dir, &spec, &ArchitectClock) {
        Ok(r) => {
            let mut out = Map::new();
            out.insert("ok".into(), Value::Bool(r.ok));
            out.insert("ref".into(), Value::String(r.reference));
            out.insert("dir".into(), Value::String(r.dir.to_string_lossy().into_owned()));
            out.insert("agent_count".into(), Value::from(r.agent_count));
            json_response(StatusCode::OK, Value::Object(out))
        }
        Err(e) => json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

// ── skills ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct SkillsQuery {
    category: Option<String>,
    pack: Option<String>,
}

async fn skills_list(State(st): State<AppState>, Query(q): Query<SkillsQuery>) -> Response {
    let category = q.category.as_deref().filter(|s| !s.is_empty());
    let pack = q.pack.as_deref().filter(|s| !s.is_empty());
    let (Some(category), Some(pack)) = (category, pack) else {
        return json_error(StatusCode::BAD_REQUEST, "need category and pack");
    };
    let seeds_dir = seeds_dir_of(&st);
    let skills = list_skills_in_pack(&seeds_dir, category, pack);
    let arr: Vec<Value> = skills
        .into_iter()
        .map(|s| {
            json!({
                "name": s.name,
                "description": s.description,
                "scope": s.scope,
                "version": s.version,
                "requires_mcp": s.requires_mcp,
                "env": s.env,
                "has_mcp": s.has_mcp,
            })
        })
        .collect();
    json_response(StatusCode::OK, json!({ "skills": arr }))
}

// ── ides ────────────────────────────────────────────────────────────────────

async fn ides_list() -> Response {
    let supported: Vec<Value> = IDE_SUPPORTED
        .iter()
        .map(|s| Value::String((*s).to_string()))
        .collect();
    json_response(StatusCode::OK, json!({ "supported": supported }))
}

async fn ides_apply_handler(State(st): State<AppState>, body: Bytes) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let ide = v.get("ide").and_then(Value::as_str).filter(|s| !s.is_empty());
    let Some(ide) = ide else {
        return json_error(StatusCode::BAD_REQUEST, "need ide");
    };
    match ide_apply(ide, &st.mem_dir, &st.project_root) {
        Ok(r) => {
            let mut out = Map::new();
            out.insert("ok".into(), Value::Bool(true));
            out.insert(
                "written".into(),
                Value::Array(r.written.into_iter().map(Value::String).collect()),
            );
            json_response(StatusCode::OK, Value::Object(out))
        }
        Err(e) => json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}
