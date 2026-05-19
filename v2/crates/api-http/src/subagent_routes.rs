//! Rotas `/api/agents/subagent/*` — dispatch e polling de subagents
//! efêmeros paralelos.
//!
//! - `POST /api/agents/subagent/spawn`       body `{prompt, model?, tools?, timeout_secs?}`
//!   → `{subagent_id, status: "running"}`. JWT obrigatório (pai assina).
//! - `GET  /api/agents/subagent/:id`         → status + result se done.
//! - `POST /api/agents/subagent/spawn_batch` body `{tasks: [{prompt,...}]}`
//!   → `{batch_id, subagent_ids: [...]}`. Spawn N paralelos.
//! - `GET  /api/agents/subagent/batch/:id`   → estado agregado do batch.
//!
//! ## Rate limit
//!
//! Cada pai pode ter MAX_PARALLEL subagents `Running` simultâneos.
//! Default 10. Acima disso, 429 com hint.

use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Extension, Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::agent_keys;
use crate::subagent::{new_batch_id, SubagentStatus};
use crate::tenant::TenantContext;
use crate::{json_error, json_response, AppState};

const MAX_PARALLEL_PER_PARENT: usize = 10;
const DEFAULT_TIMEOUT_SECS: u64 = 600; // 10 min
const MAX_TIMEOUT_SECS: u64 = 3600; // 1 h
const MAX_BATCH_TASKS: usize = 20;

pub fn subagent_router() -> Router<AppState> {
    Router::new()
        .route("/api/agents/subagent", get(list_handler))
        .route("/api/agents/subagent/spawn", post(spawn_handler))
        .route("/api/agents/subagent/spawn_batch", post(spawn_batch_handler))
        .route(
            "/api/agents/subagent/:id",
            get(get_handler).delete(cancel_handler),
        )
        .route("/api/agents/subagent/batch/:id", get(batch_handler))
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    parent: Option<String>,
    status: Option<String>,
    limit: Option<usize>,
}

/// `GET /api/agents/subagent?parent=X&status=running&limit=50` —
/// listagem pro SubagentPanel da UI. Sem JWT (somente leitura,
/// usado por UI local).
async fn list_handler(
    State(st): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Response {
    let all = st.subagents.list(q.parent.as_deref());
    let limit = q.limit.unwrap_or(100).min(500);
    let snaps: Vec<Value> = all
        .iter()
        .filter_map(|s| {
            let snap = s.lock().unwrap().clone();
            if let Some(ref want) = q.status {
                let cur = serde_json::to_value(&snap.status).ok()?;
                if cur.as_str() != Some(want.as_str()) {
                    return None;
                }
            }
            serde_json::to_value(snap).ok()
        })
        .take(limit)
        .collect();
    json_response(
        StatusCode::OK,
        json!({
            "count": snaps.len(),
            "subagents": snaps,
        }),
    )
}

/// `DELETE /api/agents/subagent/:id` — cancela subagent. Requer JWT
/// do pai (não dá pra cancelar subagent de outro agent).
async fn cancel_handler(
    State(st): State<AppState>,
    Extension(tc): Extension<TenantContext>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    let requester = match require_parent_agent(&headers, &tc.mem_dir) {
        Ok(a) => a,
        Err(r) => return r,
    };
    // Confere que o requester É o pai do subagent.
    let Some(state_arc) = st.subagents.get(&id) else {
        return json_error(StatusCode::NOT_FOUND, "subagent não encontrado");
    };
    let parent = state_arc.lock().unwrap().parent_agent.clone();
    if parent != requester {
        return json_error(
            StatusCode::FORBIDDEN,
            "só o pai do subagent pode cancelá-lo",
        );
    }
    match st.subagents.cancel(&id) {
        Ok(()) => json_response(
            StatusCode::OK,
            json!({ "ok": true, "subagent_id": id, "status": "cancelled" }),
        ),
        Err(e) => json_error(StatusCode::CONFLICT, e),
    }
}

/// Extrai JWT, valida, retorna o `agent` autoritativo do claim.
/// Usado por todos os spawn handlers — pai precisa provar identidade.
fn require_parent_agent(
    headers: &HeaderMap,
    mem_dir: &std::path::Path,
) -> Result<String, Response> {
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    let Some(token) = bearer else {
        return Err(json_error(
            StatusCode::UNAUTHORIZED,
            "Authorization: Bearer <JWT> obrigatório",
        ));
    };
    let mem_dir_owned = mem_dir.to_path_buf();
    match agent_keys::verify_jwt(token, |name| {
        agent_keys::lookup_pubkey(&mem_dir_owned, name)
    }) {
        Ok(c) => Ok(c.agent),
        Err(e) => Err(json_error(
            StatusCode::UNAUTHORIZED,
            &format!("JWT inválido: {e}"),
        )),
    }
}

fn validate_tools(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .filter(|s| !s.is_empty() && s.len() <= 64)
                .take(20)
                .collect()
        })
        .unwrap_or_else(|| vec!["Read".into(), "Grep".into(), "Glob".into()])
}

fn validate_timeout(v: Option<&Value>) -> Duration {
    let secs = v
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(1, MAX_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

async fn spawn_handler(
    State(st): State<AppState>,
    Extension(tc): Extension<TenantContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let parent = match require_parent_agent(&headers, &tc.mem_dir) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let prompt = v
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(prompt) = prompt else {
        return json_error(StatusCode::BAD_REQUEST, "prompt obrigatório");
    };

    // Rate limit por pai.
    let in_flight = st.subagents.count_running_for(&parent);
    if in_flight >= MAX_PARALLEL_PER_PARENT {
        return json_error(
            StatusCode::TOO_MANY_REQUESTS,
            &format!(
                "máx {MAX_PARALLEL_PER_PARENT} subagents simultâneos por pai. Em flight: {in_flight}. Espere alguns terminarem."
            ),
        );
    }

    let model = v.get("model").and_then(Value::as_str).map(String::from);
    let tools = validate_tools(v.get("tools"));
    let timeout = validate_timeout(v.get("timeout_secs"));

    let id = st.subagents.spawn(
        &parent,
        prompt.to_string(),
        st.project_root.clone(),
        model,
        tools,
        timeout,
        None,
    );

    json_response(
        StatusCode::ACCEPTED,
        json!({
            "ok": true,
            "subagent_id": id,
            "status": "running",
            "parent_agent": parent,
        }),
    )
}

async fn spawn_batch_handler(
    State(st): State<AppState>,
    Extension(tc): Extension<TenantContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let parent = match require_parent_agent(&headers, &tc.mem_dir) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let tasks = match v.get("tasks").and_then(Value::as_array) {
        Some(arr) if !arr.is_empty() => arr,
        _ => return json_error(StatusCode::BAD_REQUEST, "tasks (array não-vazio) obrigatório"),
    };
    if tasks.len() > MAX_BATCH_TASKS {
        return json_error(
            StatusCode::BAD_REQUEST,
            &format!("máx {MAX_BATCH_TASKS} tasks por batch"),
        );
    }

    // Verifica rate limit: in_flight + tasks.len() > MAX → 429.
    let in_flight = st.subagents.count_running_for(&parent);
    if in_flight + tasks.len() > MAX_PARALLEL_PER_PARENT {
        return json_error(
            StatusCode::TOO_MANY_REQUESTS,
            &format!(
                "máx {MAX_PARALLEL_PER_PARENT} subagents por pai; in_flight={in_flight} + batch={} > limite",
                tasks.len()
            ),
        );
    }

    let batch_id = new_batch_id();
    let mut ids = Vec::with_capacity(tasks.len());
    let mut errors = Vec::new();
    for (i, task) in tasks.iter().enumerate() {
        let prompt = task
            .get("prompt")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let Some(prompt) = prompt else {
            errors.push(json!({"index": i, "error": "prompt obrigatório"}));
            continue;
        };
        let model = task.get("model").and_then(Value::as_str).map(String::from);
        let tools = validate_tools(task.get("tools"));
        let timeout = validate_timeout(task.get("timeout_secs"));
        let id = st.subagents.spawn(
            &parent,
            prompt.to_string(),
            st.project_root.clone(),
            model,
            tools,
            timeout,
            Some(batch_id.clone()),
        );
        ids.push(json!({"index": i, "subagent_id": id}));
    }

    json_response(
        StatusCode::ACCEPTED,
        json!({
            "ok": errors.is_empty(),
            "batch_id": batch_id,
            "parent_agent": parent,
            "spawned": ids,
            "errors": errors,
        }),
    )
}

async fn get_handler(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let Some(state_arc) = st.subagents.get(&id) else {
        return json_error(StatusCode::NOT_FOUND, "subagent não encontrado (cleanup?)");
    };
    let snap = state_arc.lock().unwrap().clone();
    json_response(StatusCode::OK, serde_json::to_value(snap).unwrap_or(Value::Null))
}

async fn batch_handler(
    State(st): State<AppState>,
    AxumPath(batch_id): AxumPath<String>,
) -> Response {
    let items = st.subagents.list_batch(&batch_id);
    if items.is_empty() {
        return json_error(StatusCode::NOT_FOUND, "batch não encontrado");
    }
    let snaps: Vec<Value> = items
        .iter()
        .map(|s| {
            let snap = s.lock().unwrap().clone();
            serde_json::to_value(snap).unwrap_or(Value::Null)
        })
        .collect();
    let total = snaps.len();
    let running = snaps
        .iter()
        .filter(|s| s.get("status") == Some(&Value::String("running".into())))
        .count();
    let done = snaps
        .iter()
        .filter(|s| s.get("status") == Some(&Value::String("done".into())))
        .count();
    let failed = snaps
        .iter()
        .filter(|s| {
            matches!(
                s.get("status").and_then(Value::as_str),
                Some("failed") | Some("timed_out")
            )
        })
        .count();
    let _ = SubagentStatus::Running;
    json_response(
        StatusCode::OK,
        json!({
            "batch_id": batch_id,
            "total": total,
            "running": running,
            "done": done,
            "failed": failed,
            "complete": running == 0,
            "subagents": snaps,
        }),
    )
}
