//! `api-http` — servidor HTTP do project-mem.
//!
//! Paridade com `plugin/ui/server.cjs` (3.665 linhas). Porta em sub-fases.
//!
//! **F4-05a** ✅: static serving + `/api/network-config` + `/api/bus/*`.
//! **F4-05b** ✅: `/api/vault/*` (20 rotas).
//! **F4-05c (atual)**: destructive + seeds + best-practices + pipelines +
//! architect + skills + ides.

pub mod agent_keys;
pub mod agent_runner;
pub mod agents_routes;
pub mod auth;
pub mod auth_routes;
pub mod db_routes;
pub mod dev_routes;
pub mod best_practices_routes;
pub mod chat_routes;
pub mod chat_send_routes;
pub mod data_route;
pub mod destructive_routes;
pub mod handoff_routes;
pub mod humans_routes;
pub mod misc_routes;
pub mod pipelines_routes;
pub mod scope_routes;
pub mod search_routes;
pub mod seeds_routes;
pub mod subagent;
pub mod subagent_routes;
pub mod tenant;
pub mod topic_routes;
pub mod topics;
pub mod unlock_guard;
pub mod vault_routes;

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use core_ports::VaultStore;
use core_queue::{AgentQueue, AgentQueueConfig};
use domain_bus::{
    compact_old_files, get_config, get_message, get_state, list_day_files, set_config, Clock,
    CompactOpts, GetStateOpts, SystemClock, DEFAULT_RETENTION, MAX_RETENTION, MIN_RETENTION,
};

use crate::unlock_guard::UnlockGuard;

pub const DEFAULT_PORT: u16 = 37778;

// ── Network config ──────────────────────────────────────────────────────────
// Paridade com `server.cjs:45-79`.

/// Valores aceitos em `network.json:bind`.
pub const VALID_BINDS: [&str; 3] = ["localhost", "lan", "custom"];

/// Configuração de bind do servidor. Persiste em `<memDir>/network.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkConfig {
    pub bind: String,
    pub custom_host: String,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            bind: "localhost".into(),
            custom_host: String::new(),
        }
    }
}

/// Caminho do `network.json`.
pub fn network_cfg_path(mem_dir: &Path) -> PathBuf {
    mem_dir.join("network.json")
}

/// Carrega `network.json`. Silencia qualquer erro (paridade com
/// `try { } catch { return default }` do `.cjs`).
pub fn load_network_config(mem_dir: &Path) -> NetworkConfig {
    let raw = match std::fs::read_to_string(network_cfg_path(mem_dir)) {
        Ok(r) => r,
        Err(_) => return NetworkConfig::default(),
    };
    let v: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return NetworkConfig::default(),
    };
    let bind = v
        .get("bind")
        .and_then(Value::as_str)
        .filter(|s| VALID_BINDS.contains(s))
        .unwrap_or("localhost")
        .to_string();
    let custom_host = v
        .get("customHost")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    NetworkConfig { bind, custom_host }
}

/// Persiste `network.json` com indent 2 sem trailing newline (paridade
/// com `JSON.stringify(cfg, null, 2)` do `.cjs`).
pub fn save_network_config(mem_dir: &Path, cfg: &NetworkConfig) {
    let _ = std::fs::create_dir_all(mem_dir);
    let mut obj = Map::new();
    obj.insert("bind".into(), Value::String(cfg.bind.clone()));
    obj.insert("customHost".into(), Value::String(cfg.custom_host.clone()));
    if let Ok(s) = serde_json::to_string_pretty(&Value::Object(obj)) {
        let _ = std::fs::write(network_cfg_path(mem_dir), s);
    }
}

/// Resolve o host físico para bind a partir da config.
pub fn resolve_bind_host(cfg: &NetworkConfig) -> String {
    match cfg.bind.as_str() {
        "lan" => "0.0.0.0".into(),
        "custom" if !cfg.custom_host.is_empty() => cfg.custom_host.clone(),
        _ => "127.0.0.1".into(),
    }
}

/// IP info.
#[derive(Debug, Clone)]
pub struct LanIp {
    pub iface: String,
    pub address: String,
}

/// Lista IPv4 não-loopback agrupados por interface. Paridade com
/// `listLanIps()` do `.cjs` usando `os.networkInterfaces()`.
pub fn list_lan_ips() -> Vec<LanIp> {
    let Ok(addrs) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for a in addrs {
        if a.is_loopback() {
            continue;
        }
        if let IpAddr::V4(v4) = a.ip() {
            out.push(LanIp {
                iface: a.name,
                address: v4.to_string(),
            });
        }
    }
    out
}

// ── App state ───────────────────────────────────────────────────────────────

/// Estado compartilhado entre handlers.
#[derive(Clone)]
pub struct AppState {
    pub mem_dir: PathBuf,
    pub project_root: PathBuf,
    pub plugin_dir: PathBuf,
    pub port: u16,
    pub bind_host: String,
    pub network_cfg: Arc<Mutex<NetworkConfig>>,
    pub clock: Arc<dyn Clock + Send + Sync>,
    pub html: Arc<String>,
    pub dist_dir: Option<PathBuf>,
    pub vault: Arc<dyn VaultStore>,
    pub unlock_guard: Arc<UnlockGuard>,
    pub auto_queue: Arc<AgentQueue>,
    /// Índice em memória `trace_id → eventos`. Populated no startup
    /// via `build_from_disk`. Callers que emitem eventos no bus devem
    /// chamar `trace_index.record(ev)` em seguida pra manter atualizado.
    pub trace_index: Arc<domain_bus::TraceIndex>,
    /// Map `trace_id → oneshot sender` pra backpressure síncrono opt-in.
    /// Quando o handoff é chamado com `wait_ack=true`, server insere
    /// um sender aqui e aguarda. Hook `on_drain_start` da queue notifica
    /// (envia `()`) quando o trace começa a ser processado.
    pub pending_acks: Arc<Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<()>>>>,
    /// Rate limit do /api/agents/handoff (single). Map `from → janela
    /// de timestamps`. Acima do limite, response é 429 com hint pra
    /// usar /handoff_batch. Defesa em código contra agente que ignora
    /// instrução do prompt — confiança em prompt é brecha conhecida.
    pub handoff_rate: Arc<Mutex<std::collections::HashMap<String, std::collections::VecDeque<std::time::Instant>>>>,
    /// FTS5 search index (SQLite bundled) sobre chats/observations.
    /// Build at startup + update incremental nos hot paths de append.
    /// Pode ser None se o SQLite falhar ao abrir (degrada graciosamente
    /// — search devolve "indisponível", restante funciona).
    pub search: Option<Arc<core_storage::SearchIndex>>,
    /// Manager de subagents efêmeros paralelos. Pai (agente) chama
    /// /api/agents/subagent/spawn pra dispatch + poll pra resultado.
    /// Custo de tokens = zero (subagent tem context próprio, não
    /// inflama o do pai).
    pub subagents: Arc<subagent::SubagentManager>,
}

impl AppState {
    pub fn new(
        mem_dir: PathBuf,
        project_root: PathBuf,
        plugin_dir: PathBuf,
        port: u16,
        html: String,
        dist_dir: Option<PathBuf>,
        vault: Arc<dyn VaultStore>,
    ) -> Self {
        let cfg = load_network_config(&mem_dir);
        let bind_host = resolve_bind_host(&cfg);
        let unlock_guard = Arc::new(UnlockGuard::load(&mem_dir));

        // Trace index: popular do disco com a janela de retention padrão.
        // O custo é O(N eventos), pagar uma vez no startup pra ter O(1)
        // lookup nas queries futuras (vs scan completo a cada chamada).
        // Criado antes do runner pra poder ser passado pra ele.
        let trace_index = Arc::new(domain_bus::TraceIndex::new());
        trace_index.build_from_disk(&mem_dir, domain_bus::DEFAULT_RETENTION, &SystemClock);

        // Search index — abrir antes do runner pra poder injetar.
        // Auto-rebuild se índice vazio mas há JSONLs no disco. Cobre
        // o caso "snapshot foi restaurado e search.db ficou stale" ou
        // "search.db foi apagado/corrompido". JsonlRepository é
        // sempre source-of-truth; search reconstrói daí.
        let search = core_storage::SearchIndex::open(&mem_dir)
            .map(Arc::new)
            .map_err(|e| eprintln!("warn: search index indisponível: {e}"))
            .ok();
        if let Some(ref idx) = search {
            let indexed = idx.count().unwrap_or(0);
            let has_chats = mem_dir.join("agent_chats").exists();
            let has_obs = mem_dir.join("observations.jsonl").exists();
            if indexed == 0 && (has_chats || has_obs) {
                eprintln!("search: índice vazio mas há JSONLs — rebuild automático...");
                match search_routes::rebuild_search_from_jsonls(&mem_dir, idx) {
                    Ok(n) => eprintln!("search: rebuild indexou {n} docs"),
                    Err(e) => eprintln!("search: rebuild falhou (não crítico): {e}"),
                }
            }
        }

        let runner = agent_runner::build_runner(
            mem_dir.clone(),
            project_root.clone(),
            plugin_dir.clone(),
            port,
            Arc::clone(&vault),
            Arc::clone(&trace_index),
            search.clone(),
        );

        // Backpressure síncrono — quando wait_ack=true no handoff, server
        // registra um oneshot pelo trace_id; hook abaixo solta o sender
        // assim que a queue chama o runner pro trace (= "B começou").
        let pending_acks: Arc<
            Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<()>>>,
        > = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let acks_for_hook = Arc::clone(&pending_acks);

        // Queue persistente: state_dir = <mem_dir>/queue. Sobrevive
        // restart (pending+running são restaurados; reconcile() depois
        // detecta turns "in-flight" que não saíram porque o processo
        // morreu e devolve pra pending).
        let mut queue_cfg = AgentQueueConfig::new(runner);
        queue_cfg.state_dir = Some(mem_dir.join("queue"));
        queue_cfg.on_drain_start = Some(Arc::new(move |ev: core_queue::DrainStartEvent| {
            let mut map = acks_for_hook.lock().unwrap();
            for tid in &ev.trace_ids {
                if let Some(tx) = map.remove(tid) {
                    let _ = tx.send(());
                }
            }
        }));
        let auto_queue = AgentQueue::new(queue_cfg);

        Self {
            mem_dir,
            project_root,
            plugin_dir,
            port,
            bind_host,
            network_cfg: Arc::new(Mutex::new(cfg)),
            clock: Arc::new(SystemClock),
            html: Arc::new(html),
            dist_dir,
            vault,
            unlock_guard,
            auto_queue,
            trace_index,
            pending_acks,
            handoff_rate: Arc::new(Mutex::new(std::collections::HashMap::new())),
            search,
            subagents: Arc::new(subagent::SubagentManager::new()),
        }
    }

    pub fn with_clock(mut self, clock: Arc<dyn Clock + Send + Sync>) -> Self {
        self.clock = clock;
        self
    }
}

// ── Router ──────────────────────────────────────────────────────────────────

/// Constrói o router Axum com todas as rotas das fases F4-05a + F4-05b.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(serve_index))
        .route("/api/network-config", get(get_network).put(put_network))
        .route("/api/bus/state", get(bus_state))
        .route("/api/bus/trace", get(bus_trace))
        .route("/api/bus/config", get(bus_config_get).put(bus_config_put))
        .route("/api/bus/compact", post(bus_compact))
        .merge(vault_routes::vault_router())
        .merge(destructive_routes::destructive_router())
        .merge(seeds_routes::seeds_router())
        .merge(best_practices_routes::best_practices_router())
        .merge(pipelines_routes::pipelines_router())
        .merge(misc_routes::misc_router())
        .merge(scope_routes::scope_router())
        .merge(auth_routes::auth_router())
        .merge(topic_routes::topic_router())
        .merge(db_routes::db_router())
        .merge(dev_routes::dev_router())
        .merge(search_routes::search_router())
        .merge(subagent_routes::subagent_router())
        .merge(humans_routes::humans_router())
        .merge(agents_routes::agents_router())
        .merge(chat_routes::chat_router())
        .merge(chat_send_routes::chat_send_router())
        .merge(handoff_routes::handoff_router())
        .merge(data_route::data_router())
        .fallback(serve_static)
        .layer(middleware::from_fn_with_state(state.clone(), auth::auth_middleware))
        .with_state(state)
}

// ── Static serving ──────────────────────────────────────────────────────────

async fn serve_index(State(st): State<AppState>) -> Response {
    // Prefere a UI moderna em `ui-next/dist/` quando disponível, igual
    // ao `serve_static` faz pros outros paths. Sem isso, a rota `/`
    // ficava amarrada no `INDEX_HTML` embedded (legacy), que chama
    // endpoints removidos (`/api/data` etc).
    if let Some(dist) = st.dist_dir.as_deref() {
        let candidate = dist.join("index.html");
        if let Ok(canon) = candidate.canonicalize() {
            if canon.starts_with(dist) && canon.is_file() {
                return serve_file(&canon);
            }
        }
    }
    html_response(&st.html)
}

async fn serve_static(State(st): State<AppState>, uri: Uri, method: Method) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return not_found();
    }
    let path = uri.path();
    // Dist (ui-next/dist) tem prioridade quando existe.
    if let Some(dist) = st.dist_dir.as_deref() {
        let clean = path.trim_start_matches('/');
        let target = if clean.is_empty() {
            "index.html"
        } else {
            clean
        };
        let target = target.split('?').next().unwrap_or(target);
        let candidate = dist.join(target);
        if let Ok(canon) = candidate.canonicalize() {
            if canon.starts_with(dist) && canon.is_file() {
                return serve_file(&canon);
            }
        }
    }
    // Fallback: "/" já é handled; qualquer outra rota → 404.
    if path == "/" {
        return html_response(&st.html);
    }
    not_found()
}

fn serve_file(path: &Path) -> Response {
    let Ok(bytes) = std::fs::read(path) else {
        return not_found();
    };
    let mime = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let cache = if ext == "html" {
        "no-cache"
    } else {
        "public, max-age=3600"
    };
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_str(&mime).unwrap());
    headers.insert("cache-control", HeaderValue::from_static(""));
    if let Ok(v) = HeaderValue::from_str(cache) {
        headers.insert("cache-control", v);
    }
    (StatusCode::OK, headers, bytes).into_response()
}

fn html_response(html: &str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert("cache-control", HeaderValue::from_static("no-cache"));
    (StatusCode::OK, headers, html.to_string()).into_response()
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "not found").into_response()
}

// ── /api/network-config ─────────────────────────────────────────────────────

async fn get_network(State(st): State<AppState>) -> Response {
    let cfg = st.network_cfg.lock().unwrap().clone();
    let lan_ips: Vec<Value> = list_lan_ips()
        .into_iter()
        .map(|ip| json!({ "iface": ip.iface, "address": ip.address }))
        .collect();
    let mut out = Map::new();
    out.insert("bind".into(), Value::String(cfg.bind));
    out.insert("customHost".into(), Value::String(cfg.custom_host));
    out.insert("activeHost".into(), Value::String(st.bind_host.clone()));
    out.insert("port".into(), Value::from(st.port));
    out.insert("lanIps".into(), Value::Array(lan_ips));
    json_response(StatusCode::OK, Value::Object(out))
}

async fn put_network(State(st): State<AppState>, body: Bytes) -> Response {
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let bind = parsed
        .get("bind")
        .and_then(Value::as_str)
        .filter(|s| VALID_BINDS.contains(s));
    let Some(bind) = bind else {
        return json_error(
            StatusCode::BAD_REQUEST,
            "bind inválido (localhost|lan|custom)",
        );
    };
    let custom_host = parsed
        .get("customHost")
        .and_then(Value::as_str)
        .map(|s| {
            s.trim()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
                .collect::<String>()
        })
        .unwrap_or_default();
    if bind == "custom" && custom_host.is_empty() {
        return json_error(
            StatusCode::BAD_REQUEST,
            "customHost é obrigatório quando bind=custom",
        );
    }
    let next = NetworkConfig {
        bind: bind.to_string(),
        custom_host,
    };
    save_network_config(&st.mem_dir, &next);
    {
        let mut lock = st.network_cfg.lock().unwrap();
        *lock = next.clone();
    }
    let mut out = Map::new();
    out.insert("bind".into(), Value::String(next.bind));
    out.insert("customHost".into(), Value::String(next.custom_host));
    out.insert("activeHost".into(), Value::String(st.bind_host.clone()));
    out.insert("port".into(), Value::from(st.port));
    out.insert("restart_required".into(), Value::Bool(true));
    out.insert(
        "note".into(),
        Value::String("reinicie o servidor pra aplicar".into()),
    );
    json_response(StatusCode::OK, Value::Object(out))
}

// ── /api/bus/* ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct BusStateQuery {
    recent: Option<String>,
}

async fn bus_state(State(st): State<AppState>, Query(q): Query<BusStateQuery>) -> Response {
    // Paridade com `parseInt(..., 10) || 50` do JS: NaN/0 → 50 sem o
    // `||` seria difícil — usamos fallback explícito.
    let recent = q
        .recent
        .as_deref()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|n| *n > 0)
        .map(|n| n as usize)
        .unwrap_or(50);
    let state = get_state(
        &st.mem_dir,
        &GetStateOpts {
            recent,
            since_days: None,
        },
        &*st.clock,
    );
    // `{ ...state, queue: autoQueue.stats() }` do `.cjs`. Em F4-05a não
    // temos autoQueue ainda — injetamos shape vazio compatível.
    let mut out = match state {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    out.insert(
        "queue".into(),
        json!({ "running": 0, "pending": 0, "agents": {}, "rateMax": 0 }),
    );
    json_response(StatusCode::OK, Value::Object(out))
}

#[derive(Debug, Deserialize)]
struct BusTraceQuery {
    trace_id: Option<String>,
}

async fn bus_trace(State(st): State<AppState>, Query(q): Query<BusTraceQuery>) -> Response {
    let Some(tid) = q.trace_id.as_deref().filter(|s| !s.is_empty()) else {
        return json_error(StatusCode::BAD_REQUEST, "missing trace_id");
    };
    match get_message(&st.mem_dir, tid, &*st.clock) {
        Some(summary) => {
            let mut out = Map::new();
            out.insert("created".into(), summary.created.unwrap_or(Value::Null));
            out.insert("latest".into(), summary.latest);
            out.insert("attempts".into(), Value::from(summary.attempts));
            out.insert("events".into(), Value::Array(summary.events));
            json_response(StatusCode::OK, Value::Object(out))
        }
        None => json_error(StatusCode::NOT_FOUND, "trace not found"),
    }
}

async fn bus_config_get(State(st): State<AppState>) -> Response {
    let mut out = get_config(&st.mem_dir);
    out.insert("min_retention".into(), Value::from(MIN_RETENTION));
    out.insert("max_retention".into(), Value::from(MAX_RETENTION));
    out.insert("default_retention".into(), Value::from(DEFAULT_RETENTION));
    out.insert(
        "day_files".into(),
        Value::Array(
            list_day_files(&st.mem_dir)
                .into_iter()
                .map(Value::String)
                .collect(),
        ),
    );
    json_response(StatusCode::OK, Value::Object(out))
}

async fn bus_config_put(State(st): State<AppState>, body: Bytes) -> Response {
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let updated = match set_config(&st.mem_dir, &parsed) {
        Ok(u) => u,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let retention = updated
        .get("retention_days")
        .and_then(Value::as_i64)
        .map(|i| i as f64);
    let compact = compact_old_files(
        &st.mem_dir,
        &CompactOpts {
            retention_days: retention,
            now: None,
        },
        &*st.clock,
    );
    let mut out = updated;
    out.insert("compact".into(), compact_result_to_json(&compact));
    json_response(StatusCode::OK, Value::Object(out))
}

async fn bus_compact(State(st): State<AppState>) -> Response {
    let compact = compact_old_files(
        &st.mem_dir,
        &CompactOpts {
            retention_days: None,
            now: None,
        },
        &*st.clock,
    );
    json_response(StatusCode::OK, compact_result_to_json(&compact))
}

fn compact_result_to_json(r: &domain_bus::CompactResult) -> Value {
    let mut m = Map::new();
    m.insert(
        "deleted".into(),
        Value::Array(r.deleted.iter().cloned().map(Value::String).collect()),
    );
    m.insert(
        "kept".into(),
        Value::Array(r.kept.iter().cloned().map(Value::String).collect()),
    );
    m.insert("retention_days".into(), Value::from(r.retention_days));
    m.insert("cutoff".into(), Value::String(r.cutoff.clone()));
    Value::Object(m)
}

// ── Helpers de response ─────────────────────────────────────────────────────

pub(crate) fn json_response(status: StatusCode, value: Value) -> Response {
    let body = serde_json::to_vec(&value).unwrap_or_default();
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    (status, headers, body).into_response()
}

pub(crate) fn json_error(status: StatusCode, msg: &str) -> Response {
    json_response(status, json!({ "error": msg }))
}

// ── Socket binding ──────────────────────────────────────────────────────────

/// Retorna `SocketAddr` para `.listen(port, host)` do `.cjs`.
///
/// Host "localhost" → `127.0.0.1`; "0.0.0.0" literal; ou IP arbitrário.
pub fn socket_addr_for(host: &str, port: u16) -> Option<SocketAddr> {
    let host = match host {
        "localhost" => "127.0.0.1",
        other => other,
    };
    host.parse::<IpAddr>()
        .ok()
        .map(|ip| SocketAddr::new(ip, port))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn td() -> TempDir {
        TempDir::new().unwrap()
    }

    #[test]
    fn load_missing_returns_default() {
        let t = td();
        let cfg = load_network_config(t.path());
        assert_eq!(cfg, NetworkConfig::default());
    }

    #[test]
    fn roundtrip_network_cfg() {
        let t = td();
        let cfg = NetworkConfig {
            bind: "lan".into(),
            custom_host: String::new(),
        };
        save_network_config(t.path(), &cfg);
        assert_eq!(load_network_config(t.path()), cfg);
    }

    #[test]
    fn invalid_bind_falls_back_to_localhost() {
        let t = td();
        std::fs::write(
            network_cfg_path(t.path()),
            r#"{"bind":"weird","customHost":"x"}"#,
        )
        .unwrap();
        let cfg = load_network_config(t.path());
        assert_eq!(cfg.bind, "localhost");
    }

    #[test]
    fn resolve_host_matrix() {
        assert_eq!(
            resolve_bind_host(&NetworkConfig {
                bind: "localhost".into(),
                custom_host: "".into()
            }),
            "127.0.0.1"
        );
        assert_eq!(
            resolve_bind_host(&NetworkConfig {
                bind: "lan".into(),
                custom_host: "".into()
            }),
            "0.0.0.0"
        );
        assert_eq!(
            resolve_bind_host(&NetworkConfig {
                bind: "custom".into(),
                custom_host: "10.0.0.1".into()
            }),
            "10.0.0.1"
        );
        assert_eq!(
            resolve_bind_host(&NetworkConfig {
                bind: "custom".into(),
                custom_host: "".into()
            }),
            "127.0.0.1"
        );
    }

    #[test]
    fn socket_addr_parses() {
        assert!(socket_addr_for("127.0.0.1", 3000).is_some());
        assert!(socket_addr_for("localhost", 3000).is_some());
        assert!(socket_addr_for("0.0.0.0", 3000).is_some());
        assert!(socket_addr_for("nothost", 3000).is_none());
    }

    #[test]
    fn list_lan_ips_does_not_panic() {
        let _ = list_lan_ips();
    }
}
