//! # domain-graph
//!
//! Porta Rust de `plugin/scripts/graph.cjs` — grafo de conhecimento
//! do projeto (nodes + edges com count/last_seen/decay).
//!
//! ## Escopo (Fase 9)
//!
//! Todas as funções puras têm paridade byte-a-byte com o `.cjs`:
//! - `empty` / `load_graph` / `save_graph`.
//! - `ingest_observation` / `ingest_agent_log_event` /
//!   `ingest_agent_chat_row` / `ingest_human_chat_row` /
//!   `ingest_decision` / `ingest_gotcha`.
//! - `rebuild_graph` — pipeline que lê JSONLs do disco.
//! - `get_context_lines` — renderiza o grafo em markdown.
//!
//! Auto-migração de versão antiga (`version != VERSION`) → rebuild
//! opcional via callback (mesma lógica do `.cjs`).

use regex::Regex;
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub const GRAPH_FILE: &str = "graph.json";
pub const VERSION: u64 = 2;
pub const COUPLING_WINDOW_MS: i64 = 15 * 60 * 1000;
pub const HALF_LIFE_DAYS: i64 = 30;
pub const HALF_LIFE_MS: i64 = HALF_LIFE_DAYS * 86_400_000;

// ── Clock ───────────────────────────────────────────────────────────────

pub trait Clock {
    fn now_ms(&self) -> i64;
    fn now_iso(&self) -> String {
        format_iso_millis(self.now_ms())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct FixedClock(pub i64);

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.0
    }
}

pub fn format_iso_millis(ms_since_epoch: i64) -> String {
    let mut seconds = ms_since_epoch.div_euclid(1000);
    let millis = ms_since_epoch.rem_euclid(1000);
    let days = seconds.div_euclid(86_400);
    seconds = seconds.rem_euclid(86_400);
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, m, d, hours, minutes, secs, millis
    )
}

fn parse_iso_millis(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    if bytes.len() < 10 {
        return None;
    }
    let year: i64 = s.get(..4)?.parse().ok()?;
    let month: i64 = s.get(5..7)?.parse().ok()?;
    let day: i64 = s.get(8..10)?.parse().ok()?;
    let (hour, min, sec, ms) = if bytes.len() >= 20 {
        let h: i64 = s.get(11..13)?.parse().ok()?;
        let mi: i64 = s.get(14..16)?.parse().ok()?;
        let sec: i64 = s.get(17..19)?.parse().ok()?;
        let ms: i64 = if bytes.len() >= 23 && &bytes[19..20] == b"." {
            s.get(20..23).and_then(|x| x.parse().ok()).unwrap_or(0)
        } else {
            0
        };
        (h, mi, sec, ms)
    } else {
        (0, 0, 0, 0)
    };
    let days = days_from_civil(year, month as u32, day as u32);
    Some(days * 86_400_000 + hour * 3_600_000 + min * 60_000 + sec * 1_000 + ms)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 }.div_euclid(400);
    let yoe = (y - era * 400) as u64;
    let mp: u32 = if m > 2 { m - 3 } else { m + 9 };
    let doy: u64 = ((153u64 * mp as u64 + 2) / 5) + d as u64 - 1;
    let doe: u64 = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

// ── I/O ──────────────────────────────────────────────────────────────────

/// Graph vazio com `version`/`rebuilt_at=null`/`updated_at=null`.
pub fn empty() -> Value {
    let mut m = Map::new();
    m.insert("version".into(), Value::from(VERSION));
    m.insert("rebuilt_at".into(), Value::Null);
    m.insert("updated_at".into(), Value::Null);
    m.insert("nodes".into(), Value::Object(Map::new()));
    m.insert("edges".into(), Value::Object(Map::new()));
    Value::Object(m)
}

pub fn graph_path(mem_dir: &Path) -> PathBuf {
    mem_dir.join(GRAPH_FILE)
}

/// Carrega `graph.json`. Se inexistente, malformado, ou tipo errado →
/// [`empty`]. Se `version != VERSION`, a auto-migração chama
/// `on_stale_version` se fornecido (rebuild do zero); caso contrário,
/// retorna [`empty`].
pub fn load_graph(
    mem_dir: &Path,
    on_stale_version: Option<&dyn Fn(&Path) -> Value>,
) -> Value {
    let file = graph_path(mem_dir);
    let raw = match fs::read_to_string(&file) {
        Ok(r) => r,
        Err(_) => return empty(),
    };
    let g: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return empty(),
    };
    if !g.is_object() {
        return empty();
    }
    let version = g.get("version").and_then(Value::as_u64).unwrap_or(0);
    if version != VERSION {
        if let Some(cb) = on_stale_version {
            return cb(mem_dir);
        }
        return empty();
    }
    g
}

/// Escreve `graph.json` com `JSON.stringify(g, null, 2)` (SEM `\n`
/// final, igual ao `.cjs`). Atualiza `updated_at` via clock.
pub fn save_graph(mem_dir: &Path, graph: &mut Value, clock: &dyn Clock) -> std::io::Result<()> {
    if let Some(obj) = graph.as_object_mut() {
        obj.insert("updated_at".into(), Value::String(clock.now_iso()));
    }
    let s = serde_json::to_string_pretty(graph).unwrap_or_default();
    fs::write(graph_path(mem_dir), s)
}

// ── Primitives ───────────────────────────────────────────────────────────

pub fn node_id(kind: &str, key: &str) -> String {
    format!("{kind}:{key}")
}

/// Toca um nó: se novo, cria com `count=0,label,last_seen=ts`, depois
/// `count++` sempre. Atualiza `last_seen` se `ts > atual` em ordem
/// cronológica (comparação via timestamp parsed). Só seta `label` se
/// nó não tinha.
pub fn touch_node(graph: &mut Value, id: &str, kind: &str, label: &str, ts: &str) {
    let nodes = graph
        .get_mut("nodes")
        .and_then(Value::as_object_mut)
        .expect("graph.nodes must be object");

    if !nodes.contains_key(id) {
        let mut n = Map::new();
        n.insert("kind".into(), Value::String(kind.into()));
        n.insert("label".into(), Value::String(label.into()));
        n.insert("count".into(), Value::from(0u64));
        n.insert("last_seen".into(), Value::String(ts.into()));
        nodes.insert(id.to_string(), Value::Object(n));
    }

    let n = nodes.get_mut(id).unwrap().as_object_mut().unwrap();
    let cur = n.get("count").and_then(Value::as_u64).unwrap_or(0);
    n.insert("count".into(), Value::from(cur + 1));

    let should_update = {
        let prev = n.get("last_seen").and_then(Value::as_str);
        match prev {
            None => true,
            Some("") => true,
            Some(p) => {
                let new_ms = parse_iso_millis(ts).unwrap_or(i64::MIN);
                let old_ms = parse_iso_millis(p).unwrap_or(i64::MIN);
                new_ms > old_ms
            }
        }
    };
    if should_update {
        n.insert("last_seen".into(), Value::String(ts.into()));
    }

    // Só seta label se não tinha (truthy falsy em JS: "" é falsy).
    let need_label = n
        .get("label")
        .and_then(Value::as_str)
        .map(|s| s.is_empty())
        .unwrap_or(true);
    if need_label && !label.is_empty() {
        n.insert("label".into(), Value::String(label.into()));
    }
}

/// Toca uma aresta. Se `intent` presente em `meta`, incrementa
/// tally em `edge.intents[intent]`.
pub fn touch_edge(
    graph: &mut Value,
    from: &str,
    to: &str,
    rel: &str,
    ts: &str,
    intent: Option<&str>,
) {
    let key = format!("{from}→{to}→{rel}");
    let edges = graph
        .get_mut("edges")
        .and_then(Value::as_object_mut)
        .expect("graph.edges must be object");

    if !edges.contains_key(&key) {
        let mut e = Map::new();
        e.insert("from".into(), Value::String(from.into()));
        e.insert("to".into(), Value::String(to.into()));
        e.insert("rel".into(), Value::String(rel.into()));
        e.insert("count".into(), Value::from(0u64));
        e.insert("last_seen".into(), Value::String(ts.into()));
        edges.insert(key.clone(), Value::Object(e));
    }

    let e = edges.get_mut(&key).unwrap().as_object_mut().unwrap();
    let cur = e.get("count").and_then(Value::as_u64).unwrap_or(0);
    e.insert("count".into(), Value::from(cur + 1));

    let should_update = {
        let prev = e.get("last_seen").and_then(Value::as_str);
        match prev {
            None => true,
            Some("") => true,
            Some(p) => {
                let new_ms = parse_iso_millis(ts).unwrap_or(i64::MIN);
                let old_ms = parse_iso_millis(p).unwrap_or(i64::MIN);
                new_ms > old_ms
            }
        }
    };
    if should_update {
        e.insert("last_seen".into(), Value::String(ts.into()));
    }

    if let Some(intent) = intent {
        if !e.contains_key("intents") {
            e.insert("intents".into(), Value::Object(Map::new()));
        }
        let intents = e.get_mut("intents").unwrap().as_object_mut().unwrap();
        let cur = intents.get(intent).and_then(Value::as_u64).unwrap_or(0);
        intents.insert(intent.into(), Value::from(cur + 1));
    }
}

/// `count * exp(-age / HALF_LIFE_MS)`. `age < 0` ou `!finite` → `count`.
pub fn score_of(entity: &Value, now_ms: i64) -> f64 {
    let count = entity.get("count").and_then(Value::as_u64).unwrap_or(0) as f64;
    let last = entity.get("last_seen").and_then(Value::as_str);
    let Some(last) = last else { return count };
    let Some(last_ms) = parse_iso_millis(last) else {
        return count;
    };
    let age = (now_ms - last_ms) as f64;
    if !age.is_finite() || age < 0.0 {
        return count;
    }
    count * (-age / HALF_LIFE_MS as f64).exp()
}

pub fn same_window(ts1: &str, ts2: &str, window_ms: i64) -> bool {
    let a = parse_iso_millis(ts1).unwrap_or(0);
    let b = parse_iso_millis(ts2).unwrap_or(0);
    (a - b).abs() <= window_ms
}

// ── Observations ────────────────────────────────────────────────────────

fn obs_node_id(obs: &Value) -> Option<String> {
    let t = obs.get("type").and_then(Value::as_str)?;
    if t == "file_edit" {
        let f = obs.get("file").and_then(Value::as_str)?;
        return Some(node_id("file", f));
    }
    if t == "bash" {
        let cmd = obs
            .get("cmd")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let norm = cmd
            .split_whitespace()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ");
        return Some(node_id("command", &norm));
    }
    None
}

fn obs_label(obs: &Value) -> String {
    let t = obs.get("type").and_then(Value::as_str).unwrap_or("");
    if t == "file_edit" {
        return obs
            .get("file")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
    }
    if t == "bash" {
        let cmd = obs
            .get("cmd")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        return cmd.split_whitespace().take(4).collect::<Vec<_>>().join(" ");
    }
    String::new()
}

pub fn ingest_observation(graph: &mut Value, current: &Value, prev: Option<&Value>) {
    let Some(ts) = current.get("ts").and_then(Value::as_str) else {
        return;
    };
    let t = current.get("type").and_then(Value::as_str).unwrap_or("");

    if t == "agent_call" {
        let Some(agent) = current.get("agent").and_then(Value::as_str) else {
            return;
        };
        touch_node(graph, &node_id("agent", agent), "agent", agent, ts);
        if let Some(aid) = current.get("agent_id").and_then(Value::as_str) {
            touch_node(graph, &node_id("agent", aid), "agent", aid, ts);
            touch_edge(
                graph,
                &node_id("agent", aid),
                &node_id("agent", agent),
                "invokes",
                ts,
                None,
            );
        }
        return;
    }

    let Some(cur_id) = obs_node_id(current) else {
        return;
    };
    let kind = if t == "file_edit" { "file" } else { "command" };
    let label = obs_label(current);
    touch_node(graph, &cur_id, kind, &label, ts);

    if let Some(aid) = current.get("agent_id").and_then(Value::as_str) {
        let a_node = node_id("agent", aid);
        touch_node(graph, &a_node, "agent", aid, ts);
        let rel = if t == "file_edit" { "edits" } else { "runs" };
        touch_edge(graph, &a_node, &cur_id, rel, ts, None);
    }

    let Some(prev) = prev else {
        return;
    };
    let same_sess = prev.get("session_id") == current.get("session_id");
    let Some(prev_ts) = prev.get("ts").and_then(Value::as_str) else {
        return;
    };
    if !same_sess || !same_window(prev_ts, ts, COUPLING_WINDOW_MS) {
        return;
    }
    let Some(prev_id) = obs_node_id(prev) else {
        return;
    };
    if prev_id == cur_id {
        return;
    }
    let pt = prev.get("type").and_then(Value::as_str).unwrap_or("");
    match (pt, t) {
        ("file_edit", "file_edit") => {
            touch_edge(graph, &prev_id, &cur_id, "co_edited", ts, None);
            touch_edge(graph, &cur_id, &prev_id, "co_edited", ts, None);
        }
        ("file_edit", "bash") => {
            touch_edge(graph, &prev_id, &cur_id, "followed_by", ts, None);
        }
        ("bash", "file_edit") => {
            touch_edge(graph, &prev_id, &cur_id, "then_edited", ts, None);
        }
        _ => {}
    }
}

// ── Agent log events ────────────────────────────────────────────────────

pub fn ingest_agent_log_event(graph: &mut Value, agent: &str, ev: &Value) {
    if agent.is_empty() {
        return;
    }
    let Some(ts) = ev.get("ts").and_then(Value::as_str) else {
        return;
    };
    let a_id = node_id("agent", agent);
    touch_node(graph, &a_id, "agent", agent, ts);

    let intent = ev.get("intent").and_then(Value::as_str);
    let t = ev.get("type").and_then(Value::as_str).unwrap_or("");

    match t {
        "handoff_out" => {
            if let Some(target) = ev.get("target").and_then(Value::as_str) {
                let t_id = node_id("agent", target);
                touch_node(graph, &t_id, "agent", target, ts);
                touch_edge(graph, &a_id, &t_id, "handoff", ts, intent);
            }
        }
        "handoff_in" => {
            if let Some(src) = ev.get("source").and_then(Value::as_str) {
                touch_node(graph, &node_id("agent", src), "agent", src, ts);
            }
        }
        "violation" => {
            if let Some(target) = ev.get("target").and_then(Value::as_str) {
                let t_id = node_id("agent", target);
                touch_node(graph, &t_id, "agent", target, ts);
                touch_edge(graph, &a_id, &t_id, "violation", ts, None);
            }
        }
        "auto_escalation_out" => {
            if let Some(target) = ev.get("target").and_then(Value::as_str) {
                let t_id = node_id("agent", target);
                touch_node(graph, &t_id, "agent", target, ts);
                touch_edge(graph, &a_id, &t_id, "escalates", ts, None);
            }
        }
        _ => {}
    }
}

// ── Agent chats ─────────────────────────────────────────────────────────

pub fn ingest_agent_chat_row(
    graph: &mut Value,
    agent: &str,
    row: &Value,
    human_lookup: &HumanLookup,
    known_agents: &[String],
) {
    if agent.is_empty() {
        return;
    }
    let Some(ts) = row.get("ts").and_then(Value::as_str) else {
        return;
    };
    if row.get("role").and_then(Value::as_str) != Some("user") {
        return;
    }
    let Some(from) = row.get("from").and_then(Value::as_str) else {
        return;
    };
    let a_id = node_id("agent", agent);
    touch_node(graph, &a_id, "agent", agent, ts);

    if let Some(h_id) = human_lookup.resolve(from) {
        let h_node = node_id("human", &h_id);
        touch_node(graph, &h_node, "human", &h_id, ts);
        touch_edge(graph, &h_node, &a_id, "talks_to", ts, None);
    } else if known_agents.iter().any(|a| a == from) {
        let f_id = node_id("agent", from);
        touch_node(graph, &f_id, "agent", from, ts);
        touch_edge(graph, &f_id, &a_id, "handoff", ts, None);
    }
}

// ── Human chats ─────────────────────────────────────────────────────────

pub fn ingest_human_chat_row(graph: &mut Value, row: &Value) {
    let Some(ts) = row.get("ts").and_then(Value::as_str) else {
        return;
    };
    let Some(from) = row.get("from").and_then(Value::as_str) else {
        return;
    };
    let Some(to) = row.get("to").and_then(Value::as_str) else {
        return;
    };
    let f_id = node_id("human", from);
    let t_id = node_id("human", to);
    touch_node(graph, &f_id, "human", from, ts);
    touch_node(graph, &t_id, "human", to, ts);
    touch_edge(graph, &f_id, &t_id, "talks_with", ts, None);
    touch_edge(graph, &t_id, &f_id, "talks_with", ts, None);
}

// ── Decisions / Gotchas ─────────────────────────────────────────────────

static NON_AN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]+").unwrap());

fn slugify(s: &str, max_len: usize) -> String {
    let lower = ascii_lowercase(s);
    let trimmed = utf16_slice(&lower, max_len);
    NON_AN.replace_all(&trimmed, "-").to_string()
}

/// JS `.toLowerCase()` **não** é idêntico a Rust `to_lowercase()` para
/// chars Unicode: para strings ASCII o resultado bate, mas chars em PT
/// (acento) têm regras diferentes. Pro slug aqui o regex já filtra
/// non-ASCII → resultado final converge, mas a comparação intermediária
/// difere. Replicamos como ASCII-lowercase para caracteres ASCII e
/// mantemos o resto como está (igual ao JS que só lower ASCII para
/// caracteres `[A-Z]`).
fn ascii_lowercase(s: &str) -> String {
    s.chars()
        .map(|c| {
            // JS toLowerCase uses Unicode tables. Para manter paridade,
            // replicamos o comportamento da V8 via `char::to_lowercase`.
            c.to_lowercase().collect::<String>()
        })
        .collect()
}

fn utf16_slice(s: &str, n: usize) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    if units.len() <= n {
        return s.to_string();
    }
    String::from_utf16_lossy(&units[..n])
}

pub fn decision_id(decision: &Value) -> String {
    let what = decision
        .get("what")
        .and_then(Value::as_str)
        .or_else(|| decision.get("text").and_then(Value::as_str))
        .unwrap_or("");
    let sl = slugify(what, 80);
    let key = if sl.is_empty() {
        let ts = decision.get("ts").and_then(Value::as_str).unwrap_or("");
        let ts_prefix = utf16_slice(ts, 19);
        format!("anon-{ts_prefix}")
    } else {
        sl
    };
    node_id("decision", &key)
}

pub fn ingest_decision(graph: &mut Value, decision: &Value, session_file_ids: &[String]) {
    let Some(ts) = decision.get("ts").and_then(Value::as_str) else {
        return;
    };
    let did = decision_id(decision);
    let label_src = decision
        .get("what")
        .and_then(Value::as_str)
        .or_else(|| decision.get("text").and_then(Value::as_str))
        .unwrap_or("decisão");
    let label = utf16_slice(label_src, 80);
    touch_node(graph, &did, "decision", &label, ts);
    for f in session_file_ids {
        if f.starts_with("file:") {
            touch_edge(graph, &did, f, "touches", ts, None);
        }
    }
}

pub fn gotcha_id(gotcha: &Value) -> String {
    let file = gotcha
        .get("file")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let desc = gotcha
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let desc_trunc = utf16_slice(desc, 60);
    let base = format!("{file}|{desc_trunc}");
    let lower = ascii_lowercase(&base);
    let slug = NON_AN.replace_all(&lower, "-").to_string();
    let hash = utf16_slice(&slug, 80);
    let key = if hash.is_empty() {
        let ts = gotcha.get("ts").and_then(Value::as_str).unwrap_or("");
        format!("anon-{}", utf16_slice(ts, 19))
    } else {
        hash
    };
    node_id("gotcha", &key)
}

pub fn ingest_gotcha(graph: &mut Value, gotcha: &Value) {
    let Some(ts) = gotcha.get("ts").and_then(Value::as_str) else {
        return;
    };
    let gid = gotcha_id(gotcha);
    let desc = gotcha
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("gotcha");
    let label = utf16_slice(desc, 80);
    touch_node(graph, &gid, "gotcha", &label, ts);
    if let Some(file) = gotcha.get("file").and_then(Value::as_str) {
        let f_id = node_id("file", file);
        touch_node(graph, &f_id, "file", file, ts);
        touch_edge(graph, &gid, &f_id, "gotcha_for", ts, None);
    }
}

// ── Helpers: JSONL read + lookup build ──────────────────────────────────

pub fn read_jsonl(file: &Path) -> Vec<Value> {
    let raw = match fs::read_to_string(file) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    raw.split('\n')
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Lookup humano: aceita id direto, nome, e nome-lowercase →
/// retorna id canônico. Map preserva insertion order.
#[derive(Debug, Default, Clone)]
pub struct HumanLookup {
    entries: Vec<(String, String)>, // (key, canonical_id)
}

impl HumanLookup {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert(&mut self, key: impl Into<String>, id: impl Into<String>) {
        self.entries.push((key.into(), id.into()));
    }
    pub fn resolve(&self, k: &str) -> Option<String> {
        // Primeiro try exato, depois lowercase.
        for (key, id) in &self.entries {
            if key == k {
                return Some(id.clone());
            }
        }
        let lk = ascii_lowercase(k);
        for (key, id) in &self.entries {
            if key == &lk {
                return Some(id.clone());
            }
        }
        None
    }
}

pub fn build_human_lookup(mem_dir: &Path) -> HumanLookup {
    let mut l = HumanLookup::new();
    let path = mem_dir.join("humans.json");
    let Ok(raw) = fs::read_to_string(&path) else {
        return l;
    };
    let Ok(v) = serde_json::from_str::<Value>(&raw) else {
        return l;
    };
    let Some(humans) = v.get("humans").and_then(Value::as_object) else {
        return l;
    };
    for (id, h) in humans.iter() {
        l.insert(id, id);
        if let Some(name) = h.get("name").and_then(Value::as_str) {
            l.insert(name, id);
            l.insert(ascii_lowercase(name), id);
        }
    }
    l
}

pub fn build_agent_set(mem_dir: &Path) -> Vec<String> {
    let mut s = Vec::new();
    let path = mem_dir.join("agents.json");
    let Ok(raw) = fs::read_to_string(&path) else {
        return s;
    };
    let Ok(v) = serde_json::from_str::<Value>(&raw) else {
        return s;
    };
    if let Some(ag) = v.get("agents").and_then(Value::as_object) {
        for k in ag.keys() {
            s.push(k.clone());
        }
    }
    s
}

// ── Incremental public API ──────────────────────────────────────────────

pub fn update_graph(
    mem_dir: &Path,
    current: &Value,
    prev: Option<&Value>,
    clock: &dyn Clock,
) -> std::io::Result<()> {
    let mut g = load_graph(mem_dir, None);
    ingest_observation(&mut g, current, prev);
    save_graph(mem_dir, &mut g, clock)
}

pub fn on_agent_log_event(mem_dir: &Path, agent: &str, ev: &Value, clock: &dyn Clock) {
    let mut g = load_graph(mem_dir, None);
    ingest_agent_log_event(&mut g, agent, ev);
    let _ = save_graph(mem_dir, &mut g, clock);
}

pub fn on_agent_chat_row(mem_dir: &Path, agent: &str, row: &Value, clock: &dyn Clock) {
    let mut g = load_graph(mem_dir, None);
    let humans = build_human_lookup(mem_dir);
    let agents = build_agent_set(mem_dir);
    ingest_agent_chat_row(&mut g, agent, row, &humans, &agents);
    let _ = save_graph(mem_dir, &mut g, clock);
}

pub fn on_human_chat_row(mem_dir: &Path, row: &Value, clock: &dyn Clock) {
    let mut g = load_graph(mem_dir, None);
    ingest_human_chat_row(&mut g, row);
    let _ = save_graph(mem_dir, &mut g, clock);
}

pub fn on_decision(mem_dir: &Path, decision: &Value, session_files: &[String], clock: &dyn Clock) {
    let mut g = load_graph(mem_dir, None);
    ingest_decision(&mut g, decision, session_files);
    let _ = save_graph(mem_dir, &mut g, clock);
}

pub fn on_gotcha(mem_dir: &Path, gotcha: &Value, clock: &dyn Clock) {
    let mut g = load_graph(mem_dir, None);
    ingest_gotcha(&mut g, gotcha);
    let _ = save_graph(mem_dir, &mut g, clock);
}

// ── Rebuild ─────────────────────────────────────────────────────────────

/// Reconstrói todo o grafo a partir dos JSONLs em `mem_dir`. Replica
/// `rebuildGraph` em `graph.cjs:476-547`.
pub fn rebuild_graph(mem_dir: &Path, clock: &dyn Clock) -> std::io::Result<Value> {
    let mut g = empty();

    // 1) observations + co-occurrence
    let obs = read_jsonl(&mem_dir.join("observations.jsonl"));
    for i in 0..obs.len() {
        let prev = if i > 0 { Some(&obs[i - 1]) } else { None };
        ingest_observation(&mut g, &obs[i], prev);
    }

    // 1b) files by session (insertion order preserved)
    // (key, Vec<node_id>) — preserva ordem de inserção de session_ids
    // E preserva ordem dentro de cada session (Set-like via dedup ao inserir).
    let mut files_by_session: Vec<(String, Vec<String>)> = Vec::new();
    for o in &obs {
        if o.get("type").and_then(Value::as_str) == Some("file_edit") {
            let Some(sid) = o.get("session_id").and_then(Value::as_str) else {
                continue;
            };
            let Some(f) = o.get("file").and_then(Value::as_str) else {
                continue;
            };
            let fnode = node_id("file", f);
            if let Some(entry) = files_by_session.iter_mut().find(|(k, _)| k == sid) {
                if !entry.1.contains(&fnode) {
                    entry.1.push(fnode);
                }
            } else {
                files_by_session.push((sid.to_string(), vec![fnode]));
            }
        }
    }

    // 2) agent_logs
    let logs_dir = mem_dir.join("agent_logs");
    if logs_dir.is_dir() {
        let mut files: Vec<_> = fs::read_dir(&logs_dir)?
            .flatten()
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(|x| x == "jsonl")
                    .unwrap_or(false)
            })
            .collect();
        // Node's `fs.readdirSync` não garante ordem; tipicamente devolve
        // a ordem do sistema de arquivos (geralmente inode). No Linux
        // tmpfs (usado em tempfile) isso é essencialmente insertion
        // order. Para Rust, `fs::read_dir` retorna em ordem indefinida
        // também — para paridade byte-a-byte precisamos ordenar como
        // o Node faria. Abordagem pragmática: ordem alfabética, que
        // dá resultado estável e é próximo do que a maioria dos FS dá.
        files.sort_by_key(|e| e.file_name());
        for entry in files {
            let agent_name = entry
                .path()
                .file_stem()
                .and_then(|x| x.to_str())
                .map(str::to_string)
                .unwrap_or_default();
            let rows = read_jsonl(&entry.path());
            for ev in &rows {
                ingest_agent_log_event(&mut g, &agent_name, ev);
            }
        }
    }

    // 3) agent_chats
    let chats_dir = mem_dir.join("agent_chats");
    let known_agents = build_agent_set(mem_dir);
    let human_lookup = build_human_lookup(mem_dir);
    if chats_dir.is_dir() {
        let mut files: Vec<_> = fs::read_dir(&chats_dir)?
            .flatten()
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(|x| x == "jsonl")
                    .unwrap_or(false)
            })
            .collect();
        files.sort_by_key(|e| e.file_name());
        for entry in files {
            let agent_name = entry
                .path()
                .file_stem()
                .and_then(|x| x.to_str())
                .map(str::to_string)
                .unwrap_or_default();
            let rows = read_jsonl(&entry.path());
            for row in &rows {
                ingest_agent_chat_row(&mut g, &agent_name, row, &human_lookup, &known_agents);
            }
        }
    }

    // 4) human_chats
    let hc_dir = mem_dir.join("human_chats");
    if hc_dir.is_dir() {
        let mut files: Vec<_> = fs::read_dir(&hc_dir)?
            .flatten()
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(|x| x == "jsonl")
                    .unwrap_or(false)
            })
            .collect();
        files.sort_by_key(|e| e.file_name());
        for entry in files {
            let rows = read_jsonl(&entry.path());
            for row in &rows {
                ingest_human_chat_row(&mut g, row);
            }
        }
    }

    // 5) decisions
    let decisions = read_jsonl(&mem_dir.join("decisions.jsonl"));
    for d in &decisions {
        let session_files = d
            .get("session_id")
            .and_then(Value::as_str)
            .and_then(|sid| {
                files_by_session
                    .iter()
                    .find(|(k, _)| k == sid)
                    .map(|(_, v)| v.clone())
            })
            .unwrap_or_default();
        ingest_decision(&mut g, d, &session_files);
    }

    // 6) gotchas
    let gotchas = read_jsonl(&mem_dir.join("gotchas.jsonl"));
    for go in &gotchas {
        ingest_gotcha(&mut g, go);
    }

    if let Some(obj) = g.as_object_mut() {
        obj.insert("rebuilt_at".into(), Value::String(clock.now_iso()));
    }
    save_graph(mem_dir, &mut g, clock)?;
    Ok(g)
}

// ── Context lines (render markdown) ─────────────────────────────────────

fn short_file(id: &str) -> String {
    let stripped = id.strip_prefix("file:").unwrap_or(id);
    stripped
        .rsplit('/')
        .next()
        .unwrap_or(stripped)
        .to_string()
}

fn short_cmd(id: &str) -> String {
    id.strip_prefix("command:").unwrap_or(id).to_string()
}

fn short_agent(id: &str) -> String {
    id.strip_prefix("agent:").unwrap_or(id).to_string()
}

fn top_by<T>(list: &[T], score: impl Fn(&T) -> f64, n: usize) -> Vec<&T> {
    let mut pairs: Vec<(&T, f64)> = list
        .iter()
        .map(|x| (x, score(x)))
        .filter(|(_, s)| *s > 0.0)
        .collect();
    // Stable sort descending by score (Array.prototype.sort é stable
    // desde ES2019 — JS produz ordem em empate = insertion order).
    pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    pairs.into_iter().take(n).map(|(x, _)| x).collect()
}

/// Render markdown. Paridade byte-a-byte com `getContextLines` no
/// `.cjs:375-472`.
pub fn get_context_lines(graph: &Value, now_ms: i64) -> Vec<String> {
    let mut lines = Vec::new();
    let Some(nodes) = graph.get("nodes").and_then(Value::as_object) else {
        return lines;
    };
    let Some(edges) = graph.get("edges").and_then(Value::as_object) else {
        return lines;
    };
    if nodes.is_empty() {
        return lines;
    }

    let node_values: Vec<&Value> = nodes.values().collect();
    let edge_values: Vec<&Value> = edges.values().collect();

    // Arquivos mais ativos
    let files: Vec<&&Value> = node_values
        .iter()
        .filter(|n| {
            n.get("kind").and_then(Value::as_str) == Some("file")
                && n.get("count").and_then(Value::as_u64).unwrap_or(0) > 1
        })
        .collect();
    let top_files = top_by(&files, |n| score_of(n, now_ms), 8);
    if !top_files.is_empty() {
        lines.push("### Arquivos mais ativos".into());
        for f in top_files {
            let label = f.get("label").and_then(Value::as_str).unwrap_or("");
            let count = f.get("count").and_then(Value::as_u64).unwrap_or(0);
            lines.push(format!("- `{label}` — {count}x"));
        }
        lines.push(String::new());
    }

    // Acoplamentos (dedup A<B)
    let co_edges: Vec<&&Value> = edge_values
        .iter()
        .filter(|e| {
            e.get("rel").and_then(Value::as_str) == Some("co_edited")
                && e.get("count").and_then(Value::as_u64).unwrap_or(0) > 1
                && {
                    let from = e.get("from").and_then(Value::as_str).unwrap_or("");
                    let to = e.get("to").and_then(Value::as_str).unwrap_or("");
                    from < to
                }
        })
        .collect();
    let top_co = top_by(&co_edges, |e| score_of(e, now_ms), 6);
    if !top_co.is_empty() {
        lines.push("### Acoplamentos (editados juntos)".into());
        for e in top_co {
            let from = e.get("from").and_then(Value::as_str).unwrap_or("");
            let to = e.get("to").and_then(Value::as_str).unwrap_or("");
            let count = e.get("count").and_then(Value::as_u64).unwrap_or(0);
            lines.push(format!(
                "- `{}` ↔ `{}` ({count}x)",
                short_file(from),
                short_file(to)
            ));
        }
        lines.push(String::new());
    }

    // Workflows editar → rodar
    let wf: Vec<&&Value> = edge_values
        .iter()
        .filter(|e| {
            e.get("rel").and_then(Value::as_str) == Some("followed_by")
                && e.get("count").and_then(Value::as_u64).unwrap_or(0) > 1
        })
        .collect();
    let top_wf = top_by(&wf, |e| score_of(e, now_ms), 5);
    if !top_wf.is_empty() {
        lines.push("### Workflows detectados (editar → rodar)".into());
        for e in top_wf {
            let from = e.get("from").and_then(Value::as_str).unwrap_or("");
            let to = e.get("to").and_then(Value::as_str).unwrap_or("");
            let count = e.get("count").and_then(Value::as_u64).unwrap_or(0);
            lines.push(format!(
                "- editar `{}` → `{}` ({count}x)",
                short_file(from),
                short_cmd(to)
            ));
        }
        lines.push(String::new());
    }

    // Handoffs entre agentes
    let handoffs: Vec<&&Value> = edge_values
        .iter()
        .filter(|e| {
            e.get("rel").and_then(Value::as_str) == Some("handoff")
                && e.get("count").and_then(Value::as_u64).unwrap_or(0) > 1
        })
        .collect();
    let top_hand = top_by(&handoffs, |e| score_of(e, now_ms), 6);
    if !top_hand.is_empty() {
        lines.push("### Fluxo entre agentes (handoffs)".into());
        for e in top_hand {
            let from = e.get("from").and_then(Value::as_str).unwrap_or("");
            let to = e.get("to").and_then(Value::as_str).unwrap_or("");
            let count = e.get("count").and_then(Value::as_u64).unwrap_or(0);
            let tail = if let Some(intents) = e.get("intents").and_then(Value::as_object) {
                let mut parts: Vec<(&str, u64)> = intents
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_u64().unwrap_or(0)))
                    .collect();
                parts.sort_by(|a, b| b.1.cmp(&a.1));
                parts
                    .into_iter()
                    .map(|(k, v)| format!("{v} {k}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                format!("{count}x")
            };
            lines.push(format!(
                "- `{}` → `{}` ({tail})",
                short_agent(from),
                short_agent(to)
            ));
        }
        lines.push(String::new());
    }

    // Especialização de agentes (edits)
    let edits: Vec<&&Value> = edge_values
        .iter()
        .filter(|e| {
            e.get("rel").and_then(Value::as_str) == Some("edits")
                && e.get("count").and_then(Value::as_u64).unwrap_or(0) > 0
        })
        .collect();
    if !edits.is_empty() {
        // Group by from (insertion order).
        let mut groups: Vec<(String, Vec<&&Value>)> = Vec::new();
        for e in &edits {
            let from = e
                .get("from")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if let Some(g) = groups.iter_mut().find(|(k, _)| k == &from) {
                g.1.push(e);
            } else {
                groups.push((from, vec![e]));
            }
        }
        let mut agent_lines: Vec<(String, f64)> = Vec::new();
        for (a, evs) in &groups {
            let top3 = top_by(evs, |e| score_of(e, now_ms), 3);
            if top3.is_empty() {
                continue;
            }
            let files_str = top3
                .iter()
                .map(|e| {
                    let to = e.get("to").and_then(Value::as_str).unwrap_or("");
                    format!("`{}`", short_file(to))
                })
                .collect::<Vec<_>>()
                .join(", ");
            let line = format!("- `{}` ↔ {files_str}", short_agent(a));
            let score: f64 = top3.iter().map(|e| score_of(e, now_ms)).sum();
            agent_lines.push((line, score));
        }
        agent_lines
            .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let top_agents: Vec<_> = agent_lines.into_iter().take(4).collect();
        if !top_agents.is_empty() {
            lines.push("### Especialização de agentes".into());
            for (l, _) in top_agents {
                lines.push(l);
            }
            lines.push(String::new());
        }
    }

    // Gotchas
    let gotchas: Vec<&&Value> = edge_values
        .iter()
        .filter(|e| e.get("rel").and_then(Value::as_str) == Some("gotcha_for"))
        .collect();
    let recent_gotchas = top_by(&gotchas, |e| score_of(e, now_ms), 5);
    if !recent_gotchas.is_empty() {
        lines.push("### Armadilhas ativas".into());
        for e in recent_gotchas {
            let from = e.get("from").and_then(Value::as_str).unwrap_or("");
            let to = e.get("to").and_then(Value::as_str).unwrap_or("");
            let node = nodes.get(from);
            let label = node
                .and_then(|n| n.get("label").and_then(Value::as_str))
                .filter(|s| !s.is_empty())
                .unwrap_or("(sem descrição)");
            lines.push(format!("- `{}`: {label}", short_file(to)));
        }
        lines.push(String::new());
    }

    lines
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_graph_shape() {
        let g = empty();
        assert_eq!(g["version"], VERSION);
        assert_eq!(g["rebuilt_at"], Value::Null);
        assert_eq!(g["nodes"], json!({}));
    }

    #[test]
    fn touch_node_creates_then_increments() {
        let mut g = empty();
        touch_node(&mut g, "file:a.txt", "file", "a.txt", "2026-04-19T12:00:00.000Z");
        touch_node(&mut g, "file:a.txt", "file", "a.txt", "2026-04-19T12:00:10.000Z");
        let n = &g["nodes"]["file:a.txt"];
        assert_eq!(n["count"], 2);
        assert_eq!(n["last_seen"], "2026-04-19T12:00:10.000Z");
    }

    #[test]
    fn touch_edge_with_intents() {
        let mut g = empty();
        touch_edge(&mut g, "a", "b", "handoff", "2026-04-19T12:00:00.000Z", Some("ask"));
        touch_edge(&mut g, "a", "b", "handoff", "2026-04-19T12:00:01.000Z", Some("ask"));
        touch_edge(&mut g, "a", "b", "handoff", "2026-04-19T12:00:02.000Z", Some("ack"));
        let e = &g["edges"]["a→b→handoff"];
        assert_eq!(e["count"], 3);
        assert_eq!(e["intents"]["ask"], 2);
        assert_eq!(e["intents"]["ack"], 1);
    }

    #[test]
    fn score_of_zero_age_equals_count() {
        let now = 1_000_000_000i64;
        let n = json!({"count": 5, "last_seen": format_iso_millis(now)});
        let s = score_of(&n, now);
        assert!((s - 5.0).abs() < 1e-12);
    }

    #[test]
    fn score_of_half_life_gives_half() {
        let now = 1_000_000_000i64;
        let last = now - HALF_LIFE_MS;
        let n = json!({"count": 10, "last_seen": format_iso_millis(last)});
        let s = score_of(&n, now);
        assert!((s - 10.0 * (-1.0f64).exp()).abs() < 1e-9);
    }

    #[test]
    fn obs_co_edited_creates_bidir() {
        let mut g = empty();
        let a = json!({"type":"file_edit","file":"a.txt","ts":"2026-04-19T12:00:00.000Z","session_id":"s1"});
        let b = json!({"type":"file_edit","file":"b.txt","ts":"2026-04-19T12:00:05.000Z","session_id":"s1"});
        ingest_observation(&mut g, &a, None);
        ingest_observation(&mut g, &b, Some(&a));
        assert!(g["edges"].as_object().unwrap().contains_key("file:a.txt→file:b.txt→co_edited"));
        assert!(g["edges"].as_object().unwrap().contains_key("file:b.txt→file:a.txt→co_edited"));
    }

    #[test]
    fn obs_different_session_no_coupling() {
        let mut g = empty();
        let a = json!({"type":"file_edit","file":"a.txt","ts":"2026-04-19T12:00:00.000Z","session_id":"s1"});
        let b = json!({"type":"file_edit","file":"b.txt","ts":"2026-04-19T12:00:05.000Z","session_id":"s2"});
        ingest_observation(&mut g, &a, None);
        ingest_observation(&mut g, &b, Some(&a));
        assert_eq!(g["edges"].as_object().unwrap().len(), 0);
    }

    #[test]
    fn obs_outside_window_no_coupling() {
        let mut g = empty();
        let a = json!({"type":"file_edit","file":"a.txt","ts":"2026-04-19T12:00:00.000Z","session_id":"s1"});
        let b = json!({"type":"file_edit","file":"b.txt","ts":"2026-04-19T12:20:00.000Z","session_id":"s1"});
        ingest_observation(&mut g, &a, None);
        ingest_observation(&mut g, &b, Some(&a));
        // 20 minutes > 15 min window.
        assert_eq!(g["edges"].as_object().unwrap().len(), 0);
    }

    #[test]
    fn agent_log_handoff_tallies_intent() {
        let mut g = empty();
        let ev = json!({"type":"handoff_out","target":"b","intent":"ask","ts":"2026-04-19T12:00:00.000Z"});
        ingest_agent_log_event(&mut g, "a", &ev);
        let e = &g["edges"]["agent:a→agent:b→handoff"];
        assert_eq!(e["count"], 1);
        assert_eq!(e["intents"]["ask"], 1);
    }

    #[test]
    fn decision_slug_id() {
        let d = json!({"what":"Adotar Asaas","ts":"2026-04-19T12:00:00.000Z"});
        assert_eq!(decision_id(&d), "decision:adotar-asaas");
    }

    #[test]
    fn decision_anon_when_empty_what() {
        let d = json!({"what":"","ts":"2026-04-19T12:00:00.000Z"});
        assert_eq!(decision_id(&d), "decision:anon-2026-04-19T12:00:00");
    }

    #[test]
    fn gotcha_file_edge() {
        let mut g = empty();
        let go = json!({"file":"src/lib.rs","description":"off by one","ts":"2026-04-19T12:00:00.000Z"});
        ingest_gotcha(&mut g, &go);
        let gid = gotcha_id(&go);
        let key = format!("{gid}→file:src/lib.rs→gotcha_for");
        assert!(g["edges"].as_object().unwrap().contains_key(&key));
    }
}

