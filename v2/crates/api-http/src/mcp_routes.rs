//! Registro de MCP do projeto + materialização no `.mcp.json`.
//!
//! Hoje os agentes só **referenciam** nomes de MCP; as definições viviam só no
//! `~/.claude.json`/`.mcp.json` do Claude Code. Aqui o projeto passa a **ser
//! dono** de um registro de MCPs (cadastrável pela UI), com escolha de
//! armazenamento por MCP:
//!
//! - `storage:"db"`  → def completa em `<mem_dir>/mcps.json` (plano, sempre
//!   visível a todos os agentes do projeto).
//! - `storage:"vault"` → def completa cifrada no cofre (secret `MCP_<NOME>`);
//!   no `mcps.json` fica só um **stub** não-secreto (name/transport/description)
//!   pra catálogo/compartilhamento mesmo com cofre trancado.
//!
//! **Conexão real:** ao registrar, a def é materializada em
//! `<project_root>/.mcp.json` (`mcpServers`), que é o que o Claude Code lê pro
//! agente spawnado conectar de fato. Por transport:
//! - stdio → `{ command, args, env }`
//! - http/sse → `{ type, url, headers }`
//!
//! Rotas de escrita passam pelo origin-guard + auth (middleware global).

use std::fs;
use std::path::Path;

use axum::body::Bytes;
use axum::extract::{Path as AxPath, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use serde_json::{json, Map, Value};

use crate::{json_error, json_response, AppState};

pub fn mcp_router() -> Router<AppState> {
    Router::new()
        .route("/api/mcps", get(list_handler).post(register_handler))
        .route(
            "/api/mcps/:name",
            get(get_one_handler).delete(delete_handler),
        )
}

// ── Registry (mcps.json) ─────────────────────────────────────────────────────

fn registry_path(mem_dir: &Path) -> std::path::PathBuf {
    mem_dir.join("mcps.json")
}

/// Carrega o registry `{ schema_version, mcps: {} }`. Tolera arquivo ausente.
fn load_registry(mem_dir: &Path) -> Value {
    let path = registry_path(mem_dir);
    let parsed = fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .filter(|v| v.is_object());
    match parsed {
        Some(mut v) => {
            if !v.get("mcps").map(Value::is_object).unwrap_or(false) {
                v.as_object_mut()
                    .unwrap()
                    .insert("mcps".into(), Value::Object(Map::new()));
            }
            v
        }
        None => {
            let mut m = Map::new();
            m.insert("mcps".into(), Value::Object(Map::new()));
            let mut v = Value::Object(m);
            core_paths::schema::stamp(&mut v, core_paths::schema::CURRENT);
            v
        }
    }
}

fn save_registry(mem_dir: &Path, data: &mut Value) -> std::io::Result<()> {
    core_paths::schema::stamp(data, core_paths::schema::CURRENT);
    fs::create_dir_all(mem_dir)?;
    let pretty = serde_json::to_string_pretty(data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(registry_path(mem_dir), format!("{pretty}\n"))
}

/// Nome do secret no cofre pra um MCP (regex do vault: `[A-Z][A-Z0-9_]{0,63}`).
fn vault_secret_name(name: &str) -> String {
    let norm: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
        .collect();
    format!("MCP_{norm}")
}

fn is_valid_mcp_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Stub não-secreto guardado no mcps.json (catálogo). Pra storage=db é a própria
/// def; pra storage=vault são só os campos não sensíveis.
fn stub_of(def: &Value, storage: &str) -> Value {
    if storage == "vault" {
        json!({
            "name": def.get("name").cloned().unwrap_or(Value::Null),
            "transport": def.get("transport").cloned().unwrap_or(json!("stdio")),
            "description": def.get("description").cloned().unwrap_or(json!("")),
            "storage": "vault",
        })
    } else {
        let mut d = def.clone();
        if let Some(o) = d.as_object_mut() {
            o.insert("storage".into(), json!("db"));
        }
        d
    }
}

// ── Materialização no .mcp.json ──────────────────────────────────────────────

/// Monta a entrada do `.mcp.json` (formato Claude Code) a partir da def.
fn mcp_json_entry(def: &Value) -> Value {
    let transport = def.get("transport").and_then(Value::as_str).unwrap_or("stdio");
    match transport {
        "http" | "sse" => {
            let mut e = Map::new();
            e.insert("type".into(), json!(transport));
            e.insert("url".into(), def.get("url").cloned().unwrap_or(json!("")));
            if let Some(h) = def.get("headers").filter(|v| v.is_object()) {
                if !h.as_object().unwrap().is_empty() {
                    e.insert("headers".into(), h.clone());
                }
            }
            Value::Object(e)
        }
        _ => {
            let mut e = Map::new();
            e.insert("command".into(), def.get("command").cloned().unwrap_or(json!("")));
            e.insert("args".into(), def.get("args").cloned().unwrap_or(json!([])));
            if let Some(env) = def.get("env").filter(|v| v.is_object()) {
                if !env.as_object().unwrap().is_empty() {
                    e.insert("env".into(), env.clone());
                }
            }
            Value::Object(e)
        }
    }
}

/// Upsert (`entry = Some`) ou remove (`None`) o MCP `name` no
/// `<project_root>/.mcp.json`, preservando os demais servers.
fn materialize(project_root: &Path, name: &str, entry: Option<Value>) -> std::io::Result<()> {
    let path = project_root.join(".mcp.json");
    let mut root = fs::read_to_string(&path)
        .ok()
        .and_then(|r| serde_json::from_str::<Value>(&r).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| json!({ "mcpServers": {} }));
    if !root.get("mcpServers").map(Value::is_object).unwrap_or(false) {
        root.as_object_mut()
            .unwrap()
            .insert("mcpServers".into(), Value::Object(Map::new()));
    }
    let servers = root
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
        .unwrap();
    match entry {
        Some(e) => {
            servers.insert(name.to_string(), e);
        }
        None => {
            servers.remove(name);
        }
    }
    let pretty = serde_json::to_string_pretty(&root)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(&path, format!("{pretty}\n"))
}

/// Resolve a def completa de um MCP do registry (db = inline; vault = lê o
/// secret). Retorna `None` se não existe; `Err` se vault necessário e falhou.
fn full_def_parts(
    mem_dir: &Path,
    vault: &std::sync::Arc<dyn core_ports::VaultStore>,
    name: &str,
) -> Result<Option<Value>, String> {
    let reg = load_registry(mem_dir);
    let Some(entry) = reg.get("mcps").and_then(|m| m.get(name)) else {
        return Ok(None);
    };
    let storage = entry.get("storage").and_then(Value::as_str).unwrap_or("db");
    if storage == "vault" {
        match vault.get_secret(mem_dir, &vault_secret_name(name)) {
            Ok(Some(s)) => serde_json::from_str::<Value>(&s)
                .map(Some)
                .map_err(|e| format!("def do MCP no cofre inválida: {e}")),
            Ok(None) => Err("def não encontrada no cofre (destrancado?)".into()),
            Err(e) => Err(format!("cofre: {e}")),
        }
    } else {
        Ok(Some(entry.clone()))
    }
}

fn full_def(st: &AppState, name: &str) -> Result<Option<Value>, String> {
    full_def_parts(&st.mem_dir, &st.vault, name)
}

/// Re-materializa TODOS os MCPs registrados no `.mcp.json` do projeto. Chamado
/// no startup pra cobrir: `.mcp.json` apagado/restaurado, editado à mão, ou
/// `project_root` trocado (materializa no path atual). Idempotente. MCPs no
/// cofre são pulados se o cofre estiver trancado (re-materializam depois).
pub(crate) fn materialize_all(
    mem_dir: &Path,
    project_root: &Path,
    vault: &std::sync::Arc<dyn core_ports::VaultStore>,
) -> usize {
    let reg = load_registry(mem_dir);
    let Some(mcps) = reg.get("mcps").and_then(Value::as_object) else {
        return 0;
    };
    let mut n = 0;
    for name in mcps.keys() {
        let def = match full_def_parts(mem_dir, vault, name) {
            Ok(Some(d)) => d,
            _ => continue, // vault trancado / ausente → pula
        };
        if materialize(project_root, name, Some(mcp_json_entry(&def))).is_ok() {
            n += 1;
        }
    }
    n
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// `GET /api/mcps` — global+project (do ~/.claude.json) + referenciados pelos
/// agentes + **registrados** (catálogo do projeto, com stub/def + storage).
async fn list_handler(State(st): State<AppState>) -> Response {
    let mut global: Vec<String> = Vec::new();
    let mut project: Vec<String> = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        let cfg_path = std::path::PathBuf::from(home).join(".claude.json");
        if let Ok(raw) = fs::read_to_string(&cfg_path) {
            if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                if let Some(servers) = v.get("mcpServers").and_then(Value::as_object) {
                    global = servers.keys().cloned().collect();
                }
                if let Some(proj) = v
                    .get("projects")
                    .and_then(|p| p.get(st.project_root.display().to_string().as_str()))
                {
                    if let Some(servers) = proj.get("mcpServers").and_then(Value::as_object) {
                        project = servers.keys().cloned().collect();
                    }
                }
            }
        }
    }

    let mut referenced: Vec<String> = Vec::new();
    if let Some(manifest) = domain_agents::load_manifest(&st.mem_dir) {
        if let Some(agents) = manifest.get("agents").and_then(Value::as_object) {
            let mut seen = std::collections::BTreeSet::new();
            for a in agents.values() {
                if let Some(mcps) = a.get("mcps").and_then(Value::as_array) {
                    for m in mcps.iter().filter_map(Value::as_str) {
                        if seen.insert(m.to_string()) {
                            referenced.push(m.to_string());
                        }
                    }
                }
            }
        }
    }

    let reg = load_registry(&st.mem_dir);
    let registered: Vec<Value> = reg
        .get("mcps")
        .and_then(Value::as_object)
        .map(|m| m.values().cloned().collect())
        .unwrap_or_default();

    let mut all: Vec<String> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let reg_names: Vec<String> = registered
        .iter()
        .filter_map(|d| d.get("name").and_then(Value::as_str).map(String::from))
        .collect();
    for s in global.iter().chain(project.iter()).chain(referenced.iter()).chain(reg_names.iter()) {
        if seen.insert(s.clone()) {
            all.push(s.clone());
        }
    }

    json_response(
        StatusCode::OK,
        json!({ "global": global, "project": project, "referenced": referenced, "registered": registered, "all": all }),
    )
}

/// `GET /api/mcps/:name` — def completa (resolve vault). Pra edição na UI.
async fn get_one_handler(State(st): State<AppState>, AxPath(name): AxPath<String>) -> Response {
    match full_def(&st, &name) {
        Ok(Some(def)) => json_response(StatusCode::OK, json!({ "mcp": def })),
        Ok(None) => json_error(StatusCode::NOT_FOUND, "MCP não registrado"),
        Err(e) => json_error(StatusCode::CONFLICT, &e),
    }
}

/// `POST /api/mcps` — registra/atualiza um MCP e materializa no .mcp.json.
async fn register_handler(State(st): State<AppState>, body: Bytes) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let name = v.get("name").and_then(Value::as_str).unwrap_or("").trim().to_string();
    if !is_valid_mcp_name(&name) {
        return json_error(StatusCode::BAD_REQUEST, "nome inválido (use [a-z0-9_-], até 64)");
    }
    let transport = v.get("transport").and_then(Value::as_str).unwrap_or("stdio");
    if !matches!(transport, "stdio" | "http" | "sse") {
        return json_error(StatusCode::BAD_REQUEST, "transport: stdio|http|sse");
    }
    // Validação mínima por transport.
    if transport == "stdio" {
        if v.get("command").and_then(Value::as_str).map(str::trim).unwrap_or("").is_empty() {
            return json_error(StatusCode::BAD_REQUEST, "stdio exige 'command'");
        }
    } else if v.get("url").and_then(Value::as_str).map(str::trim).unwrap_or("").is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "http/sse exige 'url'");
    }
    let storage = match v.get("storage").and_then(Value::as_str) {
        Some("vault") => "vault",
        _ => "db",
    };

    // def canônica (campos conhecidos + storage/name).
    let mut def = json!({
        "name": name,
        "transport": transport,
        "description": v.get("description").cloned().unwrap_or(json!("")),
        "storage": storage,
    });
    {
        let o = def.as_object_mut().unwrap();
        for k in ["command", "args", "url", "env", "headers"] {
            if let Some(val) = v.get(k) {
                o.insert(k.into(), val.clone());
            }
        }
    }

    // Persiste conforme o storage.
    if storage == "vault" {
        let secret = serde_json::to_string(&def).unwrap_or_default();
        if let Err(e) = st.vault.set_secret(&st.mem_dir, &vault_secret_name(&name), &secret) {
            return json_error(
                StatusCode::CONFLICT,
                &format!("falha ao salvar no cofre (destrancado?): {e}"),
            );
        }
    } else {
        // Re-registro mudando vault→db: apaga o secret órfão pra não deixar
        // a def antiga (possivelmente sensível) parada no cofre. No-op se não existir.
        let _ = st.vault.delete_secret(&st.mem_dir, &vault_secret_name(&name));
    }
    let mut reg = load_registry(&st.mem_dir);
    if let Some(m) = reg.get_mut("mcps").and_then(Value::as_object_mut) {
        m.insert(name.clone(), stub_of(&def, storage));
    }
    if let Err(e) = save_registry(&st.mem_dir, &mut reg) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("mcps.json: {e}"));
    }

    // Conexão real: materializa no .mcp.json do projeto.
    if let Err(e) = materialize(&st.project_root, &name, Some(mcp_json_entry(&def))) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!(".mcp.json: {e}"));
    }

    json_response(StatusCode::OK, json!({ "ok": true, "mcp": def }))
}

/// `DELETE /api/mcps/:name` — remove do registry, do cofre (se vault) e do .mcp.json.
async fn delete_handler(State(st): State<AppState>, AxPath(name): AxPath<String>) -> Response {
    let mut reg = load_registry(&st.mem_dir);
    let existed = reg
        .get("mcps")
        .and_then(|m| m.get(&name))
        .map(|e| e.get("storage").and_then(Value::as_str) == Some("vault"));
    if let Some(was_vault) = existed {
        if was_vault {
            let _ = st.vault.delete_secret(&st.mem_dir, &vault_secret_name(&name));
        }
        if let Some(m) = reg.get_mut("mcps").and_then(Value::as_object_mut) {
            m.remove(&name);
        }
        let _ = save_registry(&st.mem_dir, &mut reg);
    }
    let _ = materialize(&st.project_root, &name, None);
    json_response(StatusCode::OK, json!({ "ok": true }))
}
