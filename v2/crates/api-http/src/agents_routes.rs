//! Rotas `/api/agents/*` + `/api/mcps` + `/api/docs/:doc.md` — paridade
//! com `server.cjs:2966-3025` (+ `3481-3517` para log/work).
//!
//! 12 rotas:
//! - `POST   /api/agents`                  — cria agente
//! - `DELETE /api/agents/:name`            — remove agente (bloqueia manager/oracle)
//! - `PATCH  /api/agents/:name/model`      — troca model (+ propagate_seed)
//! - `PATCH  /api/agents/:name/autonomy`   — troca autonomia (+ propagate_seed)
//! - `PUT    /api/agents/:name/scope`      — tools + scope.bash_allow
//! - `PUT    /api/agents/:name/vault`      — secrets (ACL)
//! - `PUT    /api/agents/:name/context`    — salva mini-contexto .md
//! - `PUT    /api/agents/:name/mcps`       — lista de mcps
//! - `GET    /api/mcps`                    — global+project+referenced
//! - `PUT    /api/docs/:doc.md`            — salva SOUL/PROJECT/… allowlist
//! - `GET    /api/agents/log?agent=X`      — tail do log do agente
//! - `GET    /api/agents/work?agent=X`     — observations filtradas

use std::fs;
use std::path::{Path, PathBuf};

use axum::body::Bytes;
use axum::extract::{Path as AxPath, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{delete, get, patch, post, put};
use axum::Router;
use domain_agents::{
    add_agent, load_manifest, remove_agent, save_manifest, save_mini_context, update_autonomy,
    update_model, update_scope, update_vault_access, AddAgentError, PropagateAutonomyError,
    PropagateModelError, UpdateAutonomyError, UpdateModelError, UpdateScopeError,
    UpdateVaultAccessError, AGENT_TYPES,
};
use domain_graph::{on_agent_log_event, Clock as GraphClock, SystemClock as GraphSystemClock};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{json_error, json_response, AppState};

pub fn agents_router() -> Router<AppState> {
    Router::new()
        .route("/api/agents", post(create_handler))
        .route("/api/agents/log", get(log_handler))
        .route("/api/agents/work", get(work_handler))
        .route("/api/agents/route", post(route_handler))
        .route("/api/agents/:name", delete(delete_handler))
        .route("/api/agents/:name/model", patch(model_handler))
        .route("/api/agents/:name/autonomy", patch(autonomy_handler))
        .route("/api/agents/:name/scope", put(scope_handler))
        .route("/api/agents/:name/vault", put(vault_handler))
        .route("/api/agents/:name/context", put(context_handler))
        .route("/api/agents/:name/mcps", put(mcps_handler))
        .route("/api/agents/:name/status", get(status_handler))
        .route("/api/agents/:name/rotate_keypair", post(rotate_keypair_handler))
        .route("/api/docs/:doc", put(doc_save_handler))
}

/// `GET /api/agents/:name/status` — usado pelo chat web pra mostrar
/// "pensando..." quando o usuário reabre o chat enquanto o agente ainda
/// está processando em background (tokio::spawn no chat_send_handler
/// sobrevive ao disconnect do SSE).
async fn status_handler(
    State(st): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Response {
    let processing = st.auto_queue.is_processing(&name);
    json_response(
        axum::http::StatusCode::OK,
        json!({ "agent": name, "processing": processing }),
    )
}

/// `POST /api/agents/:name/rotate_keypair` — gera novo par Ed25519,
/// grava priv no vault (sobrescreve secret existente), atualiza pubkey
/// no `agents.json`. Casos de uso:
///   1. Recuperar agentes legacy criados ANTES do fix de privkey_secret_name
///      (bug histórico: privkey ficou perdida porque nome do secret
///      falhava regex do vault).
///   2. Rotação periódica de chave (defesa em profundidade).
///
/// Requer vault unlocked. Retorna 412 se vault locked, 404 se agente
/// não existe, 500 se grava falha.
async fn rotate_keypair_handler(
    State(st): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Response {
    let Some(mut manifest) = domain_agents::load_manifest(&st.mem_dir) else {
        return json_error(StatusCode::NOT_FOUND, "agents.json não encontrado");
    };
    let agents_obj = match manifest
        .get_mut("agents")
        .and_then(Value::as_object_mut)
    {
        Some(o) => o,
        None => return json_error(StatusCode::NOT_FOUND, "manifest sem chave 'agents'"),
    };
    if !agents_obj.contains_key(&name) {
        return json_error(StatusCode::NOT_FOUND, &format!("agente '{name}' não existe"));
    }

    // Gera novo par + grava priv no vault PRIMEIRO. Se falhar, abortamos
    // sem mexer no manifest — agente continua com a pubkey antiga
    // (consistência: nunca ficar com pubkey nova + priv antiga, que
    // quebraria todos os JWTs subsequentes).
    let (priv_bytes, pub_bytes) = crate::agent_keys::generate_keypair();
    let secret_name = crate::agent_keys::privkey_secret_name(&name);
    if let Err(e) = st.vault.set_secret(
        &st.mem_dir,
        &secret_name,
        &crate::agent_keys::bytes_to_hex(&priv_bytes),
    ) {
        return json_error(
            StatusCode::PRECONDITION_FAILED,
            &format!(
                "falha ao gravar privkey no vault (vault destrancado? init?): {e}"
            ),
        );
    }

    // Atualiza pubkey no manifest + salva.
    let agent_cfg = agents_obj.get_mut(&name).and_then(Value::as_object_mut).unwrap();
    let new_pubkey_hex = crate::agent_keys::bytes_to_hex(&pub_bytes);
    agent_cfg.insert("pubkey".into(), Value::String(new_pubkey_hex.clone()));
    if let Err(e) = domain_agents::save_manifest(&st.mem_dir, &manifest) {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("priv gravada mas falha ao salvar manifest: {e}"),
        );
    }

    json_response(
        StatusCode::OK,
        json!({
            "ok": true,
            "agent": name,
            "pubkey": new_pubkey_hex,
            "note": "JWTs antigos do agente ficam inválidos imediatamente. Próximo spawn vai gerar token novo.",
        }),
    )
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn parse_json(body: &Bytes) -> Result<Value, Response> {
    serde_json::from_slice(body).map_err(|_| json_error(StatusCode::BAD_REQUEST, "invalid json"))
}

fn now_iso() -> String {
    GraphSystemClock.now_iso()
}

/// Paridade com `appendAgentLog` em `server.cjs:1099`.
/// Append JSONL + dispara `graph.onAgentLogEvent`.
pub(crate) fn append_agent_log(mem_dir: &Path, agent: &str, entry: &Value) {
    let dir = mem_dir.join("agent_logs");
    let _ = fs::create_dir_all(&dir);
    if let Ok(line) = serde_json::to_string(entry) {
        let path = dir.join(format!("{agent}.jsonl"));
        let mut content = line;
        content.push('\n');
        if let Ok(existing) = fs::read(&path) {
            let mut bytes = existing;
            bytes.extend_from_slice(content.as_bytes());
            let _ = fs::write(&path, bytes);
        } else {
            let _ = fs::write(&path, content);
        }
    }
    on_agent_log_event(mem_dir, agent, entry, &GraphSystemClock);
}

/// Paridade com `readAgentLog` em `server.cjs:1113`.
fn read_agent_log(mem_dir: &Path, agent: &str, limit: Option<usize>) -> Vec<Value> {
    let path = mem_dir.join("agent_logs").join(format!("{agent}.jsonl"));
    let Ok(raw) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let lines: Vec<&str> = raw.split('\n').filter(|l| !l.is_empty()).collect();
    let slice: &[&str] = match limit {
        Some(n) if n > 0 && n < lines.len() => &lines[lines.len() - n..],
        _ => &lines,
    };
    slice
        .iter()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

/// Paridade com `sanitizeStringArray` em `server.cjs:1792`.
fn sanitize_string_array(input: Option<&Value>, max: usize, max_len: usize) -> Vec<String> {
    let Some(arr) = input.and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.chars().count() <= max_len)
        .take(max)
        .collect()
}

/// Resolve `<plugin_dir>/seeds/<cat>/<slug>/agents.json`. Paridade com
/// `path.join(seed.SEEDS_DIR, cat, slug, 'agents.json')` do `.cjs`.
fn seed_agents_json(st: &AppState, cat: &str, slug: &str) -> PathBuf {
    st.plugin_dir
        .join("seeds")
        .join(cat)
        .join(slug)
        .join("agents.json")
}

/// Lê `_meta.agent_origin[name] = { category, slug }` do manifest.
fn agent_origin_of(manifest: &Value, name: &str) -> (Option<String>, Option<String>) {
    let origin = manifest
        .get("_meta")
        .and_then(|m| m.get("agent_origin"))
        .and_then(|m| m.get(name));
    let cat = origin
        .and_then(|o| o.get("category"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let slug = origin
        .and_then(|o| o.get("slug"))
        .and_then(Value::as_str)
        .map(str::to_string);
    (cat, slug)
}

// ── POST /api/agents ────────────────────────────────────────────────────────

async fn create_handler(State(st): State<AppState>, body: Bytes) -> Response {
    let v = match parse_json(&body) {
        Ok(v) => v,
        Err(r) => return r,
    };

    let name = v
        .get("name")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let Some(name) = name else {
        return json_error(StatusCode::BAD_REQUEST, "name é obrigatório");
    };

    let type_ = v.get("type").and_then(Value::as_str).unwrap_or("");
    if !AGENT_TYPES.contains(&type_) {
        return json_error(
            StatusCode::BAD_REQUEST,
            &format!("type inválido. Use: {}", AGENT_TYPES.join(", ")),
        );
    }

    let gender_clean = v
        .get("gender")
        .and_then(Value::as_str)
        .filter(|g| matches!(*g, "male" | "female" | "other"));

    let emoji = v
        .get("emoji")
        .and_then(Value::as_str)
        .map(|s| s.trim().chars().take(8).collect::<String>());
    let model = v
        .get("model")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let role = v
        .get("role")
        .and_then(Value::as_str)
        .map(|s| s.trim().chars().take(500).collect::<String>());
    let tools = sanitize_string_array(v.get("tools"), 50, 60);
    let mcps = sanitize_string_array(v.get("mcps"), 20, 80);
    let can_talk_to = sanitize_string_array(v.get("can_talk_to"), 50, 60);

    let mut config = Map::new();
    config.insert("type".into(), Value::String(type_.to_string()));
    if let Some(e) = emoji {
        config.insert("emoji".into(), Value::String(e));
    }
    if let Some(g) = gender_clean {
        config.insert("gender".into(), Value::String(g.to_string()));
    }
    if let Some(m) = model {
        config.insert("model".into(), Value::String(m));
    }
    if let Some(r) = role {
        config.insert("role".into(), Value::String(r));
    }
    if !tools.is_empty() {
        config.insert(
            "tools".into(),
            Value::Array(tools.into_iter().map(Value::String).collect()),
        );
    }
    config.insert(
        "mcps".into(),
        Value::Array(mcps.into_iter().map(Value::String).collect()),
    );
    if !can_talk_to.is_empty() {
        config.insert(
            "can_talk_to".into(),
            Value::Array(can_talk_to.into_iter().map(Value::String).collect()),
        );
    }
    if let Some(ctx) = v.get("context").and_then(Value::as_str) {
        config.insert("context".into(), Value::String(ctx.to_string()));
    }

    // Identidade Ed25519: pública vai pro agents.json (campo `pubkey`),
    // privada vai pro vault como secret `AGENT_<NAME>_PRIVKEY`. Nome
    // em uppercase+underscore pra bater regex do vault
    // (`[A-Z][A-Z0-9_]{0,63}`). Falha silenciosa: se o vault não
    // estiver destrancado, o agente é criado mesmo assim — só não terá
    // assinatura de handoff disponível.
    let (priv_bytes, pub_bytes) = crate::agent_keys::generate_keypair();
    let pubkey_hex = crate::agent_keys::bytes_to_hex(&pub_bytes);
    // Nota: passar pubkey em `config` aqui NÃO funciona — `add_agent` em
    // domain-agents reconstrói o agent map só com campos conhecidos
    // (type, emoji, model, role, context_file, tools, mcps, can_talk_to,
    // scope). Qualquer campo extra é silenciosamente dropado. Por isso
    // gravamos pubkey num PATCH separado depois do add_agent.
    let secret_name = crate::agent_keys::privkey_secret_name(&name);
    if let Err(e) = st.vault.set_secret(
        &st.mem_dir,
        &secret_name,
        &crate::agent_keys::bytes_to_hex(&priv_bytes),
    ) {
        // Não bloqueia criação (agente fica em modo "untrusted" — sem JWT),
        // mas avisa pra debug futuro. Causas comuns: vault locked, vault
        // não inicializado, regex de validação rejeitando nome (vide
        // bug histórico que `privkey_secret_name` resolve).
        eprintln!(
            "warn: set_secret falhou pra {secret_name}: {e} — agente {name} sem privkey, handoff vai retornar 401"
        );
    }

    match add_agent(&st.mem_dir, &name, &Value::Object(config)) {
        Ok(mut created) => {
            // Patch pubkey direto no manifest — add_agent dropa campos
            // não-conhecidos no rebuild do agent map. Sem isso, lookup_pubkey
            // retorna None e handoff falha com "pubkey não encontrada".
            if let Some(mut manifest) = domain_agents::load_manifest(&st.mem_dir) {
                let mut patched = false;
                if let Some(agent_cfg) = manifest
                    .pointer_mut(&format!("/agents/{name}"))
                    .and_then(Value::as_object_mut)
                {
                    agent_cfg.insert("pubkey".into(), Value::String(pubkey_hex.clone()));
                    patched = true;
                }
                if patched {
                    if let Err(e) = domain_agents::save_manifest(&st.mem_dir, &manifest) {
                        eprintln!("warn: falha ao patch pubkey no manifest pra {name}: {e}");
                    } else if let Some(obj) = created.as_object_mut() {
                        // Reflete a pubkey na response pro caller saber que
                        // a identidade está pronta.
                        obj.insert("pubkey".into(), Value::String(pubkey_hex));
                    }
                }
            }
            json_response(
                StatusCode::OK,
                json!({ "ok": true, "agent": name, "created": created }),
            )
        }
        Err(AddAgentError::InvalidName) => {
            json_error(StatusCode::BAD_REQUEST, &AddAgentError::InvalidName.to_string())
        }
        Err(e @ AddAgentError::InvalidType { .. }) => {
            json_error(StatusCode::BAD_REQUEST, &e.to_string())
        }
        Err(e @ AddAgentError::AlreadyExists { .. }) => {
            json_error(StatusCode::BAD_REQUEST, &e.to_string())
        }
        Err(AddAgentError::Io(e)) => json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

// ── DELETE /api/agents/:name ────────────────────────────────────────────────

async fn delete_handler(State(st): State<AppState>, AxPath(name): AxPath<String>) -> Response {
    if name == "handoff" {
        return json_error(StatusCode::METHOD_NOT_ALLOWED, "method not allowed");
    }
    let manifest = match load_manifest(&st.mem_dir) {
        Some(m) => m,
        None => return json_error(StatusCode::NOT_FOUND, &format!(r#"agent "{name}" not found"#)),
    };
    let exists = manifest
        .get("agents")
        .and_then(|a| a.get(&name))
        .is_some();
    if !exists {
        return json_error(
            StatusCode::NOT_FOUND,
            &format!(r#"agent "{name}" not found"#),
        );
    }

    let manager = manifest.get("manager").and_then(Value::as_str).unwrap_or("");
    let oracle = manifest.get("oracle").and_then(Value::as_str).unwrap_or("");
    if name == manager || name == oracle {
        return json_error(
            StatusCode::BAD_REQUEST,
            &format!(r#"não pode remover "{name}" (manager/oracle do manifest)"#),
        );
    }

    match remove_agent(&st.mem_dir, &name) {
        Ok(ok) => json_response(StatusCode::OK, json!({ "ok": ok, "agent": name })),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

// ── PATCH /api/agents/:name/model ───────────────────────────────────────────

async fn model_handler(
    State(st): State<AppState>,
    AxPath(name): AxPath<String>,
    body: Bytes,
) -> Response {
    let v = match parse_json(&body) {
        Ok(v) => v,
        Err(r) => return r,
    };

    let manifest = match load_manifest(&st.mem_dir) {
        Some(m) => m,
        None => return json_error(StatusCode::NOT_FOUND, &format!(r#"agent "{name}" not found"#)),
    };
    let exists = manifest
        .get("agents")
        .and_then(|a| a.get(&name))
        .is_some();
    if !exists {
        return json_error(
            StatusCode::NOT_FOUND,
            &format!(r#"agent "{name}" not found"#),
        );
    }

    let model_input = v.get("model").unwrap_or(&Value::Null);
    let updated = match update_model(&st.mem_dir, &name, model_input) {
        Ok(u) => u,
        Err(UpdateModelError::Normalize(e)) => {
            return json_error(StatusCode::BAD_REQUEST, &e.to_string());
        }
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    };

    let propagate = v.get("propagate_seed").and_then(Value::as_bool).unwrap_or(false);
    let mut propagated: Value = Value::Null;
    if propagate {
        let (origin_cat, origin_slug) = agent_origin_of(&manifest, &name);
        let cat = v
            .get("seed_category")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(origin_cat);
        let slug = v
            .get("seed_slug")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(origin_slug);
        match (cat, slug) {
            (Some(cat), Some(slug)) => {
                let seed_path = seed_agents_json(&st, &cat, &slug);
                match domain_agents::propagate_model_to_seed(&seed_path, &name, model_input) {
                    Ok(Some(_)) => {
                        propagated = json!({
                            "category": cat,
                            "slug": slug,
                            "seedPath": seed_path.display().to_string(),
                        });
                    }
                    Ok(None) => {
                        propagated = json!({
                            "skipped": true,
                            "reason": "agente não existe no seed",
                        });
                    }
                    Err(PropagateModelError::Normalize(e)) => {
                        return json_error(StatusCode::BAD_REQUEST, &e.to_string());
                    }
                    Err(e) => {
                        return json_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            json!({
                                "error": format!("propagação falhou: {e}"),
                                "updated": updated,
                            }),
                        );
                    }
                }
            }
            _ => {
                return json_response(
                    StatusCode::BAD_REQUEST,
                    json!({
                        "error": "propagate_seed=true mas origem do seed desconhecida. Envie seed_category + seed_slug explicitamente.",
                        "updated": updated,
                    }),
                );
            }
        }
    }

    json_response(
        StatusCode::OK,
        json!({ "ok": true, "agent": name, "updated": updated, "propagated": propagated }),
    )
}

// ── PATCH /api/agents/:name/autonomy ────────────────────────────────────────

async fn autonomy_handler(
    State(st): State<AppState>,
    AxPath(name): AxPath<String>,
    body: Bytes,
) -> Response {
    let v = match parse_json(&body) {
        Ok(v) => v,
        Err(r) => return r,
    };

    let manifest = match load_manifest(&st.mem_dir) {
        Some(m) => m,
        None => return json_error(StatusCode::NOT_FOUND, &format!(r#"agent "{name}" not found"#)),
    };
    let exists = manifest
        .get("agents")
        .and_then(|a| a.get(&name))
        .is_some();
    if !exists {
        return json_error(
            StatusCode::NOT_FOUND,
            &format!(r#"agent "{name}" not found"#),
        );
    }

    // Paridade com `.cjs`: `{ level, call_human_when }` vão embrulhados no
    // payload passado pra `updateAutonomy`. Se `level` ausente ou `body`
    // não é objeto, repassa `null` pra disparar o erro normalizador.
    let autonomy_input = if let Some(obj) = v.as_object() {
        let mut m = Map::new();
        if let Some(lvl) = obj.get("level") {
            m.insert("level".into(), lvl.clone());
        }
        if let Some(c) = obj.get("call_human_when") {
            m.insert("call_human_when".into(), c.clone());
        }
        Value::Object(m)
    } else {
        Value::Null
    };

    let updated = match update_autonomy(&st.mem_dir, &name, &autonomy_input) {
        Ok(u) => u,
        Err(UpdateAutonomyError::Normalize(e)) => {
            return json_error(StatusCode::BAD_REQUEST, &e.to_string());
        }
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    };

    let propagate = v.get("propagate_seed").and_then(Value::as_bool).unwrap_or(false);
    let mut propagated: Value = Value::Null;
    if propagate {
        let (origin_cat, origin_slug) = agent_origin_of(&manifest, &name);
        let cat = v
            .get("seed_category")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(origin_cat);
        let slug = v
            .get("seed_slug")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(origin_slug);
        match (cat, slug) {
            (Some(cat), Some(slug)) => {
                let seed_path = seed_agents_json(&st, &cat, &slug);
                match domain_agents::propagate_autonomy_to_seed(&seed_path, &name, &autonomy_input)
                {
                    Ok(Some(_)) => {
                        propagated = json!({
                            "category": cat,
                            "slug": slug,
                            "seedPath": seed_path.display().to_string(),
                        });
                    }
                    Ok(None) => {
                        propagated = json!({
                            "skipped": true,
                            "reason": "agente não existe no seed",
                        });
                    }
                    Err(PropagateAutonomyError::Normalize(e)) => {
                        return json_error(StatusCode::BAD_REQUEST, &e.to_string());
                    }
                    Err(e) => {
                        return json_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            json!({
                                "error": format!("propagação falhou: {e}"),
                                "updated": updated,
                            }),
                        );
                    }
                }
            }
            _ => {
                return json_response(
                    StatusCode::BAD_REQUEST,
                    json!({
                        "error": "propagate_seed=true mas origem do seed desconhecida. Envie seed_category + seed_slug explicitamente.",
                        "updated": updated,
                    }),
                );
            }
        }
    }

    json_response(
        StatusCode::OK,
        json!({ "ok": true, "agent": name, "updated": updated, "propagated": propagated }),
    )
}

// ── PUT /api/agents/:name/scope ─────────────────────────────────────────────

async fn scope_handler(
    State(st): State<AppState>,
    AxPath(name): AxPath<String>,
    body: Bytes,
) -> Response {
    let v = match parse_json(&body) {
        Ok(v) => v,
        Err(r) => return r,
    };

    let manifest = match load_manifest(&st.mem_dir) {
        Some(m) => m,
        None => return json_error(StatusCode::NOT_FOUND, &format!(r#"agent "{name}" not found"#)),
    };
    let exists = manifest
        .get("agents")
        .and_then(|a| a.get(&name))
        .is_some();
    if !exists {
        return json_error(
            StatusCode::NOT_FOUND,
            &format!(r#"agent "{name}" not found"#),
        );
    }

    let tools_input = v.get("tools");
    let scope_input = v.get("scope");
    let updated = match update_scope(&st.mem_dir, &name, tools_input, scope_input) {
        Ok(u) => u,
        Err(UpdateScopeError::AgentMissing { .. }) => {
            return json_error(
                StatusCode::NOT_FOUND,
                &format!(r#"agent "{name}" not found"#),
            );
        }
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    };

    let bash_allow = updated
        .get("scope")
        .and_then(|s| s.get("bash_allow"))
        .cloned()
        .unwrap_or_else(|| Value::Array(vec![]));
    let tools = updated.get("tools").cloned().unwrap_or(Value::Null);
    append_agent_log(
        &st.mem_dir,
        &name,
        &json!({
            "ts": now_iso(),
            "type": "scope_updated",
            "source": "ui",
            "tools": tools,
            "bash_allow": bash_allow,
        }),
    );

    json_response(
        StatusCode::OK,
        json!({ "ok": true, "agent": name, "updated": updated }),
    )
}

// ── PUT /api/agents/:name/vault ─────────────────────────────────────────────

async fn vault_handler(
    State(st): State<AppState>,
    AxPath(name): AxPath<String>,
    body: Bytes,
) -> Response {
    let v = match parse_json(&body) {
        Ok(v) => v,
        Err(r) => return r,
    };

    let secrets = v.get("secrets");
    if !secrets.map(Value::is_array).unwrap_or(false) {
        return json_error(StatusCode::BAD_REQUEST, "need {secrets: string[]}");
    }

    let manifest = match load_manifest(&st.mem_dir) {
        Some(m) => m,
        None => return json_error(StatusCode::NOT_FOUND, &format!(r#"agent "{name}" not found"#)),
    };
    let exists = manifest
        .get("agents")
        .and_then(|a| a.get(&name))
        .is_some();
    if !exists {
        return json_error(
            StatusCode::NOT_FOUND,
            &format!(r#"agent "{name}" not found"#),
        );
    }

    let updated = match update_vault_access(&st.mem_dir, &name, secrets.unwrap()) {
        Ok(u) => u,
        Err(UpdateVaultAccessError::AgentMissing { .. }) => {
            return json_error(
                StatusCode::NOT_FOUND,
                &format!(r#"agent "{name}" not found"#),
            );
        }
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    };

    let out_secrets = updated
        .get("secrets")
        .cloned()
        .unwrap_or_else(|| Value::Array(vec![]));
    append_agent_log(
        &st.mem_dir,
        &name,
        &json!({
            "ts": now_iso(),
            "type": "vault_access_updated",
            "source": "ui",
            "secrets": out_secrets.clone(),
        }),
    );

    json_response(
        StatusCode::OK,
        json!({ "ok": true, "agent": name, "secrets": out_secrets }),
    )
}

// ── PUT /api/agents/:name/context ───────────────────────────────────────────

async fn context_handler(
    State(st): State<AppState>,
    AxPath(name): AxPath<String>,
    body: Bytes,
) -> Response {
    let v = match parse_json(&body) {
        Ok(v) => v,
        Err(r) => return r,
    };

    let content = match v.get("content").and_then(Value::as_str) {
        Some(c) => c.to_string(),
        None => return json_error(StatusCode::BAD_REQUEST, "need {content: string}"),
    };

    let manifest = match load_manifest(&st.mem_dir) {
        Some(m) => m,
        None => return json_error(StatusCode::NOT_FOUND, &format!(r#"agent "{name}" not found"#)),
    };
    let cfg = manifest.get("agents").and_then(|a| a.get(&name));
    let Some(cfg) = cfg else {
        return json_error(
            StatusCode::NOT_FOUND,
            &format!(r#"agent "{name}" not found"#),
        );
    };
    let context_file = cfg
        .get("context_file")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("agents/{name}.md"));

    let bytes = content.len();
    match save_mini_context(&st.mem_dir, &context_file, &content) {
        Ok(_) => json_response(
            StatusCode::OK,
            json!({
                "ok": true,
                "agent": name,
                "context_file": context_file,
                "bytes": bytes,
            }),
        ),
        Err(e) => json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

// ── PUT /api/agents/:name/mcps ──────────────────────────────────────────────

async fn mcps_handler(
    State(st): State<AppState>,
    AxPath(name): AxPath<String>,
    body: Bytes,
) -> Response {
    let v = match parse_json(&body) {
        Ok(v) => v,
        Err(r) => return r,
    };

    let mcps = v.get("mcps");
    if !mcps.map(Value::is_array).unwrap_or(false) {
        return json_error(StatusCode::BAD_REQUEST, "need {mcps: string[]}");
    }

    let mut manifest = match load_manifest(&st.mem_dir) {
        Some(m) => m,
        None => return json_error(StatusCode::NOT_FOUND, &format!(r#"agent "{name}" not found"#)),
    };
    let agent_obj = manifest
        .get_mut("agents")
        .and_then(Value::as_object_mut)
        .and_then(|m| m.get_mut(&name))
        .and_then(Value::as_object_mut);
    let Some(agent_obj) = agent_obj else {
        return json_error(
            StatusCode::NOT_FOUND,
            &format!(r#"agent "{name}" not found"#),
        );
    };
    let clean = sanitize_string_array(mcps, 20, 80);
    let clean_arr: Vec<Value> = clean.iter().cloned().map(Value::String).collect();
    agent_obj.insert("mcps".into(), Value::Array(clean_arr.clone()));

    match save_manifest(&st.mem_dir, &manifest) {
        Ok(_) => json_response(
            StatusCode::OK,
            json!({ "ok": true, "agent": name, "mcps": clean_arr }),
        ),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

// (GET /api/mcps migrou pra `mcp_routes` — agora inclui o registro do projeto.)

// ── PUT /api/docs/:doc ──────────────────────────────────────────────────────

const DOC_ALLOWLIST: &[&str] = &[
    "SOUL.md",
    "PROJECT.md",
    "DECISIONS.md",
    "GOTCHAS.md",
    "GLOSSARY.md",
    "CODE_INDEX.md",
];

fn is_valid_doc_name(name: &str) -> bool {
    if !name.ends_with(".md") {
        return false;
    }
    let stem = &name[..name.len() - 3];
    !stem.is_empty()
        && stem
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == '_')
}

async fn doc_save_handler(
    State(st): State<AppState>,
    AxPath(doc): AxPath<String>,
    body: Bytes,
) -> Response {
    // Paridade com regex `^[A-Z_]+\.md$` do `.cjs:3020`. Rotas fora do
    // padrão não deveriam chegar aqui, mas validamos de novo.
    if !is_valid_doc_name(&doc) {
        return json_error(StatusCode::NOT_FOUND, "not found");
    }
    if !DOC_ALLOWLIST.contains(&doc.as_str()) {
        return json_error(
            StatusCode::NOT_FOUND,
            &format!(r#"doc "{doc}" não permitido"#),
        );
    }

    let v = match parse_json(&body) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let content = match v.get("content").and_then(Value::as_str) {
        Some(c) => c.to_string(),
        None => return json_error(StatusCode::BAD_REQUEST, "need {content: string}"),
    };
    if content.len() > 500_000 {
        return json_error(StatusCode::BAD_REQUEST, "doc muito longo (máx 500KB)");
    }

    let _ = fs::create_dir_all(&st.mem_dir);
    let path = st.mem_dir.join(&doc);
    let normalized = if content.ends_with('\n') {
        content.clone()
    } else {
        format!("{content}\n")
    };
    match fs::write(&path, normalized) {
        Ok(_) => json_response(
            StatusCode::OK,
            json!({ "ok": true, "doc": doc, "bytes": content.len() }),
        ),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

// ── GET /api/agents/log ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AgentLogQuery {
    agent: Option<String>,
    limit: Option<String>,
}

async fn log_handler(State(st): State<AppState>, Query(q): Query<AgentLogQuery>) -> Response {
    let agent = q.agent.as_deref().filter(|s| !s.is_empty());
    let Some(agent) = agent else {
        return json_error(StatusCode::BAD_REQUEST, "missing agent");
    };
    // Paridade com `parseInt(x || '50', 10)` — NaN vira NaN (negativos?).
    // Se `limit` ausente → 50; se inválido → NaN → sem slice.
    let limit = q
        .limit
        .as_deref()
        .map(|s| s.parse::<i64>().ok())
        .unwrap_or(Some(50));
    let limit_val = match limit {
        Some(n) if n > 0 => Some(n as usize),
        _ => None,
    };
    let entries = read_agent_log(&st.mem_dir, agent, limit_val);
    json_response(
        StatusCode::OK,
        json!({ "agent": agent, "entries": entries }),
    )
}

// ── GET /api/agents/work ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AgentWorkQuery {
    agent: Option<String>,
    limit: Option<String>,
}

async fn work_handler(State(st): State<AppState>, Query(q): Query<AgentWorkQuery>) -> Response {
    let filter_agent = q.agent.as_deref().filter(|s| !s.is_empty());
    let limit: usize = q
        .limit
        .as_deref()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|n| *n > 0)
        .map(|n| n as usize)
        .unwrap_or(100);

    let mut entries: Vec<Value> = Vec::new();
    let obs_path = st.mem_dir.join("observations.jsonl");
    if let Ok(raw) = fs::read_to_string(&obs_path) {
        for line in raw.split('\n').filter(|l| !l.is_empty()) {
            let Ok(obj) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let Some(aid) = obj.get("agent_id").and_then(Value::as_str) else {
                continue;
            };
            if let Some(needed) = filter_agent {
                if aid != needed {
                    continue;
                }
            }
            entries.push(obj);
        }
    }
    let slice: Vec<Value> = if entries.len() > limit {
        entries[entries.len() - limit..].to_vec()
    } else {
        entries
    };
    let reversed: Vec<Value> = slice.into_iter().rev().collect();
    let count = reversed.len();
    let agent_out = match filter_agent {
        Some(a) => Value::String(a.to_string()),
        None => Value::Null,
    };

    json_response(
        StatusCode::OK,
        json!({ "agent": agent_out, "count": count, "entries": reversed }),
    )
}

// ── POST /api/agents/route ──────────────────────────────────────────────────
// Paridade com `server.cjs:1574-1593` (`handleRoute`).

const VALID_INTENTS: &[&str] = &["ask", "delegate", "report", "ack", "escalate"];

async fn route_handler(State(st): State<AppState>, body: Bytes) -> Response {
    let v = match parse_json(&body) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let from = v.get("from").and_then(Value::as_str).unwrap_or("");
    if from.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "need {from}");
    }
    let file = v.get("file").and_then(Value::as_str);
    let topic = v.get("topic").and_then(Value::as_str);
    let intent = v.get("intent").and_then(Value::as_str);
    if let Some(i) = intent {
        if !VALID_INTENTS.contains(&i) {
            return json_response(
                StatusCode::BAD_REQUEST,
                json!({ "error": "invalid intent", "allowed": VALID_INTENTS }),
            );
        }
    }
    let result = domain_router::route(
        &st.mem_dir,
        from,
        file,
        topic,
        intent,
        &domain_router::SystemClock,
    );
    if let Some(err) = result.error {
        return json_response(StatusCode::NOT_FOUND, json!({ "error": err }));
    }
    let mut out = Map::new();
    out.insert("from".into(), Value::String(from.to_string()));
    out.insert(
        "can_talk_to".into(),
        Value::Array(
            result
                .can_talk_to
                .into_iter()
                .map(Value::String)
                .collect(),
        ),
    );
    out.insert("candidates".into(), Value::Array(result.candidates));
    out.insert(
        "note".into(),
        match result.note {
            Some(s) => Value::String(s),
            None => Value::Null,
        },
    );
    json_response(StatusCode::OK, Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_doc_name_matches_regex() {
        assert!(is_valid_doc_name("SOUL.md"));
        assert!(is_valid_doc_name("CODE_INDEX.md"));
        assert!(!is_valid_doc_name("soul.md"));
        assert!(!is_valid_doc_name("a.md"));
        assert!(!is_valid_doc_name(".md"));
        assert!(!is_valid_doc_name("SOUL.txt"));
        assert!(!is_valid_doc_name("X1.md"));
    }

    #[test]
    fn sanitize_string_array_trims_and_caps() {
        let v = json!([" foo ", "", "bar", 42, "baz"]);
        let r = sanitize_string_array(Some(&v), 2, 10);
        assert_eq!(r, vec!["foo", "bar"]);
    }

    #[test]
    fn sanitize_string_array_non_array() {
        assert!(sanitize_string_array(None, 10, 10).is_empty());
        assert!(sanitize_string_array(Some(&json!("str")), 10, 10).is_empty());
    }
}
