//! Rotas `/api/scope` — leitura/edição do `project_root` (escopo do
//! agente) sem mexer em env. Persiste em `<mem_dir>/scope.json` via
//! [`core_paths::write_scope_project_root`].
//!
//! - `GET  /api/scope` → `{ project_root, mem_dir, default_project_root, source }`
//! - `PUT  /api/scope` → body `{ project_root: "<abs path>" }`, valida
//!   que existe e é dir, grava em `scope.json`, indica restart.
//!
//! Handlers usam `Extension<TenantContext>` em vez de `st.mem_dir` —
//! padrão pra todo handler novo (ver CLAUDE.md). Hoje sempre devolve
//! tenant `default`, mas a indireção remove o lock-in single-tenant.

use axum::body::Bytes;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use core_paths::{read_scope_project_root, write_scope_project_root};
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::tenant::TenantContext;
use crate::{json_error, json_response, AppState};

pub fn scope_router() -> Router<AppState> {
    Router::new()
        .route("/api/scope", get(get_scope).put(put_scope))
        .route("/api/scope/suggestions", get(get_suggestions))
}

async fn get_scope(
    State(st): State<AppState>,
    Extension(tc): Extension<TenantContext>,
) -> Response {
    let from_scope = read_scope_project_root(&tc.mem_dir);
    let source = if from_scope.is_some() { "scope.json" } else { "default" };
    json_response(
        StatusCode::OK,
        json!({
            "project_root": st.project_root.to_string_lossy(),
            "mem_dir":      tc.mem_dir.to_string_lossy(),
            "source":       source,
            "tenant":       tc.id,
        }),
    )
}

/// `GET /api/scope/suggestions` — pasta-pai do clone + pastas-irmãs.
/// Usado pela UI pra quick-pick (chips clicáveis) — usuário não precisa
/// digitar paths comuns à mão. `parent` = candidato natural quando o
/// plugin é instalado num workspace; `siblings` = projetos vizinhos
/// (cada um pode ser project_root individualmente).
async fn get_suggestions(Extension(tc): Extension<TenantContext>) -> Response {
    // mem_dir = <plugin_clone>/.poupar
    let plugin_clone = tc.mem_dir.parent();
    let workspace = plugin_clone.and_then(|p| p.parent());

    let parent = workspace.map(|p| p.to_string_lossy().to_string());

    let siblings: Vec<String> = workspace
        .and_then(|ws| std::fs::read_dir(ws).ok())
        .map(|entries| {
            let mut out: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .filter(|e| {
                    let name = e.file_name();
                    let s = name.to_string_lossy();
                    // Pula hidden (.git, .cache, etc.) e o próprio clone.
                    !s.starts_with('.') && Some(e.path().as_path()) != plugin_clone
                })
                .map(|e| e.path().to_string_lossy().to_string())
                .collect();
            out.sort();
            out
        })
        .unwrap_or_default();

    json_response(
        StatusCode::OK,
        json!({
            "parent": parent,
            "siblings": siblings,
        }),
    )
}

async fn put_scope(
    State(st): State<AppState>,
    Extension(tc): Extension<TenantContext>,
    body: Bytes,
) -> Response {
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let pr = parsed
        .get("project_root")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(pr) = pr else {
        return json_error(StatusCode::BAD_REQUEST, "project_root é obrigatório");
    };
    let path = PathBuf::from(pr);
    if !path.is_absolute() {
        return json_error(StatusCode::BAD_REQUEST, "project_root precisa ser caminho absoluto");
    }
    if !path.exists() {
        return json_error(StatusCode::BAD_REQUEST, "pasta não existe");
    }
    if !path.is_dir() {
        return json_error(StatusCode::BAD_REQUEST, "caminho não é uma pasta");
    }
    if let Err(e) = write_scope_project_root(&tc.mem_dir, &path) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("falha ao gravar: {e}"));
    }
    // Pré-materializa os MCPs registrados no .mcp.json do NOVO project_root,
    // pra que ele já fique pronto antes do restart (st.project_root só passa a
    // apontar pro novo escopo no próximo boot — ver startup materialize_all).
    let n_mcp = crate::mcp_routes::materialize_all(&tc.mem_dir, &path, &st.vault);
    json_response(
        StatusCode::OK,
        json!({
            "ok": true,
            "project_root": path.to_string_lossy(),
            "restart_required": true,
            "mcps_materialized": n_mcp,
            "tenant": tc.id,
        }),
    )
}
