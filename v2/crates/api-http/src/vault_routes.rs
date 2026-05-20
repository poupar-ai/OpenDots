//! Rotas `/api/vault/*` — paridade com `server.cjs:3122-3396`.
//!
//! Guard aplicado antes de cada rota:
//! - `Host` header precisa estar em `VAULT_ALLOWED_HOSTS` (rebinding).
//! - `Origin` (se presente) em `VAULT_ALLOWED_ORIGINS`.
//! - `Sec-Fetch-Site` precisa ser `same-origin` ou `none`.
//!
//! Erros do vault viram 400 por default; `Locked`/`NotInitialized` viram 423.

use std::collections::HashSet;

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use base64::Engine;
use core_ports::VaultError;
use serde_json::{json, Map, Value};

use crate::unlock_guard::RateLimit;
use crate::{json_error, json_response, AppState};

pub fn vault_router() -> Router<AppState> {
    Router::new()
        .route("/api/vault/status", get(status))
        .route("/api/vault/paths", get(paths))
        .route("/api/vault/init", post(init))
        .route("/api/vault/unlock", post(unlock))
        .route("/api/vault/lock", post(lock))
        .route("/api/vault/change-password", post(change_password))
        .route("/api/vault/keys", get(keys))
        .route(
            "/api/vault/secret/:name",
            get(secret_get).put(secret_put).delete(secret_delete),
        )
        .route("/api/vault/envfiles", get(envfiles))
        .route(
            "/api/vault/envfile/:name",
            get(envfile_get).put(envfile_put).delete(envfile_delete),
        )
        .route("/api/vault/export", post(export))
        .route("/api/vault/import", post(import))
        .route("/api/vault/destroy", post(destroy))
}

// ── Guard ───────────────────────────────────────────────────────────────────

/// Porta do Vite dev server (proxy /api → API). Incluída nas allowlists pra
/// o fluxo de desenvolvimento (UI em :5173 proxiando pra cá) não ser barrado.
const DEV_PROXY_PORT: u16 = 5173;

/// Hosts permitidos (anti DNS-rebinding). Inclui a porta da API e a do Vite
/// dev. `pub(crate)` pra ser reusada pelo middleware global de origin-guard.
pub(crate) fn allowed_hosts(st: &AppState) -> HashSet<String> {
    let mut out = HashSet::new();
    let cfg = st.network_cfg.lock().unwrap().clone();
    for port in [st.port, DEV_PROXY_PORT] {
        out.insert(format!("localhost:{port}"));
        out.insert(format!("127.0.0.1:{port}"));
        match cfg.bind.as_str() {
            "lan" => {
                for ip in crate::list_lan_ips() {
                    out.insert(format!("{}:{}", ip.address, port));
                }
            }
            "custom" if !cfg.custom_host.is_empty() => {
                out.insert(format!("{}:{}", cfg.custom_host, port));
            }
            _ => {}
        }
    }
    out
}

/// Origins permitidos (anti CSRF/cross-origin). Mesma cobertura de portas.
pub(crate) fn allowed_origins(st: &AppState) -> HashSet<String> {
    let mut out = HashSet::new();
    let cfg = st.network_cfg.lock().unwrap().clone();
    for port in [st.port, DEV_PROXY_PORT] {
        out.insert(format!("http://localhost:{port}"));
        out.insert(format!("http://127.0.0.1:{port}"));
        match cfg.bind.as_str() {
            "lan" => {
                for ip in crate::list_lan_ips() {
                    out.insert(format!("http://{}:{}", ip.address, port));
                }
            }
            "custom" if !cfg.custom_host.is_empty() => {
                out.insert(format!("http://{}:{}", cfg.custom_host, port));
            }
            _ => {}
        }
    }
    out
}

/// Middleware manual — chamado no início de cada handler. Retorna
/// `Some(Response)` se bloqueado; `None` se pode prosseguir.
fn guard(st: &AppState, headers: &HeaderMap) -> Option<Response> {
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    if !allowed_hosts(st).contains(&host) {
        return Some(json_error(
            StatusCode::FORBIDDEN,
            "host não permitido (dns rebinding?)",
        ));
    }
    if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        let origin_lc = origin.to_lowercase();
        if !allowed_origins(st).contains(&origin_lc) {
            return Some(json_error(StatusCode::FORBIDDEN, "cross-origin bloqueado"));
        }
    }
    if let Some(sfs) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        let lc = sfs.to_lowercase();
        if !lc.is_empty() && lc != "same-origin" && lc != "none" {
            return Some(json_error(
                StatusCode::FORBIDDEN,
                "sec-fetch-site bloqueado",
            ));
        }
    }
    None
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn vault_status_to_json(s: &core_ports::VaultStatus) -> Value {
    let mut m = Map::new();
    m.insert("initialized".into(), Value::Bool(s.initialized));
    m.insert("locked".into(), Value::Bool(s.locked));
    m.insert(
        "expires_at".into(),
        s.expires_at.map(Value::from).unwrap_or(Value::Null),
    );
    m.insert(
        "ttl_ms".into(),
        s.ttl_ms.map(Value::from).unwrap_or(Value::Null),
    );
    m.insert("needs_migration".into(), Value::Bool(s.needs_migration));
    Value::Object(m)
}

fn vault_error_response(err: &VaultError) -> Response {
    let msg = err.to_string();
    // `.cjs` usa regex `/UI locked|not initialized/i` — aqui mapeamos
    // explicitamente por variant.
    let status = match err {
        VaultError::Locked | VaultError::NotInitialized => StatusCode::LOCKED,
        _ => StatusCode::BAD_REQUEST,
    };
    json_error(status, &msg)
}

// ── Handlers ────────────────────────────────────────────────────────────────

async fn status(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(r) = guard(&st, &headers) {
        return r;
    }
    match st.vault.status(&st.mem_dir) {
        Ok(s) => json_response(StatusCode::OK, vault_status_to_json(&s)),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn paths(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(r) = guard(&st, &headers) {
        return r;
    }
    let mem = st.mem_dir.display().to_string();
    let vault_file = st.mem_dir.join("vault.enc").display().to_string();
    let device_key_file = st.mem_dir.join(".vault-devicekey").display().to_string();
    json_response(
        StatusCode::OK,
        json!({
            "mem_dir": mem,
            "vault_file": vault_file,
            "device_key_file": device_key_file,
        }),
    )
}

async fn init(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(r) = guard(&st, &headers) {
        return r;
    }
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let password = v
        .get("password")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match st.vault.init(&st.mem_dir, password) {
        Ok(s) => {
            // `.cjs` retorna `{ initialized: true, expires_at }` (2 campos) —
            // paridade byte-a-byte exige shape enxuto aqui, apesar do
            // adapter Rust devolver `VaultStatus` completo.
            let mut out = Map::new();
            out.insert("initialized".into(), Value::Bool(s.initialized));
            out.insert(
                "expires_at".into(),
                s.expires_at.map(Value::from).unwrap_or(Value::Null),
            );
            json_response(StatusCode::OK, Value::Object(out))
        }
        Err(e) => json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

async fn unlock(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(r) = guard(&st, &headers) {
        return r;
    }
    let now_ms = st.clock.now_ms();
    match st.unlock_guard.check(now_ms, &st.mem_dir) {
        RateLimit::Allowed => {}
        RateLimit::Blocked { retry_after_s } => {
            let mut resp = json_response(
                StatusCode::TOO_MANY_REQUESTS,
                json!({
                    "error": format!("muitas tentativas — aguarde {retry_after_s}s"),
                    "retry_after": retry_after_s,
                }),
            );
            resp.headers_mut().insert(
                "retry-after",
                HeaderValue::from_str(&retry_after_s.to_string()).unwrap(),
            );
            return resp;
        }
    }
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let password = v
        .get("password")
        .and_then(Value::as_str)
        .unwrap_or_default();
    // `Math.max(1, Math.min(240, parseInt(body.ttl_min, 10) || 30))`
    let ttl_min = v
        .get("ttl_min")
        .and_then(|n| n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)))
        .filter(|n| *n > 0)
        .unwrap_or(30)
        .clamp(1, 240);
    let ttl_ms = ttl_min * 60 * 1000;
    match st.vault.unlock(&st.mem_dir, password, ttl_ms) {
        Ok(s) => {
            st.unlock_guard.reset(&st.mem_dir);
            json_response(StatusCode::OK, vault_status_to_json(&s))
        }
        Err(e) => {
            st.unlock_guard.record_failure(now_ms, &st.mem_dir);
            json_error(StatusCode::UNAUTHORIZED, &e.to_string())
        }
    }
}

async fn lock(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(r) = guard(&st, &headers) {
        return r;
    }
    st.vault.lock();
    json_response(StatusCode::OK, json!({ "ok": true }))
}

async fn change_password(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(r) = guard(&st, &headers) {
        return r;
    }
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let old_pw = v
        .get("old_password")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let new_pw = v
        .get("new_password")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match st.vault.change_password(&st.mem_dir, old_pw, new_pw) {
        Ok(()) => json_response(StatusCode::OK, json!({ "ok": true })),
        Err(e) => vault_error_response(&e),
    }
}

async fn keys(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(r) = guard(&st, &headers) {
        return r;
    }
    match st.vault.list_keys(&st.mem_dir) {
        Ok(keys) => json_response(
            StatusCode::OK,
            json!({
                "keys": keys,
            }),
        ),
        Err(e) => json_error(StatusCode::LOCKED, &e.to_string()),
    }
}

async fn secret_get(
    State(st): State<AppState>,
    headers: HeaderMap,
    AxumPath(name): AxumPath<String>,
) -> Response {
    if let Some(r) = guard(&st, &headers) {
        return r;
    }
    match st.vault.get_secret(&st.mem_dir, &name) {
        Ok(Some(value)) => json_response(StatusCode::OK, json!({ "name": name, "value": value })),
        Ok(None) => json_error(StatusCode::NOT_FOUND, "secret not found"),
        Err(e) => json_error(StatusCode::LOCKED, &e.to_string()),
    }
}

async fn secret_put(
    State(st): State<AppState>,
    headers: HeaderMap,
    AxumPath(name): AxumPath<String>,
    body: Bytes,
) -> Response {
    if let Some(r) = guard(&st, &headers) {
        return r;
    }
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let value = v.get("value").and_then(Value::as_str).unwrap_or_default();
    match st.vault.set_secret(&st.mem_dir, &name, value) {
        Ok(()) => json_response(StatusCode::OK, json!({ "ok": true, "name": name })),
        Err(e) => vault_error_response(&e),
    }
}

async fn secret_delete(
    State(st): State<AppState>,
    headers: HeaderMap,
    AxumPath(name): AxumPath<String>,
) -> Response {
    if let Some(r) = guard(&st, &headers) {
        return r;
    }
    match st.vault.delete_secret(&st.mem_dir, &name) {
        Ok(ok) => json_response(StatusCode::OK, json!({ "ok": ok, "name": name })),
        Err(e) => json_error(StatusCode::LOCKED, &e.to_string()),
    }
}

async fn envfiles(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(r) = guard(&st, &headers) {
        return r;
    }
    match st.vault.list_envfiles(&st.mem_dir) {
        Ok(entries) => {
            let arr: Vec<Value> = entries
                .into_iter()
                .map(|e| {
                    json!({
                        "name": e.name,
                        "updated_at": e.updated_at,
                        "size": e.size,
                    })
                })
                .collect();
            json_response(StatusCode::OK, json!({ "envfiles": arr }))
        }
        Err(e) => json_error(StatusCode::LOCKED, &e.to_string()),
    }
}

async fn envfile_get(
    State(st): State<AppState>,
    headers: HeaderMap,
    AxumPath(name): AxumPath<String>,
) -> Response {
    if let Some(r) = guard(&st, &headers) {
        return r;
    }
    match st.vault.get_envfile(&st.mem_dir, &name) {
        Ok(Some(content)) => {
            json_response(StatusCode::OK, json!({ "name": name, "content": content }))
        }
        Ok(None) => json_error(StatusCode::NOT_FOUND, "envfile not found"),
        Err(e) => json_error(StatusCode::LOCKED, &e.to_string()),
    }
}

async fn envfile_put(
    State(st): State<AppState>,
    headers: HeaderMap,
    AxumPath(name): AxumPath<String>,
    body: Bytes,
) -> Response {
    if let Some(r) = guard(&st, &headers) {
        return r;
    }
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let content = v.get("content").and_then(Value::as_str).unwrap_or_default();
    match st.vault.set_envfile(&st.mem_dir, &name, content) {
        Ok(()) => json_response(StatusCode::OK, json!({ "ok": true, "name": name })),
        Err(e) => vault_error_response(&e),
    }
}

async fn envfile_delete(
    State(st): State<AppState>,
    headers: HeaderMap,
    AxumPath(name): AxumPath<String>,
) -> Response {
    if let Some(r) = guard(&st, &headers) {
        return r;
    }
    match st.vault.delete_envfile(&st.mem_dir, &name) {
        Ok(ok) => json_response(StatusCode::OK, json!({ "ok": ok, "name": name })),
        Err(e) => json_error(StatusCode::LOCKED, &e.to_string()),
    }
}

async fn export(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(r) = guard(&st, &headers) {
        return r;
    }
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    // is_unlocked check — o adapter já rejeita via `require_ui_unlock()`
    // internamente, mas `.cjs` tem um check explícito antes. Para paridade
    // byte-a-byte do erro, checa aqui. `status()` devolve locked=true
    // quando UI não desbloqueou.
    if let Ok(s) = st.vault.status(&st.mem_dir) {
        if s.locked {
            return json_error(StatusCode::LOCKED, "vault: UI locked");
        }
    }
    let master = v
        .get("master_password")
        .and_then(Value::as_str)
        .or_else(|| v.get("password").and_then(Value::as_str))
        .unwrap_or_default();
    let bundle_pw = v.get("bundle_password").and_then(Value::as_str);
    match st.vault.export_bundle(&st.mem_dir, master, bundle_pw) {
        Ok(bundle) => {
            let fname = format!(
                "vault-{}.pmvexp",
                &st.clock.now_iso()[..10.min(st.clock.now_iso().len())]
            );
            let mut headers = HeaderMap::new();
            headers.insert(
                "content-type",
                HeaderValue::from_static("application/octet-stream"),
            );
            headers.insert(
                "content-disposition",
                HeaderValue::from_str(&format!("attachment; filename=\"{fname}\"")).unwrap(),
            );
            headers.insert(
                "content-length",
                HeaderValue::from_str(&bundle.len().to_string()).unwrap(),
            );
            headers.insert("access-control-allow-origin", HeaderValue::from_static("*"));
            (StatusCode::OK, headers, bundle).into_response()
        }
        Err(e) => vault_error_response(&e),
    }
}

async fn import(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(r) = guard(&st, &headers) {
        return r;
    }
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let bundle_b64 = v
        .get("bundle_b64")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let bundle_pw = v
        .get("bundle_password")
        .and_then(Value::as_str)
        .or_else(|| v.get("password").and_then(Value::as_str))
        .unwrap_or_default();
    let new_master = v
        .get("new_master_password")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let replace = v.get("replace").and_then(Value::as_bool).unwrap_or(false);
    if bundle_b64.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "bundle_b64 obrigatório");
    }
    if new_master.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "new_master_password obrigatório");
    }
    let bundle = match base64::engine::general_purpose::STANDARD.decode(bundle_b64) {
        Ok(b) => b,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    match st
        .vault
        .import_bundle(&st.mem_dir, &bundle, bundle_pw, replace, new_master)
    {
        Ok(()) => {
            st.unlock_guard.reset(&st.mem_dir);
            json_response(StatusCode::OK, json!({ "ok": true }))
        }
        Err(e) => vault_error_response(&e),
    }
}

async fn destroy(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(r) = guard(&st, &headers) {
        return r;
    }
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    if v.get("confirm").and_then(Value::as_str) != Some("DESTROY") {
        return json_error(StatusCode::BAD_REQUEST, "need {confirm: \"DESTROY\"}");
    }
    // `.cjs` exige UI desbloqueada.
    if let Ok(s) = st.vault.status(&st.mem_dir) {
        if s.locked {
            return json_error(StatusCode::LOCKED, "destrua só com UI desbloqueada");
        }
    }
    match st.vault.destroy(&st.mem_dir) {
        Ok(()) => json_response(StatusCode::OK, json!({ "ok": true })),
        Err(e) => json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}
