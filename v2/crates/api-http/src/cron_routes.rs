//! Rotas `/api/cron` — agendamento de jobs recorrentes.
//!
//! Endpoints:
//! - `GET    /api/cron`             — lista jobs (default: ativos. `?all=1` inclui inativos)
//! - `POST   /api/cron`             — cria job (auth Bearer opcional pra agentes; UI dispensa)
//! - `DELETE /api/cron/:id`         — soft-delete (active=false). Auth idem POST.
//!
//! ## Auth
//!
//! Modelo idêntico ao chat (`/api/chat`) e handoff: requests vindas com
//! `Authorization: Bearer <JWT Ed25519>` têm o `owner` derivado do claim
//! (ignora body.owner). Sem header, assume request da UI (loopback
//! confiado) e usa `body.owner` direto. Quem segura o token mintado é
//! o agente — então a UI nunca precisa apresentar um.
//!
//! ## Persistência
//!
//! Delega 100% pra `domain_cron::store` — JSONL append-only em
//! `<runtime mem_dir>/cron_jobs.jsonl`. Adapter HTTP fica thin.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::{delete, get},
    Router,
};
use bytes::Bytes;
use domain_cron::{model::JobAction, store, CronJob};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{json_error, json_response, AppState};

pub fn cron_router() -> Router<AppState> {
    Router::new()
        .route("/api/cron", get(list_handler).post(create_handler))
        .route("/api/cron/:id", delete(delete_handler))
}

#[derive(Deserialize, Default)]
struct ListQuery {
    #[serde(default)]
    all: Option<String>,
}

async fn list_handler(State(st): State<AppState>, Query(q): Query<ListQuery>) -> Response {
    let include_inactive = matches!(q.all.as_deref(), Some("1") | Some("true"));
    let now = unix_now_ms();
    let result = if include_inactive {
        store::list_all(&st.mem_dir)
    } else {
        store::list_active(&st.mem_dir, now)
    };
    match result {
        Ok(jobs) => json_response(StatusCode::OK, jobs_to_json(&jobs)),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn create_handler(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let target = v.get("target").and_then(Value::as_str).unwrap_or("");
    let message = v.get("message").and_then(Value::as_str).unwrap_or("");
    let schedule = v.get("schedule").and_then(Value::as_str).unwrap_or("");
    if target.is_empty() || message.is_empty() || schedule.is_empty() {
        return json_error(
            StatusCode::BAD_REQUEST,
            "need {target, message, schedule}",
        );
    }

    // Auth: se vier Bearer, owner = claim.agent; senão, body.owner (UI).
    let owner = match owner_from_request(&st, &headers, &v) {
        Ok(o) => o,
        Err(resp) => return resp,
    };

    let job = CronJob {
        id: gen_id(),
        owner,
        schedule: schedule.to_string(),
        action: JobAction::Chat {
            target: target.to_string(),
            message: message.to_string(),
        },
        created_at_ms: unix_now_ms(),
        active: true,
        last_run_ms: None,
        next_run_ms: None,
    };
    match store::create_job(&st.mem_dir, job) {
        Ok(j) => json_response(StatusCode::OK, job_to_json(&j, unix_now_ms())),
        Err(store::StoreError::BadSchedule(e)) => {
            json_error(StatusCode::BAD_REQUEST, &format!("schedule inválido: {e}"))
        }
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn delete_handler(
    State(st): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    // Se Bearer presente: claim.agent precisa casar com job.owner.
    let bearer_agent = bearer_claim(&headers);
    if let Some(claimer) = &bearer_agent {
        let jobs = match store::list_all(&st.mem_dir) {
            Ok(v) => v,
            Err(e) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
        };
        match jobs.iter().find(|j| j.id == id) {
            Some(j) if j.owner != *claimer => {
                return json_error(StatusCode::FORBIDDEN, "owner mismatch");
            }
            None => return json_error(StatusCode::NOT_FOUND, "job não encontrado"),
            _ => {}
        }
    }
    match store::deactivate_job(&st.mem_dir, &id) {
        Ok(()) => json_response(StatusCode::OK, json!({"ok": true})),
        Err(store::StoreError::NotFound(_)) => {
            json_error(StatusCode::NOT_FOUND, "job não encontrado")
        }
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

// ── helpers ────────────────────────────────────────────────────────────────

fn owner_from_request(
    st: &AppState,
    headers: &HeaderMap,
    body: &Value,
) -> Result<String, Response> {
    if let Some(token) = bearer_token(headers) {
        let mem_dir = st.mem_dir.clone();
        match crate::agent_keys::verify_jwt(&token, move |name| {
            crate::agent_keys::lookup_pubkey(&mem_dir, name)
        }) {
            Ok(claims) => Ok(claims.agent),
            Err(e) => Err(json_error(
                StatusCode::UNAUTHORIZED,
                &format!("JWT inválido: {e}"),
            )),
        }
    } else {
        let owner = body.get("owner").and_then(Value::as_str).unwrap_or("");
        if owner.is_empty() {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "owner obrigatório (ou Authorization: Bearer)",
            ));
        }
        Ok(owner.to_string())
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(String::from)
}

fn bearer_claim(headers: &HeaderMap) -> Option<String> {
    // Não decodifica/verifica aqui — só extrai o nome do claim pra check
    // de ownership. Verificação completa é feita pelo `verify_jwt` na
    // criação. Trade-off aceito: DELETE sem Bearer (UI) já está allow,
    // então DELETE com Bearer só precisa de ownership match — proteção
    // extra contra agente A apagar cron do agente B.
    let token = bearer_token(headers)?;
    // header.payload.sig — decodifica payload com base64-url
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let payload_bytes = URL_SAFE_NO_PAD.decode(parts[1].as_bytes()).ok()?;
    let payload: Value = serde_json::from_slice(&payload_bytes).ok()?;
    payload.get("agent").and_then(Value::as_str).map(String::from)
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn gen_id() -> String {
    use rand_core::RngCore;
    let mut buf = [0u8; 12];
    rand_core::OsRng.fill_bytes(&mut buf);
    let mut s = String::with_capacity(24);
    for b in buf {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    format!("c_{s}")
}

fn jobs_to_json(jobs: &[CronJob]) -> Value {
    let now = unix_now_ms();
    Value::Array(jobs.iter().map(|j| job_to_json(j, now)).collect())
}

fn job_to_json(j: &CronJob, _now: u64) -> Value {
    let (target, message) = match &j.action {
        JobAction::Chat { target, message } => (target.as_str(), message.as_str()),
    };
    json!({
        "id":            j.id,
        "owner":         j.owner,
        "target":        target,
        "message":       message,
        "schedule":      j.schedule,
        "created_at_ms": j.created_at_ms,
        "active":        j.active,
        "next_run_ms":   j.next_run_ms,
        "last_run_ms":   j.last_run_ms,
    })
}
