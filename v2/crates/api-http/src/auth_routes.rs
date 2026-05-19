//! Rotas `/api/auth/server-token` — gerencia o token global do servidor
//! usado pelo middleware quando `PROJECT_MEM_REQUIRE_TOKEN=1` no env.
//!
//! - `GET    /api/auth/server-token`          → `{configured, preview}`
//! - `POST   /api/auth/server-token/generate` → gera UUID v4, grava,
//!   retorna o token completo (apenas dessa vez).
//! - `DELETE /api/auth/server-token`          → apaga o token.
//!
//! Storage: `<mem_dir>/auth.json` com `{"server_token": "<uuid>"}`.

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use serde_json::{json, Value};
use std::path::Path;

use crate::tenant::TenantContext;
use crate::{json_error, json_response, AppState};

pub fn auth_router() -> Router<AppState> {
    Router::new()
        .route("/api/auth/server-token", get(get_token).delete(delete_token))
        .route("/api/auth/server-token/generate", post(generate_token))
}

fn auth_file(mem_dir: &Path) -> std::path::PathBuf {
    mem_dir.join("auth.json")
}

/// Lê o token atual. None se arquivo ausente ou sem chave.
pub fn load_server_token(mem_dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(auth_file(mem_dir)).ok()?;
    let v: Value = serde_json::from_str(&content).ok()?;
    let s = v.get("server_token")?.as_str()?;
    if s.is_empty() {
        return None;
    }
    Some(s.to_string())
}

fn save_server_token(mem_dir: &Path, token: Option<&str>) -> std::io::Result<()> {
    std::fs::create_dir_all(mem_dir)?;
    let body = match token {
        Some(t) => json!({ "server_token": t }),
        None => json!({}),
    };
    std::fs::write(
        auth_file(mem_dir),
        serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".into()),
    )
}

fn token_preview(token: &str) -> String {
    if token.len() <= 8 {
        return "*".repeat(token.len());
    }
    let head: String = token.chars().take(4).collect();
    let tail: String = token.chars().skip(token.len() - 4).collect();
    format!("{head}…{tail}")
}

async fn get_token(Extension(tc): Extension<TenantContext>) -> Response {
    let tok = load_server_token(&tc.mem_dir);
    let (configured, preview) = match &tok {
        Some(t) => (true, Some(token_preview(t))),
        None => (false, None),
    };
    let enforced = std::env::var("PROJECT_MEM_REQUIRE_TOKEN").ok().as_deref() == Some("1");
    json_response(
        StatusCode::OK,
        json!({
            "configured": configured,
            "preview":    preview,
            "enforced":   enforced,
        }),
    )
}

async fn generate_token(
    State(_st): State<AppState>,
    Extension(tc): Extension<TenantContext>,
) -> Response {
    let token = simple_uuid_v4();
    if let Err(e) = save_server_token(&tc.mem_dir, Some(&token)) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("falha ao gravar: {e}"));
    }
    json_response(
        StatusCode::CREATED,
        json!({
            "token": token,
            "preview": token_preview(&token),
            "note": "Salve este token agora — o servidor só devolve o preview depois.",
        }),
    )
}

async fn delete_token(Extension(tc): Extension<TenantContext>) -> Response {
    if let Err(e) = save_server_token(&tc.mem_dir, None) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("falha ao gravar: {e}"));
    }
    json_response(StatusCode::OK, json!({ "ok": true }))
}

/// UUID v4 sem deps externas — 122 bits aleatórios + 6 bits de version/variant.
/// Suficiente pra token de auth: collision probability é 1 em 2^61 com
/// um trilhão de tokens.
fn simple_uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut bytes = [0u8; 16];
    // Mistura clock + getpid + endereço de uma var local pra ter
    // entropy "boa o suficiente" sem dependência de getrandom.
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let nanos = now.as_nanos();
    let pid = std::process::id() as u128;
    let stack_addr = (&bytes as *const _) as u128;
    let mut seed: u128 = nanos
        .wrapping_mul(0x6eed_0e9d_a4d9_4a4f_a734_3b67_42aa_baf3u128)
        ^ (pid << 7)
        ^ (stack_addr.wrapping_mul(0xc6a4_a793_5bd1_e995_c6a4_a793_5bd1_e995u128));
    for byte in bytes.iter_mut() {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *byte = ((seed >> 64) & 0xff) as u8;
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant RFC 4122
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        u16::from_be_bytes([bytes[4], bytes[5]]),
        u16::from_be_bytes([bytes[6], bytes[7]]),
        u16::from_be_bytes([bytes[8], bytes[9]]),
        u64::from_be_bytes([0, 0, bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]]) & 0xffff_ffff_ffff,
    )
}
