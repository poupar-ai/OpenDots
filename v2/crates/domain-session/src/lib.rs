//! Pure domain logic extracted from `plugin/scripts/session-init.cjs`
//! and `plugin/scripts/session-end.cjs`.
//!
//! The original `.cjs` files are hook scripts (procedural, no module
//! exports). This crate carves out the pure functions — classifiers,
//! formatters, token budget, gotcha detection — leaving I/O, stdin,
//! cross-module requires and the actual file writes to a future
//! adapter layer.

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid regex: {0}")]
    Regex(#[from] regex::Error),
}

// ─── Correction detection ────────────────────────────────────────────

fn correction_markers() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(\s*)(não\b|nao\b|pare\b|espera\b|calma\b|errado\b|incorret[oa]\b|volta\b|desfaz\b|stop\b|wait\b|don'?t\b|no[,!.\s]|wrong\b|incorrect\b|undo\b|revert\b)",
        )
        .expect("correction markers regex is valid")
    })
}

/// Returns `true` when the user's prompt looks like a correction
/// (short message starting with explicit correction markers).
///
/// Matches the `.cjs` heuristic byte-for-byte:
/// - empty → `false`
/// - length > 600 → `false`
/// - else → regex test
///
/// Length is measured in Unicode scalars (`chars().count()`); diverges
/// from JS `.length` (UTF-16 code units) only for non-BMP chars.
pub fn is_correction_prompt(prompt: &str) -> bool {
    if prompt.is_empty() {
        return false;
    }
    if prompt.chars().count() > 600 {
        return false;
    }
    correction_markers().is_match(prompt)
}

// ─── Compact observation (for correction log) ────────────────────────

/// Produces a compacted JSON shape for a prior observation, used when
/// recording a correction so the stored record knows what the agent
/// was doing right before the user pushed back.
pub fn compact_obs(o: &Value) -> Value {
    let type_ = o.get("type").and_then(Value::as_str).unwrap_or("");
    let mut m = Map::new();
    match type_ {
        "file_edit" => {
            m.insert("type".into(), Value::String("file_edit".into()));
            m.insert("file".into(), o.get("file").cloned().unwrap_or(Value::Null));
        }
        "bash" => {
            m.insert("type".into(), Value::String("bash".into()));
            let cmd = o.get("cmd").and_then(Value::as_str).unwrap_or("");
            let sliced: String = cmd.chars().take(120).collect();
            m.insert("cmd".into(), Value::String(sliced));
        }
        _ => {
            m.insert("type".into(), o.get("type").cloned().unwrap_or(Value::Null));
        }
    }
    Value::Object(m)
}

// ─── Observation formatting ─────────────────────────────────────────

fn js_display(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => "[object Object]".into(),
    }
}

fn is_truthy_string(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// Renders a single observation as the markdown line used in the
/// "Observações recentes" injection block.
pub fn format_observation(ts: &str, o: &Value) -> String {
    let type_ = o.get("type").and_then(Value::as_str).unwrap_or("");
    match type_ {
        "file_edit" => {
            let file = js_display(o.get("file"));
            let stat = if let Some(d) = o.get("diff") {
                if let Some(ins) = d.get("ins").and_then(Value::as_i64) {
                    let del = d.get("del").and_then(Value::as_i64).unwrap_or(0);
                    let tracked_false = matches!(d.get("tracked"), Some(Value::Bool(false)));
                    let novo = if tracked_false { " novo" } else { "" };
                    format!(" (+{}/-{}{})", ins, del, novo)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            format!("- `{}` EDIT `{}`{}", ts, file, stat)
        }
        "bash" => {
            let cmd = o.get("cmd").and_then(Value::as_str).unwrap_or("");
            let cmd_sliced: String = cmd.chars().take(120).collect();
            let exit = match o.get("exit") {
                Some(Value::Null) | None => String::new(),
                Some(v) => format!(" → exit {}", js_display(Some(v))),
            };
            let out = is_truthy_string(o.get("output_summary"))
                .map(|s| format!(" _{}_", s))
                .unwrap_or_default();
            format!("- `{}` BASH `{}`{}{}", ts, cmd_sliced, exit, out)
        }
        "agent_call" => {
            let agent = js_display(o.get("agent"));
            let desc = is_truthy_string(o.get("description"))
                .map(|s| format!(" — {}", s))
                .unwrap_or_default();
            format!("- `{}` AGENT `{}`{}", ts, agent, desc)
        }
        "note" => {
            let content = js_display(o.get("content"));
            format!("- `{}` NOTE: {}", ts, content)
        }
        _ => {
            let t = js_display(o.get("type"));
            format!("- `{}` {}", ts, t)
        }
    }
}

// ─── Token budget ────────────────────────────────────────────────────

/// Rough token estimator: ~1 token per 3.5 UTF-16 code units.
/// Matches `plugin/scripts/lib.cjs:estimateTokens`.
pub fn estimate_tokens(s: &str) -> usize {
    if s.is_empty() {
        return 0;
    }
    let len = s.encode_utf16().count() as f64;
    (len / 3.5).ceil() as usize
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub name: String,
    pub priority: i32,
    pub text: String,
    pub tokens: usize,
}

/// Builds a block from an array of lines. Returns `None` if the
/// joined content is empty after `trim()`.
pub fn make_block(name: &str, priority: i32, lines: &[String]) -> Option<Block> {
    let text = lines.join("\n");
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let owned = trimmed.to_string();
    let tokens = estimate_tokens(&owned);
    Some(Block {
        name: name.to_string(),
        priority,
        text: owned,
        tokens,
    })
}

/// Sorts blocks by priority (stable) and partitions them into
/// `(included, skipped)` given a token budget. Matches the greedy
/// selection in `session-init.cjs` exactly.
pub fn apply_token_budget(mut blocks: Vec<Block>, budget: usize) -> (Vec<Block>, Vec<Block>) {
    blocks.sort_by(|a, b| a.priority.cmp(&b.priority));
    let mut included = Vec::new();
    let mut skipped = Vec::new();
    let mut total: usize = 0;
    for b in blocks {
        if total.saturating_add(b.tokens) <= budget {
            total += b.tokens;
            included.push(b);
        } else {
            skipped.push(b);
        }
    }
    (included, skipped)
}

// ─── Session classification ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    Docs,
    Config,
    Bugfix,
    Refactor,
    Feature,
    Exploration,
    Unknown,
}

impl SessionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionKind::Docs => "docs",
            SessionKind::Config => "config",
            SessionKind::Bugfix => "bugfix",
            SessionKind::Refactor => "refactor",
            SessionKind::Feature => "feature",
            SessionKind::Exploration => "exploration",
            SessionKind::Unknown => "unknown",
        }
    }
}

fn re_docs() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\.md$|readme|docs/").unwrap())
}
fn re_config() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\.(env|config|toml|yml|yaml|json)$|dockerfile|\.dockerignore|makefile")
            .unwrap()
    })
}
fn re_src() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\.(py|ts|tsx|js|jsx|rs|go|java)$").unwrap())
}
fn re_fix_cmd() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(fix|bugfix|debug|hotfix)\b").unwrap())
}
fn re_fix_or_bug() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bfix\b|\bbug\b").unwrap())
}
fn re_refactor_cmd() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\brefactor\b|\bmove\b|\brename\b").unwrap())
}
fn re_test() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)test").unwrap())
}

/// Classifies a session from its observations.
///
/// Ports `classifySession` in `session-end.cjs`. Operator precedence
/// preserved: `bugfix` trigger is `cmd-has-fix || (text-has-fix &&
/// files-has-test)`.
pub fn classify_session(obs: &[Value]) -> SessionKind {
    let files: Vec<String> = obs
        .iter()
        .filter(|o| o.get("type").and_then(Value::as_str) == Some("file_edit"))
        .map(|o| {
            o.get("file")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        })
        .collect();
    let cmds: Vec<String> = obs
        .iter()
        .filter(|o| o.get("type").and_then(Value::as_str) == Some("bash"))
        .map(|o| {
            o.get("cmd")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_lowercase()
        })
        .collect();

    let mut all_text = String::new();
    for f in &files {
        if !all_text.is_empty() {
            all_text.push(' ');
        }
        all_text.push_str(f);
    }
    for c in &cmds {
        if !all_text.is_empty() {
            all_text.push(' ');
        }
        all_text.push_str(c);
    }
    let all_text_lc = all_text.to_lowercase();

    let only_docs = !files.is_empty() && files.iter().all(|f| re_docs().is_match(f));
    if only_docs {
        return SessionKind::Docs;
    }

    let any_config = files.iter().any(|f| re_config().is_match(f));
    let any_src = files.iter().any(|f| re_src().is_match(f));
    if any_config && !any_src {
        return SessionKind::Config;
    }

    let fix_in_cmd = cmds.iter().any(|c| re_fix_cmd().is_match(c));
    let text_has_fix = re_fix_or_bug().is_match(&all_text_lc);
    let has_test_file = files.iter().any(|f| re_test().is_match(f));
    if fix_in_cmd || (text_has_fix && has_test_file) {
        return SessionKind::Bugfix;
    }

    if cmds.iter().any(|c| re_refactor_cmd().is_match(c)) {
        return SessionKind::Refactor;
    }

    let has_tests = files.iter().any(|f| re_test().is_match(f));
    let has_src = files.iter().any(|f| re_src().is_match(f));
    if has_tests && has_src {
        return SessionKind::Feature;
    }
    if has_src {
        return SessionKind::Feature;
    }

    let edit_count = obs
        .iter()
        .filter(|o| o.get("type").and_then(Value::as_str) == Some("file_edit"))
        .count();
    if edit_count == 0 && !cmds.is_empty() {
        return SessionKind::Exploration;
    }
    SessionKind::Unknown
}

// ─── Gotcha detection ────────────────────────────────────────────────

/// A detected gotcha, matching the shape appended to `gotchas.jsonl`
/// in `session-end.cjs:detectGotchas`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GotchaDetected {
    pub session_id: Option<String>,
    pub file: Value,
    pub command: String,
    pub failure_hint: Option<String>,
    pub description: String,
    pub ts: String,
}

fn bash_exit_nonzero(o: &Value) -> bool {
    match o.get("exit") {
        None | Some(Value::Null) => false,
        Some(Value::Number(n)) => n.as_i64() != Some(0),
        _ => true,
    }
}

fn bash_exit_zero(o: &Value) -> bool {
    // JS: `(c.exit ?? 1) !== 0 → continue`. Missing/null treated as 1
    // (nonzero → skip), so only an explicit 0 counts as success.
    match o.get("exit") {
        Some(Value::Number(n)) => n.as_i64() == Some(0),
        _ => false,
    }
}

fn cmd_prefix3(cmd: &str) -> String {
    cmd.split_whitespace().take(3).collect::<Vec<_>>().join(" ")
}

/// Finds `bash(fail) → file_edit → bash(success, same prefix)`
/// sequences and returns one `GotchaDetected` per match.
///
/// `session_id` and `now` are injected by the caller (pure function;
/// no clock or env access inside).
pub fn detect_gotchas(obs: &[Value], session_id: Option<&str>, now: &str) -> Vec<GotchaDetected> {
    let mut out = Vec::new();
    if obs.len() < 3 {
        return out;
    }
    for i in 0..obs.len().saturating_sub(2) {
        let a = &obs[i];
        let b = &obs[i + 1];
        let c = &obs[i + 2];

        if a.get("type").and_then(Value::as_str) != Some("bash") {
            continue;
        }
        if !bash_exit_nonzero(a) {
            continue;
        }
        if b.get("type").and_then(Value::as_str) != Some("file_edit") {
            continue;
        }
        if c.get("type").and_then(Value::as_str) != Some("bash") {
            continue;
        }

        let a_cmd = a.get("cmd").and_then(Value::as_str).unwrap_or("");
        let c_cmd = c.get("cmd").and_then(Value::as_str).unwrap_or("");
        if cmd_prefix3(a_cmd) != cmd_prefix3(c_cmd) {
            continue;
        }
        if !bash_exit_zero(c) {
            continue;
        }

        let a_cmd_80: String = a_cmd.chars().take(80).collect();
        let file_disp = js_display(b.get("file"));
        let description = format!("`{}` falhou até editar `{}`", a_cmd_80, file_disp);

        let command_120: String = a_cmd.chars().take(120).collect();
        let failure_hint = is_truthy_string(a.get("output_summary")).map(str::to_string);

        out.push(GotchaDetected {
            session_id: session_id.map(str::to_string),
            file: b.get("file").cloned().unwrap_or(Value::Null),
            command: command_120,
            failure_hint,
            description,
            ts: now.to_string(),
        });
    }
    out
}

// ─── Summary ─────────────────────────────────────────────────────────

fn basename_slash(p: &str) -> String {
    p.rsplit('/').next().unwrap_or("").to_string()
}

fn dedup_preserve_order<I, T>(iter: I) -> Vec<T>
where
    I: IntoIterator<Item = T>,
    T: Eq + std::hash::Hash + Clone,
{
    let mut seen: HashSet<T> = HashSet::new();
    let mut out = Vec::new();
    for item in iter {
        if seen.insert(item.clone()) {
            out.push(item);
        }
    }
    out
}

/// Produces the `summary` string appended to `sessions.jsonl` on
/// session end. Matches `session-end.cjs` lines 44-60.
pub fn summarize_session(session_obs: &[Value]) -> String {
    let edited_files: Vec<String> = dedup_preserve_order(
        session_obs
            .iter()
            .filter(|o| o.get("type").and_then(Value::as_str) == Some("file_edit"))
            .filter_map(|o| o.get("file").and_then(Value::as_str))
            .map(basename_slash),
    )
    .into_iter()
    .take(8)
    .collect();

    let cmds: Vec<String> = dedup_preserve_order(
        session_obs
            .iter()
            .filter(|o| o.get("type").and_then(Value::as_str) == Some("bash"))
            .filter_map(|o| o.get("cmd").and_then(Value::as_str))
            .map(|c| c.split_whitespace().take(4).collect::<Vec<_>>().join(" ")),
    )
    .into_iter()
    .take(5)
    .collect();

    let notes: Vec<String> = session_obs
        .iter()
        .filter(|o| o.get("type").and_then(Value::as_str) == Some("note"))
        .filter_map(|o| o.get("content").and_then(Value::as_str).map(str::to_string))
        .take(3)
        .collect();

    let mut parts: Vec<String> = Vec::new();
    if !edited_files.is_empty() {
        parts.push(format!("Editado: {}", edited_files.join(", ")));
    }
    if !cmds.is_empty() {
        parts.push(format!("Rodou: {}", cmds.join("; ")));
    }
    if !notes.is_empty() {
        parts.push(format!("Notas: {}", notes.join("; ")));
    }

    if parts.is_empty() {
        format!("{} operações", session_obs.len())
    } else {
        parts.join(". ")
    }
}

// ─── Markdown regeneration ───────────────────────────────────────────

/// Regenerates DECISIONS.md from a chronological decisions array.
/// Returns `None` if the input is empty (JS version silently writes
/// nothing in that case).
pub fn regenerate_decisions_md(decisions: &[Value]) -> Option<String> {
    if decisions.is_empty() {
        return None;
    }
    let mut out: Vec<String> = vec!["# Decisões do projeto".into(), String::new()];
    for d in decisions.iter().rev() {
        let ts = d.get("ts").and_then(Value::as_str).unwrap_or("");
        let date: String = ts.chars().take(10).collect();
        let what = is_truthy_string(d.get("what"))
            .or_else(|| is_truthy_string(d.get("text")))
            .unwrap_or("(sem título)");
        out.push(format!("## {} — {}", date, what));
        if let Some(why) = is_truthy_string(d.get("why")) {
            out.push(String::new());
            out.push(why.to_string());
        }
        out.push(String::new());
    }
    Some(out.join("\n"))
}

/// Regenerates GOTCHAS.md, dedup'ing by `(file, command[..40])`.
/// Iterates gotchas in reverse (latest first) just like the `.cjs`.
pub fn regenerate_gotchas_md(gotchas: &[Value]) -> Option<String> {
    if gotchas.is_empty() {
        return None;
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut unique: Vec<&Value> = Vec::new();
    for g in gotchas.iter().rev() {
        let file = js_display(g.get("file"));
        let cmd = g.get("command").and_then(Value::as_str).unwrap_or("");
        let cmd40: String = cmd.chars().take(40).collect();
        let key = format!("{}|{}", file, cmd40);
        if seen.insert(key) {
            unique.push(g);
        }
    }
    if unique.is_empty() {
        return None;
    }
    let mut out: Vec<String> = vec!["# Armadilhas conhecidas".into(), String::new()];
    for g in unique {
        let ts = g.get("ts").and_then(Value::as_str).unwrap_or("");
        let date: String = ts.chars().take(10).collect();
        let file = js_display(g.get("file"));
        out.push(format!("## {} — `{}`", date, file));
        out.push(String::new());
        let desc = g.get("description").and_then(Value::as_str).unwrap_or("");
        out.push(desc.to_string());
        if let Some(hint) = is_truthy_string(g.get("failure_hint")) {
            out.push(String::new());
            out.push(format!("_Sintoma:_ {}", hint));
        }
        out.push(String::new());
    }
    Some(out.join("\n"))
}

/// Regenerates GLOSSARY.md from entries in `glossary.jsonl`.
pub fn regenerate_glossary_md(entries: &[Value]) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let mut out: Vec<String> = vec!["# Glossário do projeto".into(), String::new()];
    for e in entries {
        let term = js_display(e.get("term"));
        out.push(format!("## {}", term));
        out.push(String::new());
        let def = e.get("definition").and_then(Value::as_str).unwrap_or("");
        out.push(def.to_string());
        out.push(String::new());
    }
    Some(out.join("\n"))
}

// ─── Unit tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // is_correction_prompt

    #[test]
    fn correction_empty_false() {
        assert!(!is_correction_prompt(""));
    }

    #[test]
    fn correction_markers_match() {
        assert!(is_correction_prompt("não faz isso"));
        assert!(is_correction_prompt("nao, pare"));
        assert!(is_correction_prompt("pare agora"));
        assert!(is_correction_prompt("stop"));
        assert!(is_correction_prompt("don't do that"));
        assert!(is_correction_prompt("dont do it"));
        assert!(is_correction_prompt("errado"));
        assert!(is_correction_prompt("incorreta"));
        assert!(is_correction_prompt("incorreto"));
        assert!(is_correction_prompt("undo"));
        assert!(is_correction_prompt("   volta aqui"));
        assert!(is_correction_prompt("No, that's wrong"));
    }

    #[test]
    fn correction_non_markers_false() {
        assert!(!is_correction_prompt("faz isso"));
        assert!(!is_correction_prompt("hello world"));
        assert!(!is_correction_prompt("pode continuar"));
    }

    #[test]
    fn correction_too_long_false() {
        let long = "não ".repeat(300); // 1200 chars
        assert!(!is_correction_prompt(&long));
    }

    // compact_obs

    #[test]
    fn compact_file_edit() {
        let o = json!({"type": "file_edit", "file": "src/a.rs", "extra": "ignored"});
        assert_eq!(
            compact_obs(&o),
            json!({"type": "file_edit", "file": "src/a.rs"})
        );
    }

    #[test]
    fn compact_bash_truncates_120() {
        let cmd = "a".repeat(200);
        let o = json!({"type": "bash", "cmd": cmd});
        let out = compact_obs(&o);
        assert_eq!(out["cmd"].as_str().unwrap().len(), 120);
        assert_eq!(out["type"], "bash");
    }

    #[test]
    fn compact_bash_missing_cmd() {
        let o = json!({"type": "bash"});
        assert_eq!(compact_obs(&o), json!({"type": "bash", "cmd": ""}));
    }

    #[test]
    fn compact_other_keeps_type() {
        let o = json!({"type": "note", "content": "hi"});
        assert_eq!(compact_obs(&o), json!({"type": "note"}));
    }

    // estimate_tokens

    #[test]
    fn estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn estimate_tokens_basic() {
        // 7 chars / 3.5 = 2.0 → ceil = 2
        assert_eq!(estimate_tokens("1234567"), 2);
        // 8 chars / 3.5 = 2.28 → ceil = 3
        assert_eq!(estimate_tokens("12345678"), 3);
    }

    // make_block

    #[test]
    fn make_block_empty_returns_none() {
        assert!(make_block("x", 1, &[]).is_none());
        assert!(make_block("x", 1, &["".into(), "   ".into()]).is_none());
    }

    #[test]
    fn make_block_trims() {
        let b = make_block("x", 2, &["  hello  ".into()]).unwrap();
        assert_eq!(b.text, "hello");
        assert_eq!(b.priority, 2);
    }

    // apply_token_budget

    #[test]
    fn budget_selects_by_priority() {
        let blocks = vec![
            Block {
                name: "b".into(),
                priority: 2,
                text: "bb".into(),
                tokens: 10,
            },
            Block {
                name: "a".into(),
                priority: 1,
                text: "aa".into(),
                tokens: 10,
            },
            Block {
                name: "c".into(),
                priority: 3,
                text: "cc".into(),
                tokens: 10,
            },
        ];
        let (inc, skip) = apply_token_budget(blocks, 25);
        assert_eq!(
            inc.iter().map(|b| b.name.clone()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert_eq!(
            skip.iter().map(|b| b.name.clone()).collect::<Vec<_>>(),
            vec!["c"]
        );
    }

    #[test]
    fn budget_skips_oversize_continues_scan() {
        // Greedy: once a block is skipped, later blocks that fit still get included.
        let blocks = vec![
            Block {
                name: "big".into(),
                priority: 1,
                text: String::new(),
                tokens: 100,
            },
            Block {
                name: "tiny".into(),
                priority: 2,
                text: String::new(),
                tokens: 5,
            },
        ];
        let (inc, skip) = apply_token_budget(blocks, 10);
        assert_eq!(
            inc.iter().map(|b| b.name.clone()).collect::<Vec<_>>(),
            vec!["tiny"]
        );
        assert_eq!(
            skip.iter().map(|b| b.name.clone()).collect::<Vec<_>>(),
            vec!["big"]
        );
    }

    // classify_session

    #[test]
    fn classify_docs_only() {
        let obs = vec![json!({"type": "file_edit", "file": "README.md"})];
        assert_eq!(classify_session(&obs), SessionKind::Docs);
    }

    #[test]
    fn classify_config() {
        let obs = vec![json!({"type": "file_edit", "file": "Dockerfile"})];
        assert_eq!(classify_session(&obs), SessionKind::Config);
    }

    #[test]
    fn classify_feature_tests_and_src() {
        let obs = vec![
            json!({"type": "file_edit", "file": "src/foo.py"}),
            json!({"type": "file_edit", "file": "tests/test_foo.py"}),
        ];
        assert_eq!(classify_session(&obs), SessionKind::Feature);
    }

    #[test]
    fn classify_bugfix_by_cmd() {
        let obs = vec![
            json!({"type": "file_edit", "file": "src/a.py"}),
            json!({"type": "bash", "cmd": "git commit -m 'fix crash'"}),
        ];
        assert_eq!(classify_session(&obs), SessionKind::Bugfix);
    }

    #[test]
    fn classify_bugfix_by_text_and_test_file() {
        let obs = vec![
            json!({"type": "file_edit", "file": "tests/test_bug.py"}),
            json!({"type": "bash", "cmd": "pytest fix"}),
        ];
        assert_eq!(classify_session(&obs), SessionKind::Bugfix);
    }

    #[test]
    fn classify_refactor() {
        let obs = vec![json!({"type": "bash", "cmd": "git mv old new  # refactor"})];
        assert_eq!(classify_session(&obs), SessionKind::Refactor);
    }

    #[test]
    fn classify_exploration() {
        let obs = vec![json!({"type": "bash", "cmd": "ls"})];
        assert_eq!(classify_session(&obs), SessionKind::Exploration);
    }

    #[test]
    fn classify_unknown_empty() {
        assert_eq!(classify_session(&[]), SessionKind::Unknown);
    }

    // detect_gotchas

    #[test]
    fn gotcha_fail_edit_success() {
        let obs = vec![
            json!({"type": "bash", "cmd": "cargo test foo", "exit": 1, "output_summary": "bar missing"}),
            json!({"type": "file_edit", "file": "src/lib.rs"}),
            json!({"type": "bash", "cmd": "cargo test foo --release", "exit": 0}),
        ];
        let got = detect_gotchas(&obs, Some("s1"), "2026-04-19T10:00:00Z");
        assert_eq!(got.len(), 1);
        let g = &got[0];
        assert_eq!(g.session_id.as_deref(), Some("s1"));
        assert_eq!(g.file, json!("src/lib.rs"));
        assert_eq!(g.failure_hint.as_deref(), Some("bar missing"));
        assert!(g.description.contains("falhou até editar `src/lib.rs`"));
        assert_eq!(g.ts, "2026-04-19T10:00:00Z");
    }

    #[test]
    fn gotcha_no_match_when_cmd_prefix_differs() {
        let obs = vec![
            json!({"type": "bash", "cmd": "pytest a", "exit": 1}),
            json!({"type": "file_edit", "file": "a.py"}),
            json!({"type": "bash", "cmd": "cargo build", "exit": 0}),
        ];
        assert!(detect_gotchas(&obs, None, "x").is_empty());
    }

    #[test]
    fn gotcha_no_match_when_second_bash_fails() {
        let obs = vec![
            json!({"type": "bash", "cmd": "pytest", "exit": 1}),
            json!({"type": "file_edit", "file": "a.py"}),
            json!({"type": "bash", "cmd": "pytest", "exit": 2}),
        ];
        assert!(detect_gotchas(&obs, None, "x").is_empty());
    }

    #[test]
    fn gotcha_c_exit_missing_treated_as_failure() {
        // `c.exit ?? 1 !== 0 → continue`. Missing c.exit → 1 → skip.
        let obs = vec![
            json!({"type": "bash", "cmd": "pytest", "exit": 1}),
            json!({"type": "file_edit", "file": "a.py"}),
            json!({"type": "bash", "cmd": "pytest"}),
        ];
        let got = detect_gotchas(&obs, None, "x");
        assert!(got.is_empty());
    }

    // summarize_session

    #[test]
    fn summary_empty() {
        assert_eq!(summarize_session(&[]), "0 operações");
    }

    #[test]
    fn summary_mixed() {
        let obs = vec![
            json!({"type": "file_edit", "file": "src/foo.py"}),
            json!({"type": "bash", "cmd": "cargo test --all -- --nocapture"}),
            json!({"type": "note", "content": "rebasing"}),
        ];
        let s = summarize_session(&obs);
        assert!(s.contains("Editado: foo.py"));
        assert!(s.contains("Rodou: cargo test --all --"));
        assert!(s.contains("Notas: rebasing"));
    }

    #[test]
    fn summary_dedups_preserve_order() {
        let obs = vec![
            json!({"type": "file_edit", "file": "a/x.rs"}),
            json!({"type": "file_edit", "file": "b/x.rs"}),
        ];
        // basename collision → only first "x.rs"
        let s = summarize_session(&obs);
        assert_eq!(s, "Editado: x.rs");
    }

    // format_observation

    #[test]
    fn fmt_file_edit_with_diff() {
        let o = json!({
            "type": "file_edit",
            "file": "a.rs",
            "diff": {"ins": 10, "del": 2}
        });
        assert_eq!(
            format_observation("2026-04-19 10:00", &o),
            "- `2026-04-19 10:00` EDIT `a.rs` (+10/-2)"
        );
    }

    #[test]
    fn fmt_file_edit_no_diff() {
        let o = json!({"type": "file_edit", "file": "a.rs"});
        assert_eq!(format_observation("t", &o), "- `t` EDIT `a.rs`");
    }

    #[test]
    fn fmt_file_edit_novo() {
        let o = json!({
            "type": "file_edit",
            "file": "a.rs",
            "diff": {"ins": 5, "tracked": false}
        });
        assert_eq!(
            format_observation("t", &o),
            "- `t` EDIT `a.rs` (+5/-0 novo)"
        );
    }

    #[test]
    fn fmt_bash_with_exit_and_summary() {
        let o = json!({
            "type": "bash",
            "cmd": "ls",
            "exit": 0,
            "output_summary": "ok"
        });
        assert_eq!(format_observation("t", &o), "- `t` BASH `ls` → exit 0 _ok_");
    }

    #[test]
    fn fmt_bash_exit_null_suppressed() {
        let o = json!({"type": "bash", "cmd": "ls", "exit": null});
        assert_eq!(format_observation("t", &o), "- `t` BASH `ls`");
    }

    #[test]
    fn fmt_agent_call() {
        let o = json!({"type": "agent_call", "agent": "manager", "description": "review"});
        assert_eq!(
            format_observation("t", &o),
            "- `t` AGENT `manager` — review"
        );
    }

    #[test]
    fn fmt_note() {
        let o = json!({"type": "note", "content": "rebasing"});
        assert_eq!(format_observation("t", &o), "- `t` NOTE: rebasing");
    }

    #[test]
    fn fmt_unknown_type() {
        let o = json!({"type": "weird"});
        assert_eq!(format_observation("t", &o), "- `t` weird");
    }

    // regenerate_decisions_md

    #[test]
    fn decisions_md_empty() {
        assert!(regenerate_decisions_md(&[]).is_none());
    }

    #[test]
    fn decisions_md_basic() {
        let d = vec![
            json!({"ts": "2026-04-18T10:00:00Z", "what": "Use pg", "why": "scale"}),
            json!({"ts": "2026-04-19T10:00:00Z", "text": "Ship MVP"}),
        ];
        let md = regenerate_decisions_md(&d).unwrap();
        // Latest first: Ship MVP before Use pg
        let ship_idx = md.find("Ship MVP").unwrap();
        let pg_idx = md.find("Use pg").unwrap();
        assert!(ship_idx < pg_idx);
        assert!(md.contains("2026-04-19 — Ship MVP"));
        assert!(md.contains("2026-04-18 — Use pg"));
        assert!(md.contains("\nscale\n"));
    }

    #[test]
    fn decisions_md_fallback_title() {
        let d = vec![json!({"ts": "2026-01-01T00:00:00Z"})];
        let md = regenerate_decisions_md(&d).unwrap();
        assert!(md.contains("(sem título)"));
    }

    // regenerate_gotchas_md

    #[test]
    fn gotchas_md_dedup() {
        let g = vec![
            json!({"file": "a.rs", "command": "cargo test", "description": "older", "ts": "2026-04-18T00:00:00Z"}),
            json!({"file": "a.rs", "command": "cargo test", "description": "newer", "ts": "2026-04-19T00:00:00Z"}),
        ];
        let md = regenerate_gotchas_md(&g).unwrap();
        assert!(md.contains("newer"));
        assert!(!md.contains("older"));
    }

    #[test]
    fn gotchas_md_with_hint() {
        let g = vec![json!({
            "file": "a.rs",
            "command": "cargo test",
            "description": "flaky",
            "failure_hint": "timeout",
            "ts": "2026-04-19T00:00:00Z"
        })];
        let md = regenerate_gotchas_md(&g).unwrap();
        assert!(md.contains("_Sintoma:_ timeout"));
    }

    // regenerate_glossary_md

    #[test]
    fn glossary_md_basic() {
        let e = vec![
            json!({"term": "Seller", "definition": "quem vende"}),
            json!({"term": "Buyer"}),
        ];
        let md = regenerate_glossary_md(&e).unwrap();
        assert!(md.contains("## Seller"));
        assert!(md.contains("quem vende"));
        assert!(md.contains("## Buyer"));
    }
}
