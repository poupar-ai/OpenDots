//! Rotas `/api/topics/*` — pub/sub por topic.
//!
//! - `GET    /api/topics`                              → lista topics
//! - `GET    /api/topics/:name/subscribers`            → quem assina
//! - `PUT    /api/topics/:name/subscribe/:agent`       → assina
//! - `DELETE /api/topics/:name/subscribe/:agent`       → desassina
//! - `POST   /api/topics/:name/publish`                → publica (JWT obrigatório)
//!
//! Publish entrega via `auto_queue.enqueue_with` pra cada subscriber,
//! com prefixo `"[topic:<name>] <from>: <message>"` no payload pra B
//! saber o contexto. Append no `events.jsonl` pra audit.

use axum::body::Bytes;
use axum::extract::{Extension, Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::{get, post, put};
use axum::Router;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::agent_keys;
use crate::tenant::TenantContext;
use crate::topics;
use crate::{json_error, json_response, AppState};

pub fn topic_router() -> Router<AppState> {
    Router::new()
        .route("/api/topics", get(list_handler))
        .route("/api/topics/:name/subscribers", get(subscribers_handler))
        .route("/api/topics/:name/subscribe/:agent", put(subscribe_handler).delete(unsubscribe_handler))
        .route("/api/topics/:name/publish", post(publish_handler))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

async fn list_handler(Extension(tc): Extension<TenantContext>) -> Response {
    let names = topics::list_topics(&tc.mem_dir);
    let mut detailed: Vec<Value> = Vec::with_capacity(names.len());
    for name in &names {
        let subs = topics::load_subscribers(&tc.mem_dir, name);
        detailed.push(json!({
            "name": name,
            "subscribers_count": subs.agents.len(),
        }));
    }
    json_response(StatusCode::OK, json!({ "topics": detailed }))
}

async fn subscribers_handler(
    Extension(tc): Extension<TenantContext>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    if !topics::is_valid_topic_name(&name) {
        return json_error(StatusCode::BAD_REQUEST, "topic name inválido");
    }
    let subs = topics::load_subscribers(&tc.mem_dir, &name);
    json_response(
        StatusCode::OK,
        json!({ "topic": name, "subscribers": subs.agents }),
    )
}

async fn subscribe_handler(
    Extension(tc): Extension<TenantContext>,
    AxumPath((name, agent)): AxumPath<(String, String)>,
) -> Response {
    if !topics::is_valid_topic_name(&name) {
        return json_error(StatusCode::BAD_REQUEST, "topic name inválido");
    }
    if agent.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "agent vazio");
    }
    let mut subs = topics::load_subscribers(&tc.mem_dir, &name);
    let inserted = subs.agents.insert(agent.clone());
    if let Err(e) = topics::save_subscribers(&tc.mem_dir, &name, &subs) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("falha ao gravar: {e}"));
    }
    json_response(
        StatusCode::OK,
        json!({
            "ok": true,
            "topic": name,
            "agent": agent,
            "inserted": inserted,
        }),
    )
}

async fn unsubscribe_handler(
    Extension(tc): Extension<TenantContext>,
    AxumPath((name, agent)): AxumPath<(String, String)>,
) -> Response {
    if !topics::is_valid_topic_name(&name) {
        return json_error(StatusCode::BAD_REQUEST, "topic name inválido");
    }
    let mut subs = topics::load_subscribers(&tc.mem_dir, &name);
    let removed = subs.agents.remove(&agent);
    if let Err(e) = topics::save_subscribers(&tc.mem_dir, &name, &subs) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("falha ao gravar: {e}"));
    }
    json_response(
        StatusCode::OK,
        json!({
            "ok": true,
            "topic": name,
            "agent": agent,
            "removed": removed,
        }),
    )
}

async fn publish_handler(
    State(st): State<AppState>,
    Extension(tc): Extension<TenantContext>,
    AxumPath(name): AxumPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !topics::is_valid_topic_name(&name) {
        return json_error(StatusCode::BAD_REQUEST, "topic name inválido");
    }

    // JWT obrigatório (mesmo padrão do handoff). `from` autoritativo
    // vem do claim — body é ignorado pra esse campo.
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    let Some(token) = bearer else {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "Authorization: Bearer <JWT> obrigatório pra publish",
        );
    };
    let mem_dir_for_lookup = st.mem_dir.clone();
    let claims = match agent_keys::verify_jwt(token, |n| {
        agent_keys::lookup_pubkey(&mem_dir_for_lookup, n)
    }) {
        Ok(c) => c,
        Err(e) => return json_error(StatusCode::UNAUTHORIZED, &format!("JWT inválido: {e}")),
    };
    let from = claims.agent;

    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let message = v.get("message").and_then(Value::as_str).unwrap_or("");
    if message.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "message obrigatória");
    }
    let priority = v
        .get("priority")
        .and_then(Value::as_str)
        .map(core_queue::Priority::parse)
        .unwrap_or(core_queue::Priority::Normal);

    let subs = topics::load_subscribers(&tc.mem_dir, &name);
    let msg_id = format!("topic-{}-{}", name, now_ms());
    let prefixed = format!("[topic:{name}] {from}: {message}");

    let mut delivered: Vec<String> = Vec::new();
    let mut skipped_self: bool = false;
    for agent in &subs.agents {
        // Não entrega pro próprio publicador (evita loop).
        if agent == &from {
            skipped_self = true;
            continue;
        }
        let ok = st.auto_queue.enqueue_with(
            agent,
            &from,
            &prefixed,
            Some(&msg_id),
            priority,
        );
        if ok {
            delivered.push(agent.clone());
        }
    }

    let event = json!({
        "ts_ms": now_ms(),
        "msg_id": msg_id,
        "from": from,
        "message": message,
        "delivered_to": delivered,
        "skipped_self": skipped_self,
        "priority": v.get("priority").cloned().unwrap_or(Value::String("normal".into())),
    });
    let _ = topics::append_event(&tc.mem_dir, &name, &event);

    json_response(
        StatusCode::OK,
        json!({
            "ok": true,
            "topic": name,
            "msg_id": event["msg_id"],
            "delivered_to": event["delivered_to"],
            "subscribers_count": subs.agents.len(),
        }),
    )
}
