//! Endpoints de DEV / benchmark — só montados no router quando a env
//! `POUPAR_DEV_MODE=1` está setada no startup. Sem isso, as rotas
//! não existem (404) e o código é morto pra prod.
//!
//! **NÃO LIGAR POUPAR_DEV_MODE EM PROD**: o endpoint `mint_token`
//! permite forjar identidade de qualquer agente cadastrado, quebrando
//! a garantia de não-impersonação do handoff.
//!
//! Razão de existir: bench end-to-end precisa de JWT válido pra bater
//! em /api/agents/handoff, mas em uso normal JWTs só são mintados pelo
//! spawn do agente (env do subprocess). Sem esse endpoint, bench precisa
//! reimplementar a lógica de mint inteira em script.

use axum::body::Bytes;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::post;
use axum::Router;
use serde_json::{json, Value};

use crate::agent_keys;
use crate::tenant::TenantContext;
use crate::{json_error, json_response, AppState};

pub fn is_dev_mode() -> bool {
    std::env::var("POUPAR_DEV_MODE").ok().as_deref() == Some("1")
}

/// Router das rotas dev. Retorna vazio quando `is_dev_mode()` é false —
/// caller faz `.merge(dev_router())` sem afetar o servidor em prod.
pub fn dev_router() -> Router<AppState> {
    if !is_dev_mode() {
        return Router::new();
    }
    Router::new().route("/api/_dev/mint_token", post(mint_token_handler))
}

/// `POST /api/_dev/mint_token` body `{agent, ttl_seconds?}` retorna JWT
/// assinado com a privkey do agente (lida do vault). Útil pra bench.
async fn mint_token_handler(
    State(st): State<AppState>,
    Extension(_tc): Extension<TenantContext>,
    body: Bytes,
) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let agent = v
        .get("agent")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(agent) = agent else {
        return json_error(StatusCode::BAD_REQUEST, "agent obrigatório");
    };
    match agent_keys::mint_token_for(agent, &st.vault, &st.mem_dir) {
        Some(token) => json_response(
            StatusCode::OK,
            json!({
                "ok": true,
                "agent": agent,
                "token": token,
                "warning": "DEV mode — endpoint não existe sem POUPAR_DEV_MODE=1",
            }),
        ),
        None => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "falha ao mintar — vault locked ou agente sem privkey?",
        ),
    }
}
