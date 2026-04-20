//! Rotas `/api/humans/*` — paridade com `server.cjs:3028-3053`.
//!
//! 8 rotas (6 CRUD + heartbeat em F4-05d-1, + 2 read-only chat em F4-05d-2):
//! - `GET    /api/humans`                — lista + default_id
//! - `POST   /api/humans`                — cria humano novo (201)
//! - `GET    /api/humans/:id`            — summary
//! - `PUT    /api/humans/:id`            — update parcial
//! - `DELETE /api/humans/:id`            — remove
//! - `POST   /api/humans/:id/heartbeat`  — marca last_seen_at = now
//! - `GET    /api/humans/:id/chats`      — resumo de chats (agents + humans)
//! - `GET    /api/human-chat/history`    — histórico me<->with
//!
//! Shadow-turn + handoff (escrita) ficam em F4-05f (precisa spawn Claude).

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use domain_humans::{
    create_human, delete_human, get_human_summary, human_summary, list_humans, load_humans,
    read_human_chat, summarize_human_chats, touch_human, update_human, Clock as HumansClock,
    CreateInput, Error as HumansError, SystemClock as HumansSystemClock, UpdateInput,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{json_error, json_response, AppState};

pub fn humans_router() -> Router<AppState> {
    Router::new()
        .route("/api/humans", get(list_handler).post(create_handler))
        .route(
            "/api/humans/:id",
            get(get_handler).put(update_handler).delete(delete_handler),
        )
        .route("/api/humans/:id/heartbeat", post(heartbeat_handler))
        .route("/api/humans/:id/chats", get(chats_summary_handler))
        .route("/api/human-chat/history", get(human_chat_history_handler))
}

async fn list_handler(State(st): State<AppState>) -> Response {
    let clock = HumansSystemClock;
    let list = list_humans(&st.mem_dir, clock.now_ms());
    json_response(StatusCode::OK, list)
}

async fn create_handler(State(st): State<AppState>, body: Bytes) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(name) = name else {
        return json_error(StatusCode::BAD_REQUEST, "need name");
    };

    let description = v.get("description").and_then(Value::as_str);
    let important = v.get("important").and_then(Value::as_str);
    let notes = v.get("notes").and_then(Value::as_str);

    let input = CreateInput {
        name,
        description,
        skills: v.get("skills"),
        important,
        offline_privacy: v.get("offline_privacy"),
        notes,
        location: v.get("location"),
        gender: v.get("gender"),
        skin: v.get("skin"),
        position: v.get("position"),
        ui_prefs: v.get("ui_prefs"),
    };

    let clock = HumansSystemClock;
    match create_human(&st.mem_dir, &input, &clock) {
        Ok(h) => {
            let summary = human_summary(&h, clock.now_ms());
            json_response(StatusCode::CREATED, json!({ "human": summary }))
        }
        Err(HumansError::NeedName) => json_error(StatusCode::BAD_REQUEST, "need name"),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn get_handler(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    let clock = HumansSystemClock;
    match get_human_summary(&st.mem_dir, &id, clock.now_ms()) {
        Some(summary) => json_response(StatusCode::OK, json!({ "human": summary })),
        None => json_error(StatusCode::NOT_FOUND, "not found"),
    }
}

async fn update_handler(
    State(st): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };

    // Paridade com `.cjs`: 404 precede o parse do body? Não — o `.cjs` faz o
    // JSON.parse primeiro, retorna 400 em JSON inválido. Só depois consulta
    // `data.humans[id]` e retorna 404. Mantemos a mesma ordem.
    let data = load_humans(&st.mem_dir);
    let exists = data
        .get("humans")
        .and_then(|m| m.get(&id))
        .is_some();
    if !exists {
        return json_error(StatusCode::NOT_FOUND, "not found");
    }

    // `.cjs` usa `body[f] === undefined` para decidir noop. Em JSON isso
    // significa "chave ausente no objeto". Se `v` não for objeto, nenhuma
    // chave está presente → nenhum update.
    let obj = v.as_object();
    let get_if_present = |key: &str| -> Option<&Value> { obj.and_then(|o| o.get(key)) };

    let input = UpdateInput {
        name: get_if_present("name"),
        description: get_if_present("description"),
        skills: get_if_present("skills"),
        important: get_if_present("important"),
        offline_privacy: get_if_present("offline_privacy"),
        notes: get_if_present("notes"),
        location: get_if_present("location"),
        gender: get_if_present("gender"),
        skin: get_if_present("skin"),
        position: get_if_present("position"),
        ui_prefs: get_if_present("ui_prefs"),
    };

    let clock = HumansSystemClock;
    match update_human(&st.mem_dir, &id, &input, &clock) {
        Ok(h) => {
            let summary = human_summary(&h, clock.now_ms());
            json_response(StatusCode::OK, json!({ "human": summary }))
        }
        Err(HumansError::NotFound) => json_error(StatusCode::NOT_FOUND, "not found"),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn delete_handler(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    let clock = HumansSystemClock;
    match delete_human(&st.mem_dir, &id, &clock) {
        Ok(rid) => json_response(StatusCode::OK, json!({ "ok": true, "id": rid })),
        Err(HumansError::NotFound) => json_error(StatusCode::NOT_FOUND, "not found"),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn heartbeat_handler(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    let clock = HumansSystemClock;
    match touch_human(&st.mem_dir, &id, &clock) {
        Ok(Some(h)) => {
            let last_seen = h
                .get("last_seen_at")
                .cloned()
                .unwrap_or(Value::Null);
            json_response(
                StatusCode::OK,
                json!({
                    "ok": true,
                    "last_seen_at": last_seen,
                    "is_online": true,
                }),
            )
        }
        Ok(None) => json_error(StatusCode::NOT_FOUND, "not found"),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn chats_summary_handler(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    // `.cjs` não valida existência do humano — só varre files por id.
    let v = summarize_human_chats(&st.mem_dir, &id);
    json_response(StatusCode::OK, v)
}

#[derive(Debug, Deserialize)]
struct ChatHistoryQuery {
    me: Option<String>,
    #[serde(rename = "with")]
    with_id: Option<String>,
}

async fn human_chat_history_handler(
    State(st): State<AppState>,
    Query(q): Query<ChatHistoryQuery>,
) -> Response {
    let me = q.me.as_deref().filter(|s| !s.is_empty());
    let with_id = q.with_id.as_deref().filter(|s| !s.is_empty());
    let (Some(me), Some(with_id)) = (me, with_id) else {
        return json_error(StatusCode::BAD_REQUEST, "need me & with");
    };
    let messages = read_human_chat(&st.mem_dir, me, with_id);
    json_response(
        StatusCode::OK,
        json!({
            "me": me,
            "with": with_id,
            "messages": messages,
        }),
    )
}

