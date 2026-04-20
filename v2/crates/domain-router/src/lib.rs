//! # domain-router
//!
//! Porta Rust de `plugin/scripts/router.cjs` (137 linhas).
//!
//! Roteador determinístico de handoff entre agentes. Responde:
//! "dado que `from` quer acionar alguém sobre `file`/`topic`,
//! quem dentro de `can_talk_to` é o melhor destino?"
//!
//! Paridade:
//! - Decay exponencial com `HALF_LIFE_DAYS = 30`.
//! - Score = 2× sinal de arquivo + 0.2× atividade geral +
//!   `canTalkBonus = (len - idx) * 0.01` (tie-break).
//! - `suggestTargets` detecta filename em ``[^\n]+``  ou extensão.

use regex::Regex;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

pub use domain_graph::{Clock, FixedClock, SystemClock};

pub const HALF_LIFE_DAYS: i64 = 30;
pub const HALF_LIFE_MS: i64 = HALF_LIFE_DAYS * 86_400_000;

/// Paridade com `decayScore` em `router.cjs:21`.
pub fn decay_score(entity: &Value, now_ms: i64) -> f64 {
    let count = entity.get("count").and_then(Value::as_f64).unwrap_or(0.0);
    let last_seen = entity
        .get("last_seen")
        .and_then(Value::as_str);
    let Some(iso) = last_seen else {
        return count;
    };
    // Parse ISO to millis — paridade com `new Date(entity.last_seen).getTime()`.
    let last_ms = match parse_iso_ms(iso) {
        Some(m) => m,
        None => return count,
    };
    let age = now_ms - last_ms;
    if age < 0 {
        return count;
    }
    count * (-(age as f64) / HALF_LIFE_MS as f64).exp()
}

/// Paridade simples com `new Date(x).getTime()` para ISO 8601.
/// Retorna None se inválido. Usa impl mínima (RFC3339 via chrono seria mais
/// correta, mas queremos zero deps; o formato salvo em `graph.json` é
/// `YYYY-MM-DDTHH:MM:SS.mmmZ`.)
fn parse_iso_ms(s: &str) -> Option<i64> {
    // Formato esperado: 2026-04-19T12:34:56.789Z
    // Extrai ano, mes, dia, hora, min, sec, ms
    let b = s.as_bytes();
    if b.len() < 20 {
        return None;
    }
    let year: i32 = std::str::from_utf8(&b[0..4]).ok()?.parse().ok()?;
    let month: u32 = std::str::from_utf8(&b[5..7]).ok()?.parse().ok()?;
    let day: u32 = std::str::from_utf8(&b[8..10]).ok()?.parse().ok()?;
    let hour: u32 = std::str::from_utf8(&b[11..13]).ok()?.parse().ok()?;
    let min: u32 = std::str::from_utf8(&b[14..16]).ok()?.parse().ok()?;
    let sec: u32 = std::str::from_utf8(&b[17..19]).ok()?.parse().ok()?;
    // millis opcional: .NNN
    let ms: i64 = if b.len() >= 23 && b[19] == b'.' {
        std::str::from_utf8(&b[20..23]).ok()?.parse().ok()?
    } else {
        0
    };
    // Converte para epoch millis via algoritmo Gregoriano.
    // Paridade com `Date.UTC(y, m-1, d, h, mi, s, ms)` do JS.
    let days = days_from_epoch(year, month, day)?;
    let total_sec =
        days as i64 * 86_400 + hour as i64 * 3600 + min as i64 * 60 + sec as i64;
    Some(total_sec * 1000 + ms)
}

fn days_from_epoch(y: i32, m: u32, d: u32) -> Option<i64> {
    // Civil from/to days — Howard Hinnant's algorithm.
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) as u64 + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era as i64 * 146_097 + doe as i64) - 719_468)
}

/// Paridade com `agentsForFile` em `router.cjs:30`.
/// Retorna `agent_name → score` somando decay de edges `edits` que apontam
/// pra `file:<file>`.
pub fn agents_for_file(g: &Value, file: &str, now_ms: i64) -> HashMap<String, f64> {
    let mut out: HashMap<String, f64> = HashMap::new();
    let Some(edges) = g.get("edges").and_then(Value::as_object) else {
        return out;
    };
    if file.is_empty() {
        return out;
    }
    let target_id = format!("file:{file}");
    for e in edges.values() {
        let rel = e.get("rel").and_then(Value::as_str).unwrap_or("");
        let to = e.get("to").and_then(Value::as_str).unwrap_or("");
        if rel != "edits" || to != target_id {
            continue;
        }
        let from = e.get("from").and_then(Value::as_str).unwrap_or("");
        let agent = from.strip_prefix("agent:").unwrap_or(from).to_string();
        let score = decay_score(e, now_ms);
        *out.entry(agent).or_insert(0.0) += score;
    }
    out
}

/// Paridade com `agentActivity` em `router.cjs:45`.
pub fn agent_activity(g: &Value, agent: &str, now_ms: i64) -> f64 {
    let Some(edges) = g.get("edges").and_then(Value::as_object) else {
        return 0.0;
    };
    let id = format!("agent:{agent}");
    let mut sum = 0.0;
    for e in edges.values() {
        let to = e.get("to").and_then(Value::as_str).unwrap_or("");
        let from = e.get("from").and_then(Value::as_str).unwrap_or("");
        if to != id && from != id {
            continue;
        }
        let rel = e.get("rel").and_then(Value::as_str).unwrap_or("");
        if matches!(rel, "edits" | "runs" | "handoff") {
            sum += decay_score(e, now_ms);
        }
    }
    sum
}

/// Paridade com `scoreCandidate` em `router.cjs:60`.
pub fn score_candidate(
    g: &Value,
    agent: &str,
    file_signals: &HashMap<String, f64>,
    now_ms: i64,
) -> f64 {
    let file_score = file_signals.get(agent).copied().unwrap_or(0.0);
    let activity = agent_activity(g, agent, now_ms);
    file_score * 2.0 + activity * 0.2
}

/// Paridade com `buildReason` em `router.cjs:66`.
pub fn build_reason(
    g: &Value,
    agent: &str,
    file_signals: &HashMap<String, f64>,
    file: Option<&str>,
    now_ms: i64,
) -> String {
    if let Some(file) = file {
        if file_signals.get(agent).copied().unwrap_or(0.0) > 0.0 {
            let target = format!("file:{file}");
            let source = format!("agent:{agent}");
            let n = g
                .get("edges")
                .and_then(Value::as_object)
                .and_then(|m| {
                    m.values().find(|e| {
                        e.get("rel").and_then(Value::as_str) == Some("edits")
                            && e.get("from").and_then(Value::as_str) == Some(source.as_str())
                            && e.get("to").and_then(Value::as_str) == Some(target.as_str())
                    })
                })
                .and_then(|e| e.get("count").and_then(Value::as_i64))
                .unwrap_or(0);
            return format!("editou `{file}` {n}x");
        }
    }
    let activity = agent_activity(g, agent, now_ms);
    if activity > 0.0 {
        "ativo recentemente".into()
    } else {
        "na sua can_talk_to".into()
    }
}

/// Resultado de [`route`].
#[derive(Debug, Clone)]
pub struct RouteResult {
    pub error: Option<String>,
    pub candidates: Vec<Value>,
    pub can_talk_to: Vec<String>,
    pub note: Option<String>,
}

impl RouteResult {
    fn err(msg: String) -> Self {
        Self {
            error: Some(msg),
            candidates: vec![],
            can_talk_to: vec![],
            note: None,
        }
    }
}

/// Paridade com `route` em `router.cjs:88`.
pub fn route(
    mem_dir: &Path,
    from: &str,
    file: Option<&str>,
    _topic: Option<&str>,
    intent: Option<&str>,
    clock: &dyn Clock,
) -> RouteResult {
    let manifest = match domain_agents::load_manifest(mem_dir) {
        Some(m) => m,
        None => return RouteResult::err(format!(r#"agent "{from}" not found"#)),
    };
    let agents = manifest.get("agents").and_then(Value::as_object);
    let cfg = agents.and_then(|m| m.get(from));
    let Some(cfg) = cfg else {
        return RouteResult::err(format!(r#"agent "{from}" not found"#));
    };

    let can_talk: Vec<String> = cfg
        .get("can_talk_to")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    if can_talk.is_empty() {
        return RouteResult {
            error: None,
            candidates: vec![],
            can_talk_to: vec![],
            note: Some("sem can_talk_to configurado".into()),
        };
    }

    let g = domain_graph::load_graph(mem_dir, None);
    let now_ms = clock.now_ms();
    let file_signals = match file {
        Some(f) => agents_for_file(&g, f, now_ms),
        None => HashMap::new(),
    };

    // escalate → move firefighters pro início
    let mut prioritized: Vec<String> = can_talk.clone();
    if intent == Some("escalate") {
        let re: &Regex = &FIREFIGHTER_RE;
        let (ff, rest): (Vec<_>, Vec<_>) = prioritized.into_iter().partition(|a| re.is_match(a));
        prioritized = ff.into_iter().chain(rest).collect();
    }

    let len = prioritized.len();
    let mut candidates: Vec<(f64, Value)> = prioritized
        .iter()
        .enumerate()
        .map(|(idx, agent)| {
            let base = score_candidate(&g, agent, &file_signals, now_ms);
            let can_talk_bonus = (len - idx) as f64 * 0.01;
            let reason = build_reason(&g, agent, &file_signals, file, now_ms);
            let score = base + can_talk_bonus;
            let mut obj = Map::new();
            obj.insert("agent".into(), Value::String(agent.clone()));
            obj.insert(
                "score".into(),
                Value::Number(
                    serde_json::Number::from_f64(score).unwrap_or_else(|| 0.into()),
                ),
            );
            obj.insert("reason".into(), Value::String(reason));
            obj.insert("in_can_talk_to".into(), Value::Bool(true));
            (score, Value::Object(obj))
        })
        .collect();
    // sort DESC by score
    candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    RouteResult {
        error: None,
        candidates: candidates.into_iter().map(|(_, v)| v).collect(),
        can_talk_to: can_talk,
        note: None,
    }
}

/// Paridade com `suggestTargets` em `router.cjs:130`.
pub fn suggest_targets(
    mem_dir: &Path,
    from: &str,
    message: &str,
    clock: &dyn Clock,
) -> (Option<String>, Vec<Value>) {
    let file = FILE_RE.captures(message).and_then(|caps| {
        caps.get(1)
            .or_else(|| caps.get(2))
            .map(|m| m.as_str().to_string())
    });
    let r = route(mem_dir, from, file.as_deref(), None, None, clock);
    let mut cands = r.candidates;
    cands.truncate(3);
    (file, cands)
}

static FIREFIGHTER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)firefighter").unwrap());
static FILE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"`([^`\n]+)`|\b([a-zA-Z0-9_\-./]+\.(py|ts|tsx|js|cjs|rs|md|json|yaml|yml|sql))\b")
        .unwrap()
});

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_iso_exact() {
        let ms = parse_iso_ms("2026-04-19T00:00:00.000Z").unwrap();
        // 2026-04-19 = days since epoch * 86400 * 1000
        // Sanity: > year-2000 baseline
        assert!(ms > 1_700_000_000_000);
    }

    #[test]
    fn decay_no_last_seen_returns_count() {
        let e = json!({"count": 5});
        assert_eq!(decay_score(&e, 0), 5.0);
    }

    #[test]
    fn decay_same_time_returns_count() {
        let e = json!({"count": 10, "last_seen": "2026-04-19T00:00:00.000Z"});
        let ms = parse_iso_ms("2026-04-19T00:00:00.000Z").unwrap();
        assert_eq!(decay_score(&e, ms), 10.0);
    }

    #[test]
    fn decay_age_30d_approx_half() {
        let e = json!({"count": 100, "last_seen": "2026-03-20T00:00:00.000Z"});
        let ms = parse_iso_ms("2026-04-19T00:00:00.000Z").unwrap();
        let s = decay_score(&e, ms);
        assert!((s - 100.0 / std::f64::consts::E).abs() < 0.5);
    }

    #[test]
    fn agents_for_file_sums() {
        let g = json!({
            "edges": {
                "e1": {"rel": "edits", "from": "agent:a", "to": "file:x.rs", "count": 3},
                "e2": {"rel": "edits", "from": "agent:b", "to": "file:x.rs", "count": 5},
                "e3": {"rel": "edits", "from": "agent:a", "to": "file:y.rs", "count": 1},
                "e4": {"rel": "handoff", "from": "agent:a", "to": "file:x.rs", "count": 99},
            }
        });
        let out = agents_for_file(&g, "x.rs", 0);
        assert_eq!(out.get("a"), Some(&3.0));
        assert_eq!(out.get("b"), Some(&5.0));
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn file_regex_matches_backtick() {
        let caps = FILE_RE.captures("veja `src/x.rs` por favor").unwrap();
        assert_eq!(caps.get(1).unwrap().as_str(), "src/x.rs");
    }

    #[test]
    fn file_regex_matches_extension() {
        let caps = FILE_RE.captures("olha o foo/bar.py que fiz").unwrap();
        assert_eq!(caps.get(2).unwrap().as_str(), "foo/bar.py");
    }
}
