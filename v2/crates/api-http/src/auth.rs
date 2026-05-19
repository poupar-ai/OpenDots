//! Middleware de auth — resolve a request num `TenantContext`.
//!
//! Hoje single-tenant: lê (opcionalmente) `Authorization: Bearer <token>`
//! e ignora o valor, retornando sempre o tenant `default`. Sem header
//! também → `default`. Logging zero pra não poluir.
//!
//! Quando multi-tenancy real ativar:
//!   1. Substituir `resolve_tenant` por lookup numa tabela token→tenant_id
//!   2. Retornar 401 quando o token não bate
//!   3. Endpoints públicos (ex.: UI local) podem ser whitelistados antes
//!
//! Handlers acessam via `Extension<TenantContext>` no signature.

use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;

use crate::tenant::TenantContext;
use crate::AppState;

/// Resolve o tenant baseado nos headers da request.
/// Single-tenant hoje: sempre `default_for(state.mem_dir)`.
fn resolve_tenant(_headers: &HeaderMap, state: &AppState) -> TenantContext {
    // Futuro: ler "Authorization: Bearer <token>", buscar em DB, etc.
    // Hoje, sem efeito — devolve default.
    TenantContext::default_for(&state.mem_dir)
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let tenant = resolve_tenant(req.headers(), &state);
    req.extensions_mut().insert(tenant);
    next.run(req).await
}
