//! Port byte-a-byte de `plugin/scripts/indexer.cjs`. Extração regex por
//! linguagem, ranking híbrido (hot_file × refs × recência), rebuild +
//! incremental update, renderização em MD.
//!
//! JSON output é byte-exato com o .cjs (usamos `serde_json` com
//! `preserve_order` para espelhar o Object insertion-order do V8).

use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, io};

pub const INDEX_JSON: &str = "code_index.json";
pub const INDEX_MD: &str = "CODE_INDEX.md";
pub const MAX_FILE_BYTES: u64 = 512 * 1024;
pub const MAX_FILES: usize = 2000;
pub const INDEX_VERSION: u64 = 1;

#[derive(Debug, thiserror::Error)]
pub enum IndexerError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

// ── Clock ────────────────────────────────────────────────────────────────

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

// ── Linguagens ───────────────────────────────────────────────────────────

pub fn detect_lang(file: &str) -> Option<&'static str> {
    let ext = Path::new(file)
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    match ext.as_str() {
        "py" | "pyi" => Some("python"),
        "ts" | "tsx" => Some("typescript"),
        "js" | "jsx" | "mjs" | "cjs" => Some("javascript"),
        "rs" => Some("rust"),
        "sql" => Some("sql"),
        "go" => Some("go"),
        _ => None,
    }
}

// ── UTF-16 helpers (para paridade com JS string semantics) ────────────────

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn utf16_slice(s: &str, start: usize, end: usize) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    let end = end.min(units.len());
    let start = start.min(end);
    String::from_utf16_lossy(&units[start..end])
}

// ── compactArgs ──────────────────────────────────────────────────────────

static RE_WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

pub fn compact_args(args: &str) -> String {
    if args.is_empty() {
        return String::new();
    }
    let replaced = RE_WHITESPACE.replace_all(args, " ");
    let s = replaced.trim().to_string();
    if utf16_len(&s) > 60 {
        let mut truncated = utf16_slice(&s, 0, 57);
        truncated.push_str("...");
        truncated
    } else {
        s
    }
}

// ── escapeRegex ──────────────────────────────────────────────────────────

pub fn escape_regex(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '.' | '*' | '+' | '?' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']'
            | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

// ── Regexes por linguagem (LazyLock) ─────────────────────────────────────

static RE_PY_DEF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([ \t]*)(async\s+)?def\s+([A-Za-z0-9_]+)\s*\(([^)]*)\)(?:\s*->\s*([^:]+))?\s*:")
        .unwrap()
});
static RE_PY_CLASS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([ \t]*)class\s+([A-Za-z0-9_]+)(?:\s*\(([^)]*)\))?\s*:").unwrap()
});
static RE_PY_ROUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"^\s*@([A-Za-z0-9_]+)\.(get|post|put|delete|patch|websocket)\s*\(\s*["']([^"']+)["']"#,
    )
    .unwrap()
});
static RE_PY_DEF_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"def\s+([A-Za-z0-9_]+)").unwrap());

static RE_TS_FN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^export\s+(default\s+)?(async\s+)?function\s+([A-Za-z0-9_]+)\s*\(([^)]*)\)")
        .unwrap()
});
static RE_TS_CONST_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^export\s+(default\s+)?(const|let|var)\s+([A-Za-z0-9_]+)\s*(?::\s*[^=]+)?=")
        .unwrap()
});
static RE_TS_ARROW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(async\s+)?\(([^)]*)\)\s*(?::[^=]+)?=>").unwrap());
static RE_TS_CLASS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^export\s+(default\s+)?(abstract\s+)?class\s+([A-Za-z0-9_]+)(?:\s+extends\s+([A-Za-z0-9_]+))?",
    )
    .unwrap()
});
static RE_TS_TYPEDECL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^export\s+(interface|type|enum)\s+([A-Za-z0-9_]+)").unwrap()
});
static RE_TS_ROUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^export\s+(async\s+)?function\s+(GET|POST|PUT|DELETE|PATCH|OPTIONS|HEAD)\s*\(")
        .unwrap()
});

static RE_RS_FN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([ \t]*)(pub\s+(?:\([^)]+\)\s+)?)?(async\s+)?fn\s+([A-Za-z0-9_]+)\s*(?:<[^>]*>)?\s*\(([^)]*)\)")
        .unwrap()
});
static RE_RS_STRUCT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(pub\s+)?struct\s+([A-Za-z0-9_]+)").unwrap());
static RE_RS_ENUM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(pub\s+)?enum\s+([A-Za-z0-9_]+)").unwrap());
static RE_RS_TRAIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(pub\s+)?trait\s+([A-Za-z0-9_]+)").unwrap());

static RE_SQL_TABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)create\s+table\s+(?:if\s+not\s+exists\s+)?["']?([A-Za-z0-9_]+)["']?"#,
    )
    .unwrap()
});
static RE_SQL_TYPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)create\s+type\s+([A-Za-z0-9_]+)").unwrap());
static RE_SQL_INDEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)create\s+(?:or\s+replace\s+)?(?:unique\s+)?index\s+(?:if\s+not\s+exists\s+)?([A-Za-z0-9_]+)",
    )
    .unwrap()
});

static RE_GO_FN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^func\s+(?:\([^)]+\)\s+)?([A-Za-z][A-Za-z0-9_]*)\s*\(").unwrap()
});
static RE_GO_TYPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^type\s+([A-Za-z0-9_]+)\s+(struct|interface)").unwrap());

// ── Extratores ───────────────────────────────────────────────────────────

fn push_symbol(out: &mut Vec<Value>, fields: Vec<(&str, Value)>) {
    let mut map = Map::new();
    for (k, v) in fields {
        map.insert(k.to_string(), v);
    }
    out.push(Value::Object(map));
}

pub fn extract_python(content: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let lines: Vec<&str> = content.split('\n').collect();
    for (i, l) in lines.iter().enumerate() {
        if let Some(m) = RE_PY_DEF.captures(l) {
            let indent = m.get(1).map(|x| x.as_str().len()).unwrap_or(0);
            if indent == 0 {
                let is_async = m.get(2).is_some();
                let name = m.get(3).unwrap().as_str();
                let args = m.get(4).map(|x| x.as_str()).unwrap_or("");
                let ret = m.get(5).map(|x| x.as_str());
                let ret_part = match ret {
                    Some(r) => format!(" -> {}", r.trim()),
                    None => String::new(),
                };
                let sig = format!("def {}({}){}", name, compact_args(args), ret_part);
                push_symbol(
                    &mut out,
                    vec![
                        ("type", json!(if is_async { "async-fn" } else { "fn" })),
                        ("name", json!(name)),
                        ("sig", json!(sig)),
                        ("line", json!(i + 1)),
                    ],
                );
            }
            continue;
        }
        if let Some(m) = RE_PY_CLASS.captures(l) {
            let indent = m.get(1).map(|x| x.as_str().len()).unwrap_or(0);
            if indent == 0 {
                let name = m.get(2).unwrap().as_str();
                let bases = m.get(3).map(|x| x.as_str());
                let sig = match bases {
                    Some(b) => format!("class {}({})", name, b.trim()),
                    None => format!("class {}", name),
                };
                push_symbol(
                    &mut out,
                    vec![
                        ("type", json!("class")),
                        ("name", json!(name)),
                        ("sig", json!(sig)),
                        ("line", json!(i + 1)),
                    ],
                );
                continue;
            }
        }
        if let Some(m) = RE_PY_ROUTE.captures(l) {
            let next_line = lines.get(i + 1).copied().unwrap_or("");
            let next_next = lines.get(i + 2).copied().unwrap_or("");
            let concat = format!("{}{}", next_line, next_next);
            if let Some(fm) = RE_PY_DEF_NAME.captures(&concat) {
                let verb = m.get(2).unwrap().as_str().to_uppercase();
                let path_str = m.get(3).unwrap().as_str();
                let name = fm.get(1).unwrap().as_str().to_string();
                push_symbol(
                    &mut out,
                    vec![
                        ("type", json!("route")),
                        ("name", json!(name)),
                        ("sig", json!(format!("{} {}", verb, path_str))),
                        ("line", json!(i + 1)),
                    ],
                );
            }
        }
    }
    out
}

pub fn extract_ts(content: &str) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let lines: Vec<&str> = content.split('\n').collect();
    for (i, l) in lines.iter().enumerate() {
        if let Some(m) = RE_TS_FN.captures(l) {
            let is_async = m.get(2).is_some();
            let name = m.get(3).unwrap().as_str();
            let args = m.get(4).map(|x| x.as_str()).unwrap_or("");
            push_symbol(
                &mut out,
                vec![
                    ("type", json!(if is_async { "async-fn" } else { "fn" })),
                    ("name", json!(name)),
                    ("sig", json!(format!("function {}({})", name, compact_args(args)))),
                    ("line", json!(i + 1)),
                    ("exported", json!(true)),
                ],
            );
            continue; // espelha o `continue` da .cjs — route check fica dead code (F11-01)
        } else if let Some(m) = RE_TS_CONST_HEAD.captures(l) {
            let name = m.get(3).unwrap().as_str().to_string();
            let end = m.get(0).unwrap().end();
            let rest = &l[end..];
            if let Some(arrow) = RE_TS_ARROW.captures(rest) {
                let is_async = arrow.get(1).is_some();
                let args = arrow.get(2).map(|x| x.as_str()).unwrap_or("");
                push_symbol(
                    &mut out,
                    vec![
                        ("type", json!(if is_async { "async-fn" } else { "fn" })),
                        ("name", json!(name.clone())),
                        ("sig", json!(format!("{} = ({}) =>", name, compact_args(args)))),
                        ("line", json!(i + 1)),
                        ("exported", json!(true)),
                    ],
                );
            } else {
                push_symbol(
                    &mut out,
                    vec![
                        ("type", json!("const")),
                        ("name", json!(name.clone())),
                        ("sig", json!(format!("const {}", name))),
                        ("line", json!(i + 1)),
                        ("exported", json!(true)),
                    ],
                );
            }
            continue;
        } else if let Some(m) = RE_TS_CLASS.captures(l) {
            let name = m.get(3).unwrap().as_str().to_string();
            let extends = m.get(4).map(|x| x.as_str().to_string());
            let sig = match extends {
                Some(e) => format!("class {} extends {}", name, e),
                None => format!("class {}", name),
            };
            push_symbol(
                &mut out,
                vec![
                    ("type", json!("class")),
                    ("name", json!(name)),
                    ("sig", json!(sig)),
                    ("line", json!(i + 1)),
                    ("exported", json!(true)),
                ],
            );
            continue;
        } else if let Some(m) = RE_TS_TYPEDECL.captures(l) {
            let kind = m.get(1).unwrap().as_str().to_string();
            let name = m.get(2).unwrap().as_str().to_string();
            push_symbol(
                &mut out,
                vec![
                    ("type", json!(kind.clone())),
                    ("name", json!(name.clone())),
                    ("sig", json!(format!("{} {}", kind, name))),
                    ("line", json!(i + 1)),
                    ("exported", json!(true)),
                ],
            );
            continue;
        }

        if RE_TS_ROUTE.is_match(l) {
            if let Some(last) = out.last_mut() {
                if let Some(obj) = last.as_object_mut() {
                    obj.insert("type".to_string(), json!("route"));
                }
            }
        }
    }
    out
}

pub fn extract_rust(content: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for (i, l) in content.split('\n').enumerate() {
        if let Some(m) = RE_RS_FN.captures(l) {
            let indent = m.get(1).map(|x| x.as_str().len()).unwrap_or(0);
            if indent == 0 {
                let exported = m.get(2).is_some();
                let is_async = m.get(3).is_some();
                let name = m.get(4).unwrap().as_str();
                let args = m.get(5).map(|x| x.as_str()).unwrap_or("");
                push_symbol(
                    &mut out,
                    vec![
                        ("type", json!(if is_async { "async-fn" } else { "fn" })),
                        ("name", json!(name)),
                        ("sig", json!(format!("fn {}({})", name, compact_args(args)))),
                        ("line", json!(i + 1)),
                        ("exported", json!(exported)),
                    ],
                );
                continue;
            }
        }
        if let Some(m) = RE_RS_STRUCT.captures(l) {
            let exported = m.get(1).is_some();
            let name = m.get(2).unwrap().as_str();
            push_symbol(
                &mut out,
                vec![
                    ("type", json!("struct")),
                    ("name", json!(name)),
                    ("sig", json!(format!("struct {}", name))),
                    ("line", json!(i + 1)),
                    ("exported", json!(exported)),
                ],
            );
            continue;
        }
        if let Some(m) = RE_RS_ENUM.captures(l) {
            let exported = m.get(1).is_some();
            let name = m.get(2).unwrap().as_str();
            push_symbol(
                &mut out,
                vec![
                    ("type", json!("enum")),
                    ("name", json!(name)),
                    ("sig", json!(format!("enum {}", name))),
                    ("line", json!(i + 1)),
                    ("exported", json!(exported)),
                ],
            );
            continue;
        }
        if let Some(m) = RE_RS_TRAIT.captures(l) {
            let exported = m.get(1).is_some();
            let name = m.get(2).unwrap().as_str();
            push_symbol(
                &mut out,
                vec![
                    ("type", json!("trait")),
                    ("name", json!(name)),
                    ("sig", json!(format!("trait {}", name))),
                    ("line", json!(i + 1)),
                    ("exported", json!(exported)),
                ],
            );
        }
    }
    out
}

pub fn extract_sql(content: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for (i, l) in content.split('\n').enumerate() {
        if let Some(m) = RE_SQL_TABLE.captures(l) {
            let name = m.get(1).unwrap().as_str();
            push_symbol(
                &mut out,
                vec![
                    ("type", json!("table")),
                    ("name", json!(name)),
                    ("sig", json!(format!("TABLE {}", name))),
                    ("line", json!(i + 1)),
                ],
            );
            continue;
        }
        if let Some(m) = RE_SQL_TYPE.captures(l) {
            let name = m.get(1).unwrap().as_str();
            push_symbol(
                &mut out,
                vec![
                    ("type", json!("type")),
                    ("name", json!(name)),
                    ("sig", json!(format!("TYPE {}", name))),
                    ("line", json!(i + 1)),
                ],
            );
            continue;
        }
        if let Some(m) = RE_SQL_INDEX.captures(l) {
            let name = m.get(1).unwrap().as_str();
            push_symbol(
                &mut out,
                vec![
                    ("type", json!("index")),
                    ("name", json!(name)),
                    ("sig", json!(format!("INDEX {}", name))),
                    ("line", json!(i + 1)),
                ],
            );
        }
    }
    out
}

pub fn extract_go(content: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for (i, l) in content.split('\n').enumerate() {
        if let Some(m) = RE_GO_FN.captures(l) {
            let name = m.get(1).unwrap().as_str();
            let exported = name.chars().next().is_some_and(|c| c.is_ascii_uppercase());
            push_symbol(
                &mut out,
                vec![
                    ("type", json!("fn")),
                    ("name", json!(name)),
                    ("sig", json!(format!("func {}", name))),
                    ("line", json!(i + 1)),
                    ("exported", json!(exported)),
                ],
            );
            continue;
        }
        if let Some(m) = RE_GO_TYPE.captures(l) {
            let name = m.get(1).unwrap().as_str();
            let kind = m.get(2).unwrap().as_str();
            let exported = name.chars().next().is_some_and(|c| c.is_ascii_uppercase());
            push_symbol(
                &mut out,
                vec![
                    ("type", json!(kind)),
                    ("name", json!(name)),
                    ("sig", json!(format!("{} {}", kind, name))),
                    ("line", json!(i + 1)),
                    ("exported", json!(exported)),
                ],
            );
        }
    }
    out
}

pub fn extract(lang: &str, content: &str) -> Vec<Value> {
    match lang {
        "python" => extract_python(content),
        "typescript" | "javascript" => extract_ts(content),
        "rust" => extract_rust(content),
        "sql" => extract_sql(content),
        "go" => extract_go(content),
        _ => Vec::new(),
    }
}

// ── Leitura do repo ──────────────────────────────────────────────────────

pub fn list_tracked_files(project_root: &Path) -> Vec<String> {
    let out = Command::new("git")
        .arg("ls-files")
        .current_dir(project_root)
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            s.split('\n').filter(|x| !x.is_empty()).map(|x| x.to_string()).collect()
        }
        _ => Vec::new(),
    }
}

/// Equivalente a `extractFile(projectRoot, relPath)`. Retorna um Value
/// `{file, lang, mtime, symbols}` ou None se linguagem não suportada /
/// arquivo ausente / grande demais.
pub fn extract_file(project_root: &Path, rel_path: &str) -> Option<Value> {
    let lang = detect_lang(rel_path)?;
    let abs = project_root.join(rel_path);
    let stat = fs::metadata(&abs).ok()?;
    if !stat.is_file() || stat.len() > MAX_FILE_BYTES {
        return None;
    }
    let content = fs::read_to_string(&abs).ok()?;
    let symbols = extract(lang, &content);
    let mtime_ms = stat_mtime_ms(&stat);
    let mut o = Map::new();
    o.insert("file".into(), json!(rel_path));
    o.insert("lang".into(), json!(lang));
    o.insert("mtime".into(), mtime_number(mtime_ms));
    o.insert("symbols".into(), Value::Array(symbols));
    Some(Value::Object(o))
}

fn stat_mtime_ms(stat: &fs::Metadata) -> f64 {
    // Node: `stat.mtimeMs = Number(sec) * 1000 + Number(nsec) / 1e6`.
    // Repete as mesmas ops na mesma ordem para paridade byte-a-byte.
    match stat.modified() {
        Ok(t) => match t.duration_since(UNIX_EPOCH) {
            Ok(d) => (d.as_secs() as f64) * 1000.0 + (d.subsec_nanos() as f64) / 1_000_000.0,
            Err(_) => 0.0,
        },
        Err(_) => 0.0,
    }
}

fn mtime_number(ms: f64) -> Value {
    // Node representa mtimeMs como Number. Se fracional → float, senão int.
    serde_json::Number::from_f64(ms)
        .map(Value::Number)
        .unwrap_or_else(|| Value::Number(0.into()))
}

// ── Reference counting ───────────────────────────────────────────────────

static RE_NAME_FILTER_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(new|get|set|is|to|of|on|in|as|at|do|up|or|if|_)").unwrap());

pub fn count_references(
    project_root: &Path,
    files: &[String],
    symbols_by_file: &Map<String, Value>,
) -> Map<String, Value> {
    let mut unique_names: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (_f, entries) in symbols_by_file.iter() {
        let Some(arr) = entries.as_array() else { continue };
        for s in arr {
            let Some(name) = s.get("name").and_then(Value::as_str) else { continue };
            if utf16_len(name) < 3 {
                continue;
            }
            if RE_NAME_FILTER_PREFIX.is_match(name) {
                continue;
            }
            // exported !== false → include when exported is true OR missing
            let exported = s.get("exported");
            let excluded = matches!(exported, Some(Value::Bool(false)));
            if excluded {
                continue;
            }
            if seen.insert(name.to_string()) {
                unique_names.push(name.to_string());
            }
        }
    }

    let mut refs: Map<String, Value> = Map::new();
    for n in &unique_names {
        refs.insert(n.clone(), json!(0));
    }
    if unique_names.is_empty() {
        return refs;
    }

    // Chunk em 200 nomes
    let chunk_size = 200;
    let mut patterns: Vec<Regex> = Vec::new();
    let mut i = 0;
    while i < unique_names.len() {
        let end = (i + chunk_size).min(unique_names.len());
        let chunk: Vec<String> = unique_names[i..end].iter().map(|n| escape_regex(n)).collect();
        let pat = format!(r"(?-u:\b({})\b)", chunk.join("|"));
        patterns.push(Regex::new(&pat).unwrap());
        i = end;
    }

    // Mapa rel_path → set of own symbol names (para skip).
    // Usado só durante scan, por arquivo.
    let mut own_cache: HashMap<String, HashSet<String>> = HashMap::new();
    for (f, entries) in symbols_by_file.iter() {
        let Some(arr) = entries.as_array() else { continue };
        let mut set = HashSet::new();
        for s in arr {
            if let Some(name) = s.get("name").and_then(Value::as_str) {
                set.insert(name.to_string());
            }
        }
        own_cache.insert(f.clone(), set);
    }

    for rel in files {
        let abs = project_root.join(rel);
        let stat = match fs::metadata(&abs) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if stat.len() > MAX_FILE_BYTES {
            continue;
        }
        let content = match fs::read_to_string(&abs) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let empty = HashSet::new();
        let own = own_cache.get(rel).unwrap_or(&empty);
        for re in &patterns {
            for cap in re.captures_iter(&content) {
                let name = cap.get(1).unwrap().as_str();
                if own.contains(name) {
                    continue;
                }
                let entry = refs.get_mut(name).unwrap();
                let cur = entry.as_u64().unwrap_or(0);
                *entry = json!(cur + 1);
            }
        }
    }

    refs
}

// ── I/O index ────────────────────────────────────────────────────────────

pub fn load_index(mem_dir: &Path) -> Option<Value> {
    let file = mem_dir.join(INDEX_JSON);
    let raw = fs::read_to_string(&file).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn save_index(mem_dir: &Path, index: &Value) -> Result<(), IndexerError> {
    let path = mem_dir.join(INDEX_JSON);
    let body = serde_json::to_string_pretty(index)?;
    fs::write(&path, body)?;
    write_index_md(mem_dir, index)?;
    Ok(())
}

// ── Rebuild / Update ─────────────────────────────────────────────────────

pub fn rebuild_index(
    mem_dir: &Path,
    project_root: &Path,
    clock: &dyn Clock,
) -> Result<Value, IndexerError> {
    let tracked = list_tracked_files(project_root);
    if tracked.is_empty() {
        let mut m = Map::new();
        m.insert("files".into(), Value::Object(Map::new()));
        m.insert("generated_at".into(), json!(clock.now_iso()));
        return Ok(Value::Object(m));
    }

    let files: Vec<String> = tracked
        .iter()
        .filter(|f| detect_lang(f).is_some())
        .take(MAX_FILES)
        .cloned()
        .collect();

    let mut symbols_by_file: Map<String, Value> = Map::new();
    let mut meta: HashMap<String, (String, f64)> = HashMap::new();

    for f in &files {
        let Some(entry) = extract_file(project_root, f) else { continue };
        let lang = entry
            .get("lang")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mtime = entry.get("mtime").and_then(Value::as_f64).unwrap_or(0.0);
        let symbols = entry.get("symbols").cloned().unwrap_or(Value::Array(Vec::new()));
        symbols_by_file.insert(f.clone(), symbols);
        meta.insert(f.clone(), (lang, mtime));
    }

    let refs = count_references(project_root, &tracked, &symbols_by_file);

    let mut files_map: Map<String, Value> = Map::new();
    for (f, symbols_val) in symbols_by_file.iter() {
        let (lang, mtime) = meta.get(f).cloned().unwrap_or((String::new(), 0.0));
        let symbols = symbols_val.as_array().cloned().unwrap_or_default();
        let with_refs: Vec<Value> = symbols
            .into_iter()
            .map(|mut s| {
                let name = s.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                let r = refs.get(&name).and_then(Value::as_u64).unwrap_or(0);
                if let Some(obj) = s.as_object_mut() {
                    obj.insert("refs".into(), json!(r));
                }
                s
            })
            .collect();
        let mut fe = Map::new();
        fe.insert("lang".into(), json!(lang));
        fe.insert("mtime".into(), mtime_number(mtime));
        fe.insert("symbols".into(), Value::Array(with_refs));
        files_map.insert(f.clone(), Value::Object(fe));
    }

    let mut index_obj = Map::new();
    index_obj.insert("version".into(), json!(INDEX_VERSION));
    index_obj.insert("generated_at".into(), json!(clock.now_iso()));
    index_obj.insert("files".into(), Value::Object(files_map));
    let index = Value::Object(index_obj);

    save_index(mem_dir, &index)?;
    Ok(index)
}

pub fn update_index_for_files(
    mem_dir: &Path,
    project_root: &Path,
    changed_files: &[String],
    clock: &dyn Clock,
) -> Result<Value, IndexerError> {
    let Some(mut index) = load_index(mem_dir) else {
        return rebuild_index(mem_dir, project_root, clock);
    };
    let files_obj = index
        .as_object_mut()
        .and_then(|o| o.get_mut("files"))
        .and_then(Value::as_object_mut);
    let Some(files_obj) = files_obj else {
        return rebuild_index(mem_dir, project_root, clock);
    };

    for f in changed_files {
        let abs = project_root.join(f);
        if !abs.exists() {
            files_obj.shift_remove(f);
            continue;
        }
        let Some(entry) = extract_file(project_root, f) else {
            files_obj.shift_remove(f);
            continue;
        };
        let lang = entry.get("lang").cloned().unwrap_or(Value::Null);
        let mtime = entry.get("mtime").cloned().unwrap_or(Value::Null);
        let symbols = entry
            .get("symbols")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let with_refs: Vec<Value> = symbols
            .into_iter()
            .map(|mut s| {
                if let Some(obj) = s.as_object_mut() {
                    obj.insert("refs".into(), json!(0));
                }
                s
            })
            .collect();
        let mut fe = Map::new();
        fe.insert("lang".into(), lang);
        fe.insert("mtime".into(), mtime);
        fe.insert("symbols".into(), Value::Array(with_refs));
        files_obj.insert(f.clone(), Value::Object(fe));
    }

    // index.generated_at = new Date().toISOString() — key exists, stays in place
    if let Some(obj) = index.as_object_mut() {
        obj.insert("generated_at".into(), json!(clock.now_iso()));
    }
    save_index(mem_dir, &index)?;
    Ok(index)
}

// ── Ranking ──────────────────────────────────────────────────────────────

pub fn rank_symbols(index: &Value, graph: Option<&Value>, now_ms: i64) -> Vec<Value> {
    const DAY_MS: f64 = 86_400_000.0;
    let mut hot_count: HashMap<String, f64> = HashMap::new();
    if let Some(g) = graph {
        if let Some(nodes) = g.get("nodes").and_then(Value::as_object) {
            for (_, n) in nodes.iter() {
                if n.get("kind").and_then(Value::as_str) == Some("file") {
                    let label = n.get("label").and_then(Value::as_str).unwrap_or("").to_string();
                    let count = n.get("count").and_then(Value::as_f64).unwrap_or(0.0);
                    hot_count.insert(label, count);
                }
            }
        }
    }
    let max_hot = hot_count.values().copied().fold(1.0_f64, f64::max);

    let mut all: Vec<Value> = Vec::new();
    let Some(files) = index.get("files").and_then(Value::as_object) else { return all };
    let now = now_ms as f64;
    for (file, entry) in files.iter() {
        let hot = hot_count.get(file).copied().unwrap_or(0.0) / max_hot;
        let mtime = entry.get("mtime").and_then(Value::as_f64).unwrap_or(now);
        let age_days = ((now - mtime) / DAY_MS).max(0.0);
        let recency = (-age_days / 30.0).exp();
        let lang = entry.get("lang").and_then(Value::as_str).unwrap_or("").to_string();
        let Some(syms) = entry.get("symbols").and_then(Value::as_array) else { continue };
        for s in syms {
            let refs = s.get("refs").and_then(Value::as_f64).unwrap_or(0.0);
            let refs_score = (1.0_f64 + refs).log2() / 6.0;
            let score = (hot * 0.5) + (refs_score * 0.3) + (recency * 0.2);
            // { file, lang, score, ...s } — file/lang/score first, then s fields
            let mut obj = Map::new();
            obj.insert("file".into(), json!(file));
            obj.insert("lang".into(), json!(lang.clone()));
            obj.insert("score".into(), json!(score));
            if let Some(sobj) = s.as_object() {
                for (k, v) in sobj.iter() {
                    obj.insert(k.clone(), v.clone());
                }
            }
            all.push(Value::Object(obj));
        }
    }
    all.sort_by(|a, b| {
        let sa = a.get("score").and_then(Value::as_f64).unwrap_or(0.0);
        let sb = b.get("score").and_then(Value::as_f64).unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    all
}

// ── Renderização MD ──────────────────────────────────────────────────────

pub fn write_index_md(mem_dir: &Path, index: &Value) -> Result<(), IndexerError> {
    let graph_file = mem_dir.join("graph.json");
    let graph: Option<Value> = fs::read_to_string(&graph_file)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok());

    let now_ms = SystemClock.now_ms();
    let ranked = rank_symbols(index, graph.as_ref(), now_ms);
    let total_symbols = ranked.len();
    let total_files = index
        .get("files")
        .and_then(Value::as_object)
        .map(|o| o.len())
        .unwrap_or(0);

    // byLang preserva insertion order (primeira ocorrência)
    let mut by_lang: Vec<(String, u64)> = Vec::new();
    for r in &ranked {
        let lang = r.get("lang").and_then(Value::as_str).unwrap_or("").to_string();
        if let Some(pos) = by_lang.iter().position(|(k, _)| k == &lang) {
            by_lang[pos].1 += 1;
        } else {
            by_lang.push((lang, 1));
        }
    }
    let lang_parts: Vec<String> = by_lang
        .iter()
        .map(|(l, n)| format!("{} ({})", l, n))
        .collect();

    let generated_at = index
        .get("generated_at")
        .and_then(Value::as_str)
        .unwrap_or("");

    let mut out: Vec<String> = Vec::new();
    out.push("# Code Index".to_string());
    out.push(format!("> Gerado em: {}", generated_at));
    out.push(format!(
        "> {} arquivos · {} símbolos · linguagens: {}",
        total_files,
        total_symbols,
        lang_parts.join(", ")
    ));
    out.push(String::new());

    out.push("## Top símbolos (por importância)".to_string());
    out.push(String::new());
    for s in ranked.iter().take(40) {
        let name = s.get("name").and_then(Value::as_str).unwrap_or("");
        let typ = s.get("type").and_then(Value::as_str).unwrap_or("");
        let file = s.get("file").and_then(Value::as_str).unwrap_or("");
        let line = s.get("line").and_then(Value::as_i64).unwrap_or(0);
        let refs = s.get("refs").and_then(Value::as_f64).unwrap_or(0.0);
        let refs_str = if refs != 0.0 {
            format!(" · {} refs", refs_to_str(refs))
        } else {
            String::new()
        };
        out.push(format!(
            "- **`{}`** ({}) — `{}:{}`{}",
            name, typ, file, line, refs_str
        ));
        let sig = s.get("sig").and_then(Value::as_str).unwrap_or("");
        if !sig.is_empty() && sig != name {
            out.push(format!("  `{}`", sig));
        }
    }
    out.push(String::new());

    // byFile: insertion order por arquivo (primeira ocorrência em ranked)
    let mut by_file: Vec<(String, Vec<Value>)> = Vec::new();
    for r in &ranked {
        let file = r.get("file").and_then(Value::as_str).unwrap_or("").to_string();
        if let Some(pos) = by_file.iter().position(|(k, _)| k == &file) {
            by_file[pos].1.push(r.clone());
        } else {
            by_file.push((file, vec![r.clone()]));
        }
    }
    // Sort estável por count desc, slice 15
    by_file.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    let top_files: Vec<_> = by_file.into_iter().take(15).collect();

    out.push("## Arquivos mais densos".to_string());
    out.push(String::new());
    for (file, syms) in &top_files {
        out.push(format!("### `{}` _({} símbolos)_", file, syms.len()));
        for s in syms.iter().take(8) {
            let sig = s.get("sig").and_then(Value::as_str).unwrap_or("");
            let name = s.get("name").and_then(Value::as_str).unwrap_or("");
            let line = s.get("line").and_then(Value::as_i64).unwrap_or(0);
            let label = if !sig.is_empty() { sig } else { name };
            out.push(format!("- `{}` — L{}", label, line));
        }
        if syms.len() > 8 {
            out.push(format!("- _... mais {}_", syms.len() - 8));
        }
        out.push(String::new());
    }

    let md_path = mem_dir.join(INDEX_MD);
    let _ = fs::write(&md_path, out.join("\n"));
    Ok(())
}

fn refs_to_str(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

// ── Bloco injetável ──────────────────────────────────────────────────────

pub fn get_context_lines(mem_dir: &Path, max_symbols: usize) -> Vec<String> {
    let Some(index) = load_index(mem_dir) else { return Vec::new() };
    let files_empty = index
        .get("files")
        .and_then(Value::as_object)
        .map(|o| o.is_empty())
        .unwrap_or(true);
    if files_empty {
        return Vec::new();
    }

    let graph_file = mem_dir.join("graph.json");
    let graph: Option<Value> = fs::read_to_string(&graph_file)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok());

    let mut hot_files: HashSet<String> = HashSet::new();
    if let Some(g) = graph.as_ref() {
        if let Some(nodes) = g.get("nodes").and_then(Value::as_object) {
            let mut file_nodes: Vec<(&str, f64)> = nodes
                .iter()
                .filter(|(_, n)| n.get("kind").and_then(Value::as_str) == Some("file"))
                .map(|(_, n)| {
                    (
                        n.get("label").and_then(Value::as_str).unwrap_or(""),
                        n.get("count").and_then(Value::as_f64).unwrap_or(0.0),
                    )
                })
                .collect();
            // Stable sort desc by count (matches JS Array.sort stability + b.count - a.count)
            file_nodes.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            for (label, _) in file_nodes.into_iter().take(10) {
                hot_files.insert(label.to_string());
            }
        }
    }

    let now_ms = SystemClock.now_ms();
    let ranked: Vec<Value> = rank_symbols(&index, graph.as_ref(), now_ms)
        .into_iter()
        .filter(|s| {
            hot_files.is_empty()
                || s.get("file")
                    .and_then(Value::as_str)
                    .map(|f| hot_files.contains(f))
                    .unwrap_or(false)
        })
        .take(max_symbols)
        .collect();

    if ranked.is_empty() {
        return Vec::new();
    }

    let mut lines = vec!["## Code Index (símbolos reutilizáveis)".to_string()];
    for s in &ranked {
        let name = s.get("name").and_then(Value::as_str).unwrap_or("");
        let typ = s.get("type").and_then(Value::as_str).unwrap_or("");
        let file = s.get("file").and_then(Value::as_str).unwrap_or("");
        let line = s.get("line").and_then(Value::as_i64).unwrap_or(0);
        let refs = s.get("refs").and_then(Value::as_f64).unwrap_or(0.0);
        let refs_str = if refs != 0.0 {
            format!(" _{}↑_", refs_to_str(refs))
        } else {
            String::new()
        };
        lines.push(format!("- `{}` {} — `{}:{}`{}", name, typ, file, line, refs_str));
    }
    lines
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_args_empty() {
        assert_eq!(compact_args(""), "");
    }

    #[test]
    fn compact_args_short() {
        assert_eq!(compact_args("a, b"), "a, b");
    }

    #[test]
    fn compact_args_collapses_whitespace() {
        assert_eq!(compact_args("  a,   b   c  "), "a, b c");
    }

    #[test]
    fn compact_args_truncates_long() {
        let long = "a".repeat(80);
        let got = compact_args(&long);
        assert_eq!(utf16_len(&got), 60);
        assert!(got.ends_with("..."));
    }

    #[test]
    fn escape_regex_basic() {
        assert_eq!(escape_regex("a.b*c"), "a\\.b\\*c");
        assert_eq!(escape_regex("(foo)|bar"), "\\(foo\\)\\|bar");
        assert_eq!(escape_regex("normal_name"), "normal_name");
    }

    #[test]
    fn detect_lang_extensions() {
        assert_eq!(detect_lang("x.py"), Some("python"));
        assert_eq!(detect_lang("X.PYI"), Some("python"));
        assert_eq!(detect_lang("a/b/c.ts"), Some("typescript"));
        assert_eq!(detect_lang("a/b/c.tsx"), Some("typescript"));
        assert_eq!(detect_lang("a.cjs"), Some("javascript"));
        assert_eq!(detect_lang("a.mjs"), Some("javascript"));
        assert_eq!(detect_lang("a.rs"), Some("rust"));
        assert_eq!(detect_lang("a.sql"), Some("sql"));
        assert_eq!(detect_lang("a.go"), Some("go"));
        assert_eq!(detect_lang("a.txt"), None);
        assert_eq!(detect_lang("noext"), None);
    }

    #[test]
    fn extract_python_top_level_def() {
        let syms = extract_python("def foo(a, b):\n    pass\n");
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0]["type"], "fn");
        assert_eq!(syms[0]["name"], "foo");
        assert_eq!(syms[0]["sig"], "def foo(a, b)");
        assert_eq!(syms[0]["line"], 1);
    }

    #[test]
    fn extract_python_skips_indented() {
        let syms = extract_python("class Foo:\n    def method(self):\n        pass\n");
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0]["type"], "class");
    }

    #[test]
    fn extract_python_async_and_return() {
        let syms = extract_python("async def bar(x: int) -> str:\n    pass\n");
        assert_eq!(syms[0]["type"], "async-fn");
        assert_eq!(syms[0]["sig"], "def bar(x: int) -> str");
    }

    #[test]
    fn extract_python_fastapi_route() {
        let src = "@router.get(\"/users\")\nasync def list_users():\n    pass\n";
        let syms = extract_python(src);
        // Should produce BOTH the route (from @router match) AND the async-fn (from def match)
        assert!(syms.iter().any(|s| s["type"] == "route" && s["name"] == "list_users"));
        assert!(syms.iter().any(|s| s["type"] == "async-fn" && s["name"] == "list_users"));
    }

    #[test]
    fn extract_ts_export_function() {
        let syms = extract_ts("export function hello(x: number) {}\n");
        assert_eq!(syms[0]["type"], "fn");
        assert_eq!(syms[0]["name"], "hello");
        assert_eq!(syms[0]["exported"], true);
    }

    #[test]
    fn extract_ts_arrow_const() {
        let syms = extract_ts("export const add = (a: number, b: number) => a + b;\n");
        assert_eq!(syms[0]["type"], "fn");
        assert_eq!(syms[0]["sig"], "add = (a: number, b: number) =>");
    }

    #[test]
    fn extract_ts_plain_const() {
        let syms = extract_ts("export const PI = 3.14;\n");
        assert_eq!(syms[0]["type"], "const");
        assert_eq!(syms[0]["sig"], "const PI");
    }

    #[test]
    fn extract_ts_class_extends() {
        let syms = extract_ts("export class Foo extends Bar {}\n");
        assert_eq!(syms[0]["type"], "class");
        assert_eq!(syms[0]["sig"], "class Foo extends Bar");
    }

    #[test]
    fn extract_ts_route_branch_is_dead_code() {
        // F11-01: em .cjs o fn-regex já casa `export function GET(...)` e
        // faz `continue`, então o overwrite para 'route' é inalcançável.
        let syms = extract_ts("export async function GET(req: Request) {}\n");
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0]["type"], "async-fn");
    }

    #[test]
    fn extract_rust_pub_fn() {
        let syms = extract_rust("pub fn foo(x: i32) -> i32 { x }\n");
        assert_eq!(syms[0]["type"], "fn");
        assert_eq!(syms[0]["exported"], true);
    }

    #[test]
    fn extract_rust_struct_enum_trait() {
        let src = "pub struct S;\nenum E { A, B }\npub trait T {}\n";
        let syms = extract_rust(src);
        assert_eq!(syms.len(), 3);
        assert_eq!(syms[0]["type"], "struct");
        assert_eq!(syms[0]["exported"], true);
        assert_eq!(syms[1]["type"], "enum");
        assert_eq!(syms[1]["exported"], false);
        assert_eq!(syms[2]["type"], "trait");
    }

    #[test]
    fn extract_sql_all_kinds() {
        let src = "CREATE TABLE users (id INT);\ncreate type role as enum('a');\nCREATE UNIQUE INDEX idx_users_email ON users(email);\n";
        let syms = extract_sql(src);
        assert_eq!(syms.len(), 3);
        assert_eq!(syms[0]["type"], "table");
        assert_eq!(syms[0]["name"], "users");
        assert_eq!(syms[1]["type"], "type");
        assert_eq!(syms[2]["type"], "index");
    }

    #[test]
    fn extract_go_exported_vs_unexported() {
        let src = "func Foo(x int) int { return x }\nfunc bar() {}\ntype Widget struct { x int }\n";
        let syms = extract_go(src);
        assert_eq!(syms[0]["exported"], true);
        assert_eq!(syms[1]["exported"], false);
        assert_eq!(syms[2]["type"], "struct");
        assert_eq!(syms[2]["exported"], true);
    }

    #[test]
    fn format_iso_millis_epoch() {
        assert_eq!(format_iso_millis(0), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn format_iso_millis_known() {
        // 2026-04-19T12:34:56.789Z
        let dt = 1_776_000_000_000_i64 + 12 * 3600 * 1000 + 34 * 60 * 1000 + 56_000 + 789;
        // We don't assert specific date, just that the format is ISO
        let s = format_iso_millis(dt);
        assert!(s.ends_with("Z"));
        assert_eq!(s.len(), 24);
    }
}
