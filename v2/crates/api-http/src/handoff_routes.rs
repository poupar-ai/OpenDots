//! Rotas `/api/agents/handoff` — paridade com `server.cjs:1388-1572` (`handleHandoff`).
//!
//! Fluxo:
//! 1. Valida body (`{from, to, message}` + intent opcional).
//! 2. Checa `from`/`to` no manifest (404 se ausente; sugere targets se `to` not found).
//! 3. Checa `can_talk_to` de `from` inclui `to` (403 + log violation + sugestões).
//! 4. `destructive.detect(message)` — se matched, bloqueia (403) + escala pro
//!    `internal_firefighter`.
//! 5. Resolve `group_id` se é reply a trace anterior.
//! 6. `appendChat(to, ...)` + `appendAgentLog` para ambos agentes.
//! 7. `bus.emit` (`created` / `acked` / `reported` se `reply_to`+`ack|report`).
//! 8. Se `report` de originador distinto → `enqueueAutoTurn(originator, from, ...)`.
//! 9. `enqueueAutoTurn(to, from, message, trace)`.
//! 10. `tryAutoEscalateBlocker` — padrões de bloqueio escalam pro bombeiro.
//! 11. Retorna `{ok, ts, to, intent, trace_id, auto_queued, auto_resumed, auto_escalated}`.

use std::path::Path;
use std::sync::LazyLock;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::post;
use axum::Router;
use regex::Regex;
use serde_json::{json, Map, Value};

use domain_bus::SystemClock as BusClock;
use domain_graph::{Clock as GraphClock, SystemClock as GraphSystemClock};

use crate::agents_routes::append_agent_log;
use crate::chat_routes::{append_chat, ensure_agent_channels};
use crate::{json_error, json_response, AppState};

pub fn handoff_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/agents/handoff",
            post(handoff_handler).delete(delete_not_allowed),
        )
        .route("/api/agents/handoff_batch", post(handoff_batch_handler))
}

/// Paridade com `server.cjs:2977-2980` — DELETE em `/api/agents/handoff`
/// retorna 405 com body específico (em vez do 405 default do framework).
async fn delete_not_allowed() -> Response {
    json_error(StatusCode::METHOD_NOT_ALLOWED, "method not allowed")
}

const VALID_INTENTS: &[&str] = &["ask", "delegate", "report", "ack", "escalate"];

fn now_iso() -> String {
    GraphSystemClock.now_iso()
}

/// Paridade com `newTraceId` em `server.cjs:29-31`. Gera `t_<base36 ts>_<rand>`.
fn new_trace_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let ts36 = base36_u64(ts_ms);
    // 6 chars "pseudo-aleatório" — para paridade semântica (não byte-exato)
    // usamos ns do relógio como seed.
    let extra_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let rand6 = {
        let mut v = extra_ns;
        let mut s = String::with_capacity(6);
        for _ in 0..6 {
            let d = (v % 36) as u32;
            v /= 36;
            s.push(std::char::from_digit(d, 36).unwrap_or('0'));
        }
        s
    };
    format!("t_{ts36}{rand6}")
}

fn base36_u64(mut v: u64) -> String {
    if v == 0 {
        return "0".into();
    }
    let mut out = Vec::<u8>::new();
    while v > 0 {
        let d = (v % 36) as u32;
        v /= 36;
        out.push(std::char::from_digit(d, 36).unwrap() as u8);
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

// ── Blocker detector ────────────────────────────────────────────────────────
// Paridade com `BLOCKER_SIGNALS` em `server.cjs:631-645`. 13 regexes.

static BLOCKER_SIGNALS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?i)\bbloquead[oa]s?\b",
        r"(?i)\bbloqueio\b",
        r"(?i)\bn[aã]o\s+conseg(ui|ue|uimos|uiram|uir)",
        r"(?i)\bn[aã]o\s+tenho\s+(como|permiss[aã]o|autoriza[çc][aã]o|acesso)",
        r"(?i)\bpermiss[aã]o\s+negada\b",
        r"(?i)\bpermission\s+denied\b",
        r"(?i)\bfalha\s+de\s+(permiss[aã]o|ambiente|autoriza[çc][aã]o)",
        r"(?i)\bpreciso\s+(de\s+)?(autoriza[çc][aã]o|aprova[çc][aã]o)\b",
        r"(?i)\bambiente\s+quebrado\b",
        r"(?i)\bferramenta\s+n[aã]o\s+responde\b",
        r"(?i)\bcanal\s+indispon[ií]vel\b",
        r"(?i)\btask\s+travad[ao]\b",
        r"(?i)\btravou\b",
    ]
    .iter()
    .filter_map(|p| Regex::new(p).ok())
    .collect()
});

struct BlockerResult {
    matched: bool,
    signals: Vec<String>,
}

fn detect_blocker(message: &str) -> BlockerResult {
    if message.is_empty() {
        return BlockerResult {
            matched: false,
            signals: Vec::new(),
        };
    }
    let mut hits: Vec<String> = Vec::new();
    for re in BLOCKER_SIGNALS.iter() {
        if let Some(m) = re.find(message) {
            let s = m.as_str().to_string();
            if !hits.contains(&s) {
                hits.push(s);
            }
        }
    }
    BlockerResult {
        matched: !hits.is_empty(),
        signals: hits,
    }
}

/// Paridade com `resolveGroupIdForReply` em `server.cjs:605-623`.
/// Varre os últimos entries dos canais de `to_agent` e `from_agent` buscando
/// `trace_id == reply_to`. Usa `group_id` ou `trace_id` encontrado; fallback
/// `reply_to`.
fn resolve_group_id_for_reply(
    mem_dir: &Path,
    reply_to: &str,
    to_agent: &str,
    from_agent: &str,
) -> String {
    for ag in [to_agent, from_agent] {
        if ag.is_empty() {
            continue;
        }
        let p = mem_dir.join("agent_chats").join(format!("{ag}.jsonl"));
        let Ok(raw) = std::fs::read_to_string(&p) else {
            continue;
        };
        let lines: Vec<&str> = raw.split('\n').filter(|l| !l.is_empty()).collect();
        for line in lines.iter().rev() {
            let Ok(entry) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if entry.get("trace_id").and_then(Value::as_str) == Some(reply_to) {
                if let Some(g) = entry.get("group_id").and_then(Value::as_str) {
                    return g.to_string();
                }
                if let Some(t) = entry.get("trace_id").and_then(Value::as_str) {
                    return t.to_string();
                }
                return reply_to.to_string();
            }
        }
    }
    reply_to.to_string()
}

/// Escala pro `internal_firefighter` se o message tem sinais de bloqueio.
/// Paridade com `tryAutoEscalateBlocker` em `server.cjs:657-697`.
/// Retorna `Some((ts, signals))` se escalou, `None` senão.
fn try_auto_escalate_blocker(
    state: &AppState,
    manifest: &Value,
    from_agent: &str,
    to_agent: &str,
    message: &str,
    original_ts: &str,
) -> Option<(String, Vec<String>)> {
    if to_agent == "internal_firefighter" || from_agent == "internal_firefighter" {
        return None;
    }
    let ff_exists = manifest
        .get("agents")
        .and_then(|a| a.get("internal_firefighter"))
        .is_some();
    if !ff_exists {
        return None;
    }
    let det = detect_blocker(message);
    if !det.matched {
        return None;
    }
    let ts = now_iso();
    let composed = format!(
        "[auto-escalação] {from_agent} reportou possível bloqueio pra {to_agent}.\n\
         Sinais detectados: {signals}\n\n\
         Mensagem original ({orig_ts}):\n{message}",
        signals = det.signals.join(", "),
        orig_ts = if original_ts.is_empty() { ts.as_str() } else { original_ts }
    );
    append_chat(
        &state.mem_dir,
        "internal_firefighter",
        &json!({
            "ts": ts,
            "role": "user",
            "content": composed,
            "from": from_agent,
            "auto_escalation": true,
            "original_to": to_agent,
            "signals": det.signals.clone(),
        }),
    );
    append_agent_log(
        &state.mem_dir,
        from_agent,
        &json!({
            "ts": ts,
            "type": "auto_escalation_out",
            "target": "internal_firefighter",
            "original_to": to_agent,
            "signals": det.signals.clone(),
        }),
    );
    append_agent_log(
        &state.mem_dir,
        "internal_firefighter",
        &json!({
            "ts": ts,
            "type": "auto_escalation_in",
            "source": from_agent,
            "original_to": to_agent,
            "signals": det.signals.clone(),
            "chars": message.len(),
        }),
    );
    // Auto-escalation pra bombeiro = High priority (cobre blocker detectado).
    let _ = state.auto_queue.enqueue_with(
        "internal_firefighter",
        from_agent,
        &composed,
        None,
        core_queue::Priority::High,
    );
    Some((ts, det.signals))
}

// ── Handler principal ───────────────────────────────────────────────────────

async fn handoff_handler(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };

    let body_from = v.get("from").and_then(Value::as_str).unwrap_or("");
    let to = v.get("to").and_then(Value::as_str).unwrap_or("");
    let message_opt = v.get("message").and_then(Value::as_str);
    if body_from.is_empty() || to.is_empty() || message_opt.is_none() {
        return json_error(StatusCode::BAD_REQUEST, "need {from, to, message}");
    }
    let message = message_opt.unwrap();

    // Fase C — JWT EdDSA OBRIGATÓRIO. Sem header = 401, sem fallback.
    // Segurança não pode depender do prompt do agente lembrar de
    // adicionar o Bearer: confiança em prompt é brecha. O server
    // garante; quem não consegue assinar (vault locked, agente legacy
    // pré-keypair, atacante externo) não consegue chamar handoff.
    //
    // Body.from é IGNORADO — `from` autoritativo vem do claim. Mesmo
    // assim validamos consistência: se body declara X mas token assina
    // como Y, é tentativa de impersonação ou agente confuso → 403.
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    let Some(token) = bearer else {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "Authorization: Bearer <JWT> obrigatório — agente sem keypair? recrie com vault destrancado.",
        );
    };
    let mem_dir_for_lookup = st.mem_dir.clone();
    let claims = match crate::agent_keys::verify_jwt(token, |name| {
        crate::agent_keys::lookup_pubkey(&mem_dir_for_lookup, name)
    }) {
        Ok(c) => c,
        Err(e) => {
            return json_error(StatusCode::UNAUTHORIZED, &format!("JWT inválido: {e}"));
        }
    };
    if !body_from.is_empty() && body_from != claims.agent {
        return json_error(
            StatusCode::FORBIDDEN,
            "body.from não bate com agent do token JWT (impersonação?)",
        );
    }
    let from_owned = claims.agent;
    let from = from_owned.as_str();

    // intent validation: if body.intent present and not in set → 400 (order
    // matches node: intent validated BEFORE computing default).
    if let Some(i) = v.get("intent").and_then(Value::as_str) {
        if !VALID_INTENTS.contains(&i) {
            return json_response(
                StatusCode::BAD_REQUEST,
                json!({
                    "error": "invalid intent",
                    "allowed": VALID_INTENTS,
                    "got": i,
                }),
            );
        }
    }
    let intent = v
        .get("intent")
        .and_then(Value::as_str)
        .filter(|s| VALID_INTENTS.contains(s))
        .unwrap_or("ask");
    let subject = v
        .get("subject")
        .and_then(Value::as_str)
        .map(|s| s.chars().take(120).collect::<String>());
    let trace_id = match v.get("trace_id").and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => new_trace_id(),
    };
    let reply_to = v
        .get("reply_to")
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let context_val = v
        .get("context")
        .filter(|c| c.is_object())
        .cloned();

    let manifest = match domain_agents::load_manifest(&st.mem_dir) {
        Some(m) => m,
        None => {
            return json_response(
                StatusCode::NOT_FOUND,
                json!({ "error": format!("from agent \"{from}\" not found") }),
            );
        }
    };

    let from_cfg = manifest
        .get("agents")
        .and_then(|a| a.get(from))
        .cloned();
    let to_cfg = manifest.get("agents").and_then(|a| a.get(to)).cloned();
    if from_cfg.is_none() {
        return json_response(
            StatusCode::NOT_FOUND,
            json!({ "error": format!("from agent \"{from}\" not found") }),
        );
    }
    if to_cfg.is_none() {
        let (file, cands) =
            domain_router::suggest_targets(&st.mem_dir, from, message, &domain_router::SystemClock);
        return json_response(
            StatusCode::NOT_FOUND,
            json!({
                "error": format!("to agent \"{to}\" not found"),
                "suggested_targets": cands,
                "file_detected": file,
            }),
        );
    }
    let from_cfg = from_cfg.unwrap();

    let can_talk: Vec<String> = from_cfg
        .get("can_talk_to")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if !can_talk.iter().any(|x| x == to) {
        let ts = now_iso();
        append_agent_log(
            &st.mem_dir,
            from,
            &json!({
                "ts": ts,
                "type": "violation",
                "stage": "handoff_denied",
                "target": to,
                "message": format!("{from} tentou handoff pra {to} — fora de can_talk_to"),
            }),
        );
        let (file, cands) =
            domain_router::suggest_targets(&st.mem_dir, from, message, &domain_router::SystemClock);
        return json_response(
            StatusCode::FORBIDDEN,
            json!({
                "error": "forbidden",
                "reason": format!("\"{to}\" não está em can_talk_to de \"{from}\""),
                "allowed": can_talk,
                "suggested_targets": cands,
                "file_detected": file,
            }),
        );
    }

    ensure_agent_channels(&st.mem_dir, &manifest);
    let ts = now_iso();

    // ── Motor destrutivo ────────────────────────────────────────────────────
    let det = domain_destructive::detect(&st.mem_dir, message);
    if det.matched {
        append_agent_log(
            &st.mem_dir,
            from,
            &json!({
                "ts": ts,
                "type": "violation",
                "stage": "destructive_intent_handoff",
                "target": to,
                "hits": det.hits.iter().map(|h| h.to_json()).collect::<Vec<_>>(),
                "message": format!(
                    "handoff bloqueado pelo motor: {}",
                    det.hits.iter().map(|h| h.id.clone()).collect::<Vec<_>>().join(", ")
                ),
            }),
        );
        let ff_exists = manifest
            .get("agents")
            .and_then(|a| a.get("internal_firefighter"))
            .is_some();
        if ff_exists && from != "internal_firefighter" && to != "internal_firefighter" {
            let composed = format!(
                "[motor-de-proteção] Handoff destrutivo bloqueado: {from} → {to}.\n\
                 Sinais: {signals}\n\n\
                 Mensagem original:\n{message}",
                signals = det
                    .hits
                    .iter()
                    .map(|h| h.id.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            append_chat(
                &st.mem_dir,
                "internal_firefighter",
                &json!({
                    "ts": ts,
                    "role": "user",
                    "content": composed,
                    "from": from,
                    "blocked_from": from,
                    "blocked_target": to,
                    "block_reason": "destructive_intent_handoff",
                    "block_hits": det.hits.iter().map(|h| h.to_json()).collect::<Vec<_>>(),
                }),
            );
            append_agent_log(
                &st.mem_dir,
                "internal_firefighter",
                &json!({
                    "ts": ts,
                    "type": "protection_escalation_in",
                    "source": from,
                    "original_target": to,
                    "hits": det.hits.iter().map(|h| h.to_json()).collect::<Vec<_>>(),
                }),
            );
            // Destrutivo bloqueado vai pro firefighter como High também.
            let _ = st.auto_queue.enqueue_with(
                "internal_firefighter",
                from,
                &composed,
                None,
                core_queue::Priority::High,
            );
        }
        return json_response(
            StatusCode::FORBIDDEN,
            json!({
                "error": "blocked",
                "reason": "destructive_intent",
                "hits": det.hits.iter().map(|h| h.to_json()).collect::<Vec<_>>(),
                "escalated_to": if ff_exists { Value::String("internal_firefighter".into()) } else { Value::Null },
            }),
        );
    }

    // ── Group id ────────────────────────────────────────────────────────────
    let group_id = match &reply_to {
        Some(rt) => resolve_group_id_for_reply(&st.mem_dir, rt, to, from),
        None => trace_id.clone(),
    };

    // ── appendChat + 2× appendAgentLog ──────────────────────────────────────
    let mut chat_entry = Map::new();
    chat_entry.insert("ts".into(), Value::String(ts.clone()));
    chat_entry.insert("role".into(), Value::String("user".into()));
    chat_entry.insert("content".into(), Value::String(message.to_string()));
    chat_entry.insert("from".into(), Value::String(from.to_string()));
    chat_entry.insert("intent".into(), Value::String(intent.to_string()));
    if let Some(s) = subject.clone() {
        chat_entry.insert("subject".into(), Value::String(s));
    }
    chat_entry.insert("trace_id".into(), Value::String(trace_id.clone()));
    if let Some(rt) = reply_to.clone() {
        chat_entry.insert("reply_to".into(), Value::String(rt));
    }
    chat_entry.insert("group_id".into(), Value::String(group_id.clone()));
    if let Some(ctx) = context_val.clone() {
        chat_entry.insert("context".into(), ctx);
    }
    append_chat(&st.mem_dir, to, &Value::Object(chat_entry));

    let mut log_out = Map::new();
    log_out.insert("ts".into(), Value::String(ts.clone()));
    log_out.insert("type".into(), Value::String("handoff_out".into()));
    log_out.insert("target".into(), Value::String(to.into()));
    log_out.insert("chars".into(), Value::Number(message.len().into()));
    log_out.insert("intent".into(), Value::String(intent.into()));
    log_out.insert(
        "subject".into(),
        subject
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    log_out.insert("trace_id".into(), Value::String(trace_id.clone()));
    log_out.insert(
        "reply_to".into(),
        reply_to
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    append_agent_log(&st.mem_dir, from, &Value::Object(log_out));

    let mut log_in = Map::new();
    log_in.insert("ts".into(), Value::String(ts.clone()));
    log_in.insert("type".into(), Value::String("handoff_in".into()));
    log_in.insert("source".into(), Value::String(from.into()));
    log_in.insert("chars".into(), Value::Number(message.len().into()));
    log_in.insert("intent".into(), Value::String(intent.into()));
    log_in.insert(
        "subject".into(),
        subject.clone().map(Value::String).unwrap_or(Value::Null),
    );
    log_in.insert("trace_id".into(), Value::String(trace_id.clone()));
    log_in.insert(
        "reply_to".into(),
        reply_to
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    append_agent_log(&st.mem_dir, to, &Value::Object(log_in));

    // ── Bus: created ────────────────────────────────────────────────────────
    let subject_val = subject
        .clone()
        .map(Value::String)
        .unwrap_or(Value::Null);
    let reply_to_val = reply_to
        .clone()
        .map(Value::String)
        .unwrap_or(Value::Null);
    let mut created = Map::new();
    created.insert("type".into(), Value::String("created".into()));
    created.insert("trace_id".into(), Value::String(trace_id.clone()));
    created.insert("from".into(), Value::String(from.into()));
    created.insert("to".into(), Value::String(to.into()));
    created.insert("intent".into(), Value::String(intent.into()));
    created.insert("subject".into(), subject_val);
    created.insert("reply_to".into(), reply_to_val);
    created.insert("message_chars".into(), Value::Number(message.len().into()));
    if let Some(ev) = domain_bus::emit(&st.mem_dir, &Value::Object(created), &BusClock) {
        st.trace_index.record(&ev);
    }

    // ── Bus: acked / reported + resumo ──────────────────────────────────────
    let mut resumed: Option<Value> = None;
    if let Some(rt) = &reply_to {
        if intent == "ack" || intent == "report" {
            let ev_type = if intent == "ack" { "acked" } else { "reported" };
            if let Some(ev) = domain_bus::emit(
                &st.mem_dir,
                &json!({
                    "type": ev_type,
                    "trace_id": rt,
                    "by": from,
                    "via_trace": trace_id,
                }),
                &BusClock,
            ) {
                st.trace_index.record(&ev);
            }
            if intent == "report" {
                // Tenta o índice em memória primeiro; fallback pro disk scan
                // (cobre traces fora da janela populada no startup).
                let orig_opt = st.trace_index.get_trace(rt)
                    .or_else(|| domain_bus::get_message(&st.mem_dir, rt, &BusClock));
                if let Some(orig) = orig_opt {
                    if let Some(originator) = orig
                        .created
                        .as_ref()
                        .and_then(|c| c.get("from"))
                        .and_then(Value::as_str)
                    {
                        if originator != from && originator != to {
                            let resume_msg = format!(
                                "[report de {from} sobre handoff anterior pra {to}]\n\n{message}"
                            );
                            let _ = st.auto_queue.enqueue(
                                originator,
                                from,
                                &resume_msg,
                                Some(&trace_id),
                            );
                            resumed = Some(json!({
                                "to": originator,
                                "from": from,
                                "original_trace": rt,
                            }));
                        }
                    }
                }
            }
        }
    }

    // ── Enqueue auto-turn normal ────────────────────────────────────────────
    // Prioridade vem do body opcionalmente (`"priority": "high|normal|low"`).
    // Default Normal. Auto-escalation de bombeiro usa High (vide
    // tryAutoEscalateBlocker logo abaixo, que chama enqueue diretamente).
    let priority = v
        .get("priority")
        .and_then(Value::as_str)
        .map(core_queue::Priority::parse)
        .unwrap_or(core_queue::Priority::Normal);
    // Backpressure opt-in: body.wait_ack=true ou wait_ack_timeout_ms
    // (default 30s). Server registra oneshot pelo trace_id antes do
    // enqueue, espera o hook on_drain_start sinalizar (= B começou),
    // ou timeout. Útil pra flows "tipo RPC" onde A precisa saber que
    // B engatou antes de seguir.
    let wait_ack = v.get("wait_ack").and_then(Value::as_bool).unwrap_or(false);
    let wait_timeout_ms = v
        .get("wait_ack_timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(30_000);
    let ack_rx: Option<tokio::sync::oneshot::Receiver<()>> = if wait_ack {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        st.pending_acks.lock().unwrap().insert(trace_id.clone(), tx);
        Some(rx)
    } else {
        None
    };
    let _ = st.auto_queue.enqueue_with(to, from, message, Some(&trace_id), priority);

    // ── Auto-escalação de blocker ───────────────────────────────────────────
    let escalation = try_auto_escalate_blocker(&st, &manifest, from, to, message, &ts);

    // ── Aguarda ack se wait_ack=true ────────────────────────────────────────
    let ack_status: Value = if let Some(rx) = ack_rx {
        let timeout = std::time::Duration::from_millis(wait_timeout_ms);
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(())) => Value::String("acked".into()),
            Ok(Err(_)) => {
                // Sender foi dropado sem enviar (caso raro: queue config
                // mudou ou panic no hook). Trace continua na pending.
                st.pending_acks.lock().unwrap().remove(&trace_id);
                Value::String("sender_dropped".into())
            }
            Err(_) => {
                // Timeout — limpa o sender pra não vazar memory.
                st.pending_acks.lock().unwrap().remove(&trace_id);
                Value::String("timeout".into())
            }
        }
    } else {
        Value::Null
    };

    // ── Resposta ────────────────────────────────────────────────────────────
    let mut out = Map::new();
    out.insert("ok".into(), Value::Bool(true));
    out.insert("ts".into(), Value::String(ts));
    out.insert("to".into(), Value::String(to.into()));
    out.insert("intent".into(), Value::String(intent.into()));
    out.insert("trace_id".into(), Value::String(trace_id));
    out.insert("auto_queued".into(), Value::Bool(true));
    out.insert("ack".into(), ack_status);
    out.insert("auto_resumed".into(), resumed.unwrap_or(Value::Null));
    out.insert(
        "auto_escalated".into(),
        match escalation {
            Some((_, signals)) => json!({
                "to": "internal_firefighter",
                "signals": signals,
            }),
            None => Value::Null,
        },
    );
    json_response(StatusCode::OK, Value::Object(out))
}

// ── Batch handler ───────────────────────────────────────────────────────────
//
// Fast path pra alta carga: 1 request, N handoffs. Cliente é responsável
// por agrupar. Cada item passa pela mesma validação que o single handler
// (JWT autoritativo, can_talk_to, destructive scan), mas SEM features
// sofisticadas — sem auto-escalation, sem ack wait, sem report-intent
// resolution. Quem precisa disso usa o handler single.
//
// Body:
//   {
//     "items": [
//       {"to": "oracle", "message": "...", "priority"?: "high|normal|low",
//        "intent"?: "ask|delegate|...", "subject"?: "...", "trace_id"?: "..."},
//       ...
//     ]
//   }
// Response: 200 com {"items": [{"index": 0, "ok": true, "trace_id": "..."}, ...]}.
// Sempre 200 — erros individuais ficam em items[i].error. Único 4xx é
// se JWT/JSON do body é inválido (rejeita o request inteiro).

async fn handoff_batch_handler(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let items = match v.get("items").and_then(Value::as_array) {
        Some(arr) if !arr.is_empty() => arr.clone(),
        _ => return json_error(StatusCode::BAD_REQUEST, "items (array não-vazio) obrigatório"),
    };
    if items.len() > 1000 {
        return json_error(StatusCode::BAD_REQUEST, "max 1000 items por batch");
    }

    // JWT validado UMA vez. `from` autoritativo aplica a todos os items
    // (batch é assinado por UM agente — não dá pra um agente fingir batch
    // de outros).
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    let Some(token) = bearer else {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "Authorization: Bearer <JWT> obrigatório",
        );
    };
    let mem_dir_for_lookup = st.mem_dir.clone();
    let claims = match crate::agent_keys::verify_jwt(token, |name| {
        crate::agent_keys::lookup_pubkey(&mem_dir_for_lookup, name)
    }) {
        Ok(c) => c,
        Err(e) => return json_error(StatusCode::UNAUTHORIZED, &format!("JWT inválido: {e}")),
    };
    let from = claims.agent;

    let manifest = match domain_agents::load_manifest(&st.mem_dir) {
        Some(m) => m,
        None => return json_error(StatusCode::NOT_FOUND, "manifest não encontrado"),
    };
    let from_cfg = match manifest
        .get("agents")
        .and_then(|a| a.get(&from))
        .cloned()
    {
        Some(c) => c,
        None => return json_error(StatusCode::NOT_FOUND, &format!("from '{from}' não existe no manifest")),
    };
    let can_talk: Vec<String> = from_cfg
        .get("can_talk_to")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    ensure_agent_channels(&st.mem_dir, &manifest);

    let mut results: Vec<Value> = Vec::with_capacity(items.len());
    let mut ok_count: usize = 0;

    for (idx, item) in items.iter().enumerate() {
        let to = item.get("to").and_then(Value::as_str).unwrap_or("");
        let message = item.get("message").and_then(Value::as_str).unwrap_or("");
        if to.is_empty() || message.is_empty() {
            results.push(json!({"index": idx, "ok": false, "error": "to e message obrigatórios"}));
            continue;
        }
        // Existe?
        let to_exists = manifest
            .get("agents")
            .and_then(|a| a.get(to))
            .is_some();
        if !to_exists {
            results.push(json!({"index": idx, "ok": false, "error": format!("to '{to}' não existe")}));
            continue;
        }
        // can_talk_to?
        if !can_talk.iter().any(|x| x == to) {
            append_agent_log(
                &st.mem_dir,
                &from,
                &json!({
                    "ts": now_iso(),
                    "type": "violation",
                    "stage": "handoff_batch_denied",
                    "target": to,
                    "batch_index": idx,
                }),
            );
            results.push(json!({"index": idx, "ok": false, "error": format!("'{to}' não está em can_talk_to de '{from}'")}));
            continue;
        }
        // Destrutivo?
        let det = domain_destructive::detect(&st.mem_dir, message);
        if det.matched {
            results.push(json!({
                "index": idx,
                "ok": false,
                "error": "destructive_intent",
                "hits": det.hits.iter().map(|h| h.to_json()).collect::<Vec<_>>(),
            }));
            continue;
        }

        // Validate intent + priority + trace.
        let intent = item
            .get("intent")
            .and_then(Value::as_str)
            .filter(|s| VALID_INTENTS.contains(s))
            .unwrap_or("ask");
        let priority = item
            .get("priority")
            .and_then(Value::as_str)
            .map(core_queue::Priority::parse)
            .unwrap_or(core_queue::Priority::Normal);
        let trace_id = match item.get("trace_id").and_then(Value::as_str) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => new_trace_id(),
        };
        let subject = item
            .get("subject")
            .and_then(Value::as_str)
            .map(|s| s.chars().take(120).collect::<String>());

        let ts = now_iso();
        // Append chat + 2 logs + emit + enqueue — inline pra evitar
        // overhead de função.
        let mut chat_entry = Map::new();
        chat_entry.insert("ts".into(), Value::String(ts.clone()));
        chat_entry.insert("role".into(), Value::String("user".into()));
        chat_entry.insert("content".into(), Value::String(message.to_string()));
        chat_entry.insert("from".into(), Value::String(from.clone()));
        chat_entry.insert("intent".into(), Value::String(intent.into()));
        chat_entry.insert("trace_id".into(), Value::String(trace_id.clone()));
        if let Some(ref s) = subject {
            chat_entry.insert("subject".into(), Value::String(s.clone()));
        }
        append_chat(&st.mem_dir, to, &Value::Object(chat_entry));

        append_agent_log(
            &st.mem_dir,
            &from,
            &json!({
                "ts": ts,
                "type": "handoff_out",
                "target": to,
                "chars": message.len(),
                "intent": intent,
                "trace_id": trace_id,
                "batch": true,
            }),
        );
        append_agent_log(
            &st.mem_dir,
            to,
            &json!({
                "ts": ts,
                "type": "handoff_in",
                "source": from,
                "chars": message.len(),
                "intent": intent,
                "trace_id": trace_id,
                "batch": true,
            }),
        );

        if let Some(ev) = domain_bus::emit(
            &st.mem_dir,
            &json!({
                "type": "created",
                "trace_id": trace_id,
                "from": from,
                "to": to,
                "intent": intent,
                "message_chars": message.len(),
                "batch": true,
            }),
            &BusClock,
        ) {
            st.trace_index.record(&ev);
        }

        let _ = st.auto_queue.enqueue_with(to, &from, message, Some(&trace_id), priority);
        results.push(json!({
            "index": idx,
            "ok": true,
            "trace_id": trace_id,
            "to": to,
            "intent": intent,
        }));
        ok_count += 1;
    }

    json_response(
        StatusCode::OK,
        json!({
            "ok": ok_count == items.len(),
            "total": items.len(),
            "succeeded": ok_count,
            "failed": items.len() - ok_count,
            "items": results,
        }),
    )
}
