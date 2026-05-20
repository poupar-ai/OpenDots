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
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use domain_humans::{
    append_human_chat, clear_human_chat, create_human, delete_human, get_human_summary,
    human_summary, list_humans, load_humans, read_human_chat, summarize_human_chats, touch_human,
    update_human, Clock as HumansClock, CreateInput, Error as HumansError,
    SystemClock as HumansSystemClock, UpdateInput, HUMAN_LOCATIONS,
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
        .route("/api/humans/:id/login", post(login_handler))
        .route("/api/humans/:id/chats", get(chats_summary_handler))
        .route("/api/human-chat/history", get(human_chat_history_handler))
        .route("/api/human-chat/clear", post(clear_chat_handler))
        .route("/api/human-handoff", post(handoff_handler))
}

/// Extrai e valida o Bearer JWT, retornando o id do humano autenticado
/// (subject do token). É a fonte autoritativa de "quem sou eu" — garante
/// que um humano só leia/limpe/escreva nas próprias conversas.
fn authed_human(
    tc: &crate::tenant::TenantContext,
    headers: &HeaderMap,
) -> Result<String, Response> {
    let bearer = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    let Some(token) = bearer else {
        return Err(json_error(
            StatusCode::UNAUTHORIZED,
            "Authorization: Bearer <JWT> obrigatório — faça login",
        ));
    };
    match crate::agent_keys::verify_jwt(token, |id| {
        crate::human_keys::lookup_human_pubkey(&tc.mem_dir, id)
    }) {
        Ok(c) => Ok(c.agent),
        Err(e) => Err(json_error(StatusCode::UNAUTHORIZED, &format!("token inválido: {e}"))),
    }
}

/// `POST /api/human-handoff` — envia mensagem de um humano pra outro.
/// Body: `{ from, to, message }`. Faz append no log do par e marca o
/// remetente como visto. (Shadow-turn de IA quando offline fica pra depois.)
/// `POST /api/humans/:id/login` — valida a senha (hash no cofre) e, se ok,
/// emite um JWT de sessão assinado com o keypair do humano. O cofre precisa
/// estar destrancado pra ler o hash.
async fn login_handler(
    State(st): State<AppState>,
    Extension(tc): Extension<crate::tenant::TenantContext>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let password = v.get("password").and_then(Value::as_str).unwrap_or("");
    if password.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "senha obrigatória");
    }
    let clock = HumansSystemClock;
    if get_human_summary(&tc.mem_dir, &id, clock.now_ms()).is_none() {
        return json_error(StatusCode::NOT_FOUND, "perfil não encontrado");
    }
    if !crate::human_keys::verify_human_password(&st.vault, &tc.mem_dir, &id, password) {
        return json_error(StatusCode::UNAUTHORIZED, "senha incorreta (ou cofre trancado)");
    }
    let _ = touch_human(&tc.mem_dir, &id, &clock);
    let token = crate::human_keys::mint_token_for_human(&tc.mem_dir, &id);
    json_response(StatusCode::OK, json!({ "token": token }))
}

/// `POST /api/human-handoff` — envia mensagem de um humano pra outro.
/// Exige `Authorization: Bearer <JWT>` (mesma segurança dos agentes): o
/// `from` é o subject autoritativo do token, validado por Ed25519. Body:
/// `{ from, to, message }` — `from` precisa bater com o token.
async fn handoff_handler(
    Extension(tc): Extension<crate::tenant::TenantContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let from = match authed_human(&tc, &headers) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let to = v.get("to").and_then(Value::as_str).filter(|s| !s.is_empty());
    let message = v.get("message").and_then(Value::as_str).filter(|s| !s.is_empty());
    let (Some(to), Some(message)) = (to, message) else {
        return json_error(StatusCode::BAD_REQUEST, "missing to/message");
    };
    // `from` autoritativo é o subject do token. Se o body declarar outro, é
    // tentativa de impersonação.
    if let Some(body_from) = v.get("from").and_then(Value::as_str) {
        if body_from != from {
            return json_error(StatusCode::FORBIDDEN, "from não bate com o token (impersonação?)");
        }
    }
    let from = from.as_str();

    let clock = HumansSystemClock;
    let entry = json!({
        "from": from,
        "to": to,
        "content": message,
        "ts": clock.now_iso(),
    });
    append_human_chat(&tc.mem_dir, from, to, &entry);
    let _ = touch_human(&tc.mem_dir, from, &clock);

    json_response(StatusCode::OK, json!({ "ok": true }))
}

async fn list_handler(State(st): State<AppState>) -> Response {
    let clock = HumansSystemClock;
    let list = list_humans(&st.mem_dir, clock.now_ms());
    json_response(StatusCode::OK, list)
}

async fn create_handler(
    State(st): State<AppState>,
    Extension(tc): Extension<crate::tenant::TenantContext>,
    body: Bytes,
) -> Response {
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

    // `location` é obrigatória: sem ela o humano fica fora de qualquer
    // departamento e o avatar não renderiza no canvas (não tem sala onde
    // ficar). Backend rejeita pra cobrir o caso de criação via API direta;
    // o form sempre manda uma das HUMAN_LOCATIONS pelo default.
    let location_str = v
        .get("location")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(location_str) = location_str else {
        return json_error(StatusCode::BAD_REQUEST, "location é obrigatória");
    };
    if !HUMAN_LOCATIONS.contains(&location_str) {
        return json_error(
            StatusCode::BAD_REQUEST,
            &format!("location inválida. Use: {}", HUMAN_LOCATIONS.join(", ")),
        );
    }

    // Senha obrigatória — o hash vai pro cofre (auth gate). Sem legado.
    let password = v
        .get("password")
        .and_then(Value::as_str)
        .filter(|s| s.len() >= 4);
    let Some(password) = password else {
        return json_error(StatusCode::BAD_REQUEST, "senha obrigatória (mín. 4 caracteres)");
    };

    let description = v.get("description").and_then(Value::as_str);
    let important = v.get("important").and_then(Value::as_str);
    let notes = v.get("notes").and_then(Value::as_str);
    let email = v.get("email").and_then(Value::as_str);

    let input = CreateInput {
        name,
        email,
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
    match create_human(&tc.mem_dir, &input, &clock) {
        Ok(h) => {
            let id = h.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            // Salva o hash da senha no cofre (requer cofre destrancado).
            if let Err(e) = crate::human_keys::set_human_password(&st.vault, &tc.mem_dir, &id, password) {
                return json_error(
                    StatusCode::CONFLICT,
                    &format!("perfil criado, mas falhou ao salvar senha (destranque o cofre): {e}"),
                );
            }
            // Provisiona keypair + emite token de sessão (identidade do handoff).
            let token = crate::human_keys::mint_token_for_human(&tc.mem_dir, &id);
            let summary = human_summary(&h, clock.now_ms());
            json_response(StatusCode::CREATED, json!({ "human": summary, "token": token }))
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
    // `me` agora vem do token (não do query) — mantido aqui só por compat de
    // clients antigos que ainda enviam o param; é ignorado.
    #[serde(default, rename = "me")]
    _me: Option<String>,
    #[serde(rename = "with")]
    with_id: Option<String>,
}

async fn human_chat_history_handler(
    Extension(tc): Extension<crate::tenant::TenantContext>,
    headers: HeaderMap,
    Query(q): Query<ChatHistoryQuery>,
) -> Response {
    // `me` é SEMPRE o subject do token — ignora qualquer `me` do query. Assim
    // um humano só lê conversas das quais ele participa (nunca de terceiros).
    let me = match authed_human(&tc, &headers) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let with_id = q.with_id.as_deref().filter(|s| !s.is_empty());
    let Some(with_id) = with_id else {
        return json_error(StatusCode::BAD_REQUEST, "need with");
    };
    let messages = read_human_chat(&tc.mem_dir, &me, with_id);
    json_response(
        StatusCode::OK,
        json!({
            "me": me,
            "with": with_id,
            "messages": messages,
        }),
    )
}

/// `POST /api/human-chat/clear` — limpa a conversa entre o humano autenticado
/// e `with`. NÃO apaga humano. Só participa quem está na conversa (me vem do
/// token), então ninguém limpa o chat de terceiros.
async fn clear_chat_handler(
    Extension(tc): Extension<crate::tenant::TenantContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let me = match authed_human(&tc, &headers) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let with = v.get("with").and_then(Value::as_str).filter(|s| !s.is_empty());
    let Some(with) = with else {
        return json_error(StatusCode::BAD_REQUEST, "need with");
    };
    let cleared = clear_human_chat(&tc.mem_dir, &me, with);
    json_response(StatusCode::OK, json!({ "ok": true, "cleared": cleared }))
}

