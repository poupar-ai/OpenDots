//! Middleware de auth — resolve a request num `TenantContext` e (opt-in)
//! exige Bearer token global do servidor.
//!
//! Single-tenant hoje. Bearer enforcement é OPT-IN via env
//! `PROJECT_MEM_REQUIRE_TOKEN=1`:
//!   - Off (default): comportamento atual, sem auth header obrigatório.
//!   - On: middleware rejeita 401 quando header ausente/inválido —
//!     EXCETO em rotas whitelistadas (status do token, init de vault)
//!     pra UI conseguir bootstrappar.
//!
//! Quando multi-tenancy real ativar, substitui o resolve_tenant aqui.

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, Response as HttpResponse, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

use crate::auth_routes::load_server_token;
use crate::tenant::TenantContext;
use crate::AppState;

/// Rotas que NUNCA exigem token — UI precisa conseguir ler o status
/// inicial pra orientar o user a configurar.
const WHITELIST_PREFIXES: &[&str] = &[
    "/api/auth/server-token", // GET status / preview
    "/",                       // serve_index
];

fn is_whitelisted(path: &str) -> bool {
    WHITELIST_PREFIXES.iter().any(|p| path == *p || path.starts_with(&format!("{p}/")))
}

fn resolve_tenant(_headers: &HeaderMap, state: &AppState) -> TenantContext {
    // Hoje sempre default; futuro: lookup token → tenant.
    TenantContext::default_for(&state.mem_dir)
}

fn unauthorized(msg: &str) -> Response {
    let body = format!(r#"{{"error":"{msg}"}}"#);
    HttpResponse::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap()
}

fn require_token_enabled() -> bool {
    std::env::var("PROJECT_MEM_REQUIRE_TOKEN").ok().as_deref() == Some("1")
}

fn forbidden(msg: &str) -> Response {
    let body = format!(r#"{{"error":"{msg}"}}"#);
    HttpResponse::builder()
        .status(StatusCode::FORBIDDEN)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap()
}

/// Origin-guard pra rotas de ESCRITA (POST/PUT/PATCH/DELETE). Defende contra
/// CSRF/cross-origin quando o servidor está bindado na LAN: uma página em
/// outro site não consegue mandar requests mutáveis pra cá.
///
/// - GET/HEAD/OPTIONS passam direto (read-only / preflight).
/// - `Sec-Fetch-Site: cross-site` → bloqueia (sinal forte e moderno do browser).
/// - `Origin` presente e fora da allowlist → bloqueia (fallback p/ browsers
///   sem Sec-Fetch-Site). A allowlist inclui a porta da API e a do Vite dev.
/// - Sem Origin nem Sec-Fetch-Site (curl, agentes via pmem) → passa.
///
/// Desligável com `PROJECT_MEM_GUARD_ORIGIN=0` (default: ligado).
pub async fn origin_guard_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let mutating = matches!(
        *req.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    let disabled = std::env::var("PROJECT_MEM_GUARD_ORIGIN").ok().as_deref() == Some("0");
    if mutating && !disabled {
        let headers = req.headers();
        if let Some(sfs) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
            if sfs.eq_ignore_ascii_case("cross-site") {
                return forbidden("cross-site bloqueado");
            }
        }
        if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
            if !crate::vault_routes::allowed_origins(&state).contains(&origin.to_lowercase()) {
                return forbidden("cross-origin bloqueado");
            }
        }
    }
    next.run(req).await
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    // Bearer enforcement opcional.
    if require_token_enabled() && !is_whitelisted(req.uri().path()) {
        let expected = load_server_token(&state.mem_dir);
        // Sem token configurado mas enforcement ligado = sistema fechado.
        // Mensagem distinta pra UI orientar a gerar.
        let Some(expected) = expected else {
            return unauthorized("server_token não configurado — gere em /api/auth/server-token/generate");
        };
        let got = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "));
        match got {
            Some(t) if t == expected => {}
            _ => return unauthorized("Authorization: Bearer <token> ausente ou inválido"),
        }
    }

    let tenant = resolve_tenant(req.headers(), &state);
    req.extensions_mut().insert(tenant);
    next.run(req).await
}
