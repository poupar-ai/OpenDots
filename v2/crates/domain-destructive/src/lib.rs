//! # domain-destructive
//!
//! Porta Rust de `plugin/scripts/destructive.cjs` — **detecção
//! server-side de intents destrutivos** antes de qualquer agente.
//!
//! Patterns CORE são imutáveis (embutidos no binário). Patterns USER
//! ficam em `<mem_dir>/destructive_patterns.json` e são editáveis.
//! IDs do CORE podem ser desabilitados mas não editados/apagados.
//!
//! ## Escopo (Fase 7)
//!
//! - [`CORE_PATTERNS`] estático.
//! - [`load_user_config`] / [`list_patterns`].
//! - [`save_user_patterns`] — normalização + `id` aleatório quando
//!   ausente.
//! - [`set_core_disabled`].
//! - [`detect`] / [`test_pattern`] — execução dos regex.
//! - [`compile_user_pattern`] — literal/regex com suporte a `*`.
//!
//! Paridade byte-a-byte validada por `tests/parity_*.rs` contra
//! `destructive.cjs`.

use rand::RngCore;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use thiserror::Error;

// ── CORE ─────────────────────────────────────────────────────────────────

/// Pattern CORE — imutável, compilado uma vez via [`LazyLock`].
#[derive(Debug)]
pub struct CorePattern {
    pub id: &'static str,
    pub label: &'static str,
    /// Texto cru do regex (paridade com `regex.source` em JS).
    pub text: &'static str,
    pub severity: &'static str,
    /// Se `true`, equivale à flag `/i` do JS (case-insensitive).
    pub insensitive: bool,
    /// Compilado em uso — LazyLock interno.
    compiled: LazyLock<Regex>,
}

impl CorePattern {
    /// Retorna o regex compilado. O primeiro acesso compila via
    /// `Regex::new`.
    pub fn regex(&self) -> &Regex {
        &self.compiled
    }
}

/// Helper para declarar patterns em `CORE_PATTERNS`.
macro_rules! core {
    ($id:expr, $label:expr, $src:expr, $sev:expr, $i:expr $(,)?) => {
        CorePattern {
            id: $id,
            label: $label,
            text: $src,
            severity: $sev,
            insensitive: $i,
            compiled: LazyLock::new(|| {
                let pat = if $i {
                    format!("(?i){}", $src)
                } else {
                    ($src).to_string()
                };
                Regex::new(&pat).expect(concat!("invalid CORE regex: ", $id))
            }),
        }
    };
}

/// Patterns CORE — 14 entradas cobrindo PT + EN + SQL + shell.
///
/// Paridade linha-a-linha com `destructive.cjs:19-104`. Ordem
/// preservada — afeta ordem dos hits em `detect()`.
pub static CORE_PATTERNS: LazyLock<Vec<CorePattern>> = LazyLock::new(|| {
    vec![
        core!(
            "core.apagar_tudo",
            "Apagar tudo / todo o projeto / repositório / código",
            r"\b(apag\w+|delet\w+|remov\w+|limp\w+|destru\w+|deleta)\b[^.!?\n]{0,40}\b(tudo|todos? os arquivos?|todo o (projeto|reposit[óo]rio|c[óo]digo|sistema)|o reposit[óo]rio|o projeto|o c[óo]digo|a branch|o banco|a base( de dados)?|as bases?|as mensagens?|as mem[óo]rias?)",
            "critical",
            true,
        ),
        core!(
            "core.delete_everything_en",
            "Delete everything / all / repo / project (EN)",
            r"\b(delete|remove|wipe|drop|nuke|destroy|purge)\b[^.!?\n]{0,40}\b(everything|all( files| data)?|the (repo|repository|project|code(base)?|branch|database|db))\b",
            "critical",
            true,
        ),
        core!(
            "core.rm_rf",
            "rm -rf (comando shell destrutivo)",
            r"\brm\s+-[rRf]{1,3}\b",
            "critical",
            false,
        ),
        core!(
            "core.git_reset_hard",
            "git reset --hard",
            r"\bgit\s+reset\s+--hard\b",
            "critical",
            true,
        ),
        core!(
            "core.git_clean_force",
            "git clean -f / -fd / -fdx",
            r"\bgit\s+clean\s+-[fdxFDX]{1,3}\b",
            "critical",
            true,
        ),
        core!(
            "core.git_push_force",
            "git push --force / -f",
            r"\bgit\s+push\s+(--force\b|-f\b)",
            "high",
            true,
        ),
        core!(
            "core.git_branch_delete",
            "git branch -D (deletar branch forçado)",
            r"\bgit\s+branch\s+-D\b",
            "high",
            true,
        ),
        core!(
            "core.drop_database",
            "DROP DATABASE / TABLE / SCHEMA",
            r"\bDROP\s+(DATABASE|TABLE|SCHEMA|INDEX)\b",
            "critical",
            true,
        ),
        core!(
            "core.truncate_table",
            "TRUNCATE TABLE",
            r"\bTRUNCATE\s+TABLE\b",
            "critical",
            true,
        ),
        core!(
            "core.delete_from_no_where",
            "DELETE FROM sem WHERE",
            r"\bDELETE\s+FROM\s+\w+\s*(;|$)",
            "critical",
            true,
        ),
        core!(
            "core.format_disk",
            "format / mkfs (formatar disco)",
            r"\b(mkfs|format)\s+(\/dev|c:|disk|hd|ssd)",
            "critical",
            true,
        ),
        core!(
            "core.dd_of_dev",
            "dd if=... of=/dev/...",
            r"\bdd\s+.*\bof=\/dev\/",
            "critical",
            true,
        ),
        core!(
            "core.chmod_777_root",
            "chmod 777 em raiz",
            r"\bchmod\s+-?R?\s*777\s+\/(\s|$)",
            "high",
            true,
        ),
        core!(
            "core.shutdown",
            "shutdown / poweroff / halt / reboot",
            r"\b(shutdown\s+-h|poweroff|halt|reboot\s+now)\b",
            "high",
            true,
        ),
    ]
});

// ── USER paths ───────────────────────────────────────────────────────────

/// `<mem_dir>/destructive_patterns.json`.
pub fn user_patterns_path(mem_dir: &Path) -> PathBuf {
    mem_dir.join("destructive_patterns.json")
}

/// Config do usuário — patterns + lista de core IDs desabilitados.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct UserConfig {
    pub patterns: Vec<Value>,
    pub disabled_core: Vec<String>,
}

/// Lê `destructive_patterns.json`. Tolerante a parse errors: retorna
/// config vazia em qualquer falha. Replica `loadUserConfig` em
/// `destructive.cjs:117-129`.
pub fn load_user_config(mem_dir: &Path) -> UserConfig {
    let raw = match fs::read_to_string(user_patterns_path(mem_dir)) {
        Ok(r) => r,
        Err(_) => return UserConfig::default(),
    };
    let parsed: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return UserConfig::default(),
    };
    let patterns = parsed
        .get("patterns")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let disabled_core = parsed
        .get("disabled_core")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    UserConfig {
        patterns,
        disabled_core,
    }
}

/// Atalho pra `load_user_config(...).patterns`.
/// Replica `loadUserPatterns` em `destructive.cjs:131-133`.
pub fn load_user_patterns(mem_dir: &Path) -> Vec<Value> {
    load_user_config(mem_dir).patterns
}

// ── ID generation ────────────────────────────────────────────────────────

/// Provedor de IDs aleatórios — abstraído pra testes.
///
/// Default: 6 bytes aleatórios hex (12 chars), igual ao JS
/// `crypto.randomBytes(6).toString('hex')`.
pub trait IdGen {
    /// Gera um novo id no formato `user.<hex>`.
    fn new_id(&self) -> String;
}

/// Default: `rand::rng` → 6 bytes hex.
#[derive(Debug, Default, Clone, Copy)]
pub struct RandomIdGen;

impl IdGen for RandomIdGen {
    fn new_id(&self) -> String {
        use std::fmt::Write;
        let mut bytes = [0u8; 6];
        rand::rng().fill_bytes(&mut bytes);
        let mut hex = String::with_capacity(12);
        for b in bytes {
            let _ = write!(hex, "{b:02x}");
        }
        format!("user.{hex}")
    }
}

/// Gerador fixo — retorna o mesmo id sempre. Útil em testes.
#[derive(Debug, Clone)]
pub struct FixedIdGen(pub String);

impl IdGen for FixedIdGen {
    fn new_id(&self) -> String {
        self.0.clone()
    }
}

// ── Time (for created_at) ────────────────────────────────────────────────

/// Provedor de tempo ISO pra `created_at`. Injetado nos testes.
pub trait IsoClock {
    fn now_iso(&self) -> String;
}

/// Clock do sistema. **Observação:** formato deve casar byte-a-byte com
/// `new Date().toISOString()` (inclui `.sssZ`).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemIsoClock;

impl IsoClock for SystemIsoClock {
    fn now_iso(&self) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_default();
        format_iso_millis(ms)
    }
}

/// Clock fixo pra testes.
#[derive(Debug, Clone)]
pub struct FixedIsoClock(pub String);

impl IsoClock for FixedIsoClock {
    fn now_iso(&self) -> String {
        self.0.clone()
    }
}

/// ISO 8601 UTC com millis. Copiado de `domain-bus` pra evitar
/// dependência cruzada.
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

// ── Save user patterns ───────────────────────────────────────────────────

/// Normaliza + persiste patterns do usuário.
///
/// Replica `saveUserPatterns` em `destructive.cjs:135-152`. Patterns
/// com `label` ou `text` vazios são descartados depois da normalização.
///
/// - `id` ausente → `id_gen.new_id()` (default: `RandomIdGen`).
/// - `label`: string, max 200 chars UTF-16.
/// - `text`: string, max 400 chars UTF-16.
/// - `kind`: `'regex'` ou `'literal'` (default literal).
/// - `severity`: `'critical' | 'high' | 'medium'` (default high).
/// - `enabled`: default true (`pt.enabled !== false` no JS).
/// - `created_at`: `iso_clock.now_iso()` se ausente.
pub fn save_user_patterns(
    mem_dir: &Path,
    patterns: &[Value],
    id_gen: &dyn IdGen,
    clock: &dyn IsoClock,
) -> Vec<Value> {
    let current = load_user_config(mem_dir);
    let normalized: Vec<Value> = patterns
        .iter()
        .map(|pt| normalize_pattern(pt, id_gen, clock))
        .filter(|pt| {
            let label = pt.get("label").and_then(Value::as_str).unwrap_or("");
            let text = pt.get("text").and_then(Value::as_str).unwrap_or("");
            !label.is_empty() && !text.is_empty()
        })
        .collect();

    let mut root = Map::new();
    root.insert("patterns".into(), Value::Array(normalized.clone()));
    root.insert(
        "disabled_core".into(),
        Value::Array(
            current
                .disabled_core
                .iter()
                .map(|s| Value::String(s.clone()))
                .collect(),
        ),
    );
    let s = serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default();
    let _ = fs::write(user_patterns_path(mem_dir), s);
    normalized
}

fn normalize_pattern(pt: &Value, id_gen: &dyn IdGen, clock: &dyn IsoClock) -> Value {
    // `pt.id || id_gen.new_id()` — falsy (string vazia, null, undefined,
    // 0) cai no else.
    let id = match pt.get("id") {
        Some(v) if truthy(v) => v.as_str().unwrap_or_default().to_string(),
        _ => id_gen.new_id(),
    };
    let label = slice_utf16(str_of(pt.get("label")), 200);
    let text = slice_utf16(str_of(pt.get("text")), 400);
    let kind = match pt.get("kind").and_then(Value::as_str) {
        Some("regex") => "regex",
        _ => "literal",
    };
    let severity = match pt.get("severity").and_then(Value::as_str) {
        Some("critical") => "critical",
        Some("high") => "high",
        Some("medium") => "medium",
        _ => "high",
    };
    // `pt.enabled !== false` → qualquer coisa que não seja explicitamente
    // `false` (boolean) é true.
    let enabled = !matches!(pt.get("enabled"), Some(Value::Bool(false)));
    let created_at = match pt.get("created_at") {
        Some(v) if truthy(v) => v.as_str().unwrap_or_default().to_string(),
        _ => clock.now_iso(),
    };

    let mut m = Map::new();
    m.insert("id".into(), Value::String(id));
    m.insert("label".into(), Value::String(label));
    m.insert("text".into(), Value::String(text));
    m.insert("kind".into(), Value::String(kind.into()));
    m.insert("severity".into(), Value::String(severity.into()));
    m.insert("enabled".into(), Value::Bool(enabled));
    m.insert("created_at".into(), Value::String(created_at));
    Value::Object(m)
}

/// `String(v || '')`. Casa com `String(pt.label || '')` do JS — null,
/// undefined, false, 0, "" viram "".
fn str_of(v: Option<&Value>) -> &str {
    match v {
        Some(Value::String(s)) if !s.is_empty() => s.as_str(),
        _ => "",
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `s.slice(0, n)` em UTF-16 — pra casar com `String.prototype.slice`.
fn slice_utf16(s: &str, n: usize) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    if units.len() <= n {
        return s.to_string();
    }
    String::from_utf16_lossy(&units[..n])
}

// ── Enable/disable CORE ──────────────────────────────────────────────────

/// Erro de [`set_core_disabled`].
#[derive(Debug, Error)]
pub enum SetCoreDisabledError {
    #[error("unknown core pattern id: {0}")]
    UnknownId(String),
}

/// Liga/desliga um pattern CORE. Replica `setCoreDisabled` em
/// `destructive.cjs:155-167`.
pub fn set_core_disabled(
    mem_dir: &Path,
    core_id: &str,
    disabled: bool,
) -> Result<Vec<String>, SetCoreDisabledError> {
    if !CORE_PATTERNS.iter().any(|p| p.id == core_id) {
        return Err(SetCoreDisabledError::UnknownId(core_id.to_string()));
    }
    let current = load_user_config(mem_dir);
    let mut set: Vec<String> = current.disabled_core.clone();
    if disabled {
        if !set.iter().any(|s| s == core_id) {
            set.push(core_id.to_string());
        }
    } else {
        set.retain(|s| s != core_id);
    }

    let mut root = Map::new();
    root.insert("patterns".into(), Value::Array(current.patterns));
    root.insert(
        "disabled_core".into(),
        Value::Array(set.iter().map(|s| Value::String(s.clone())).collect()),
    );
    let s = serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default();
    let _ = fs::write(user_patterns_path(mem_dir), s);
    Ok(set)
}

// ── Compile user pattern ─────────────────────────────────────────────────

/// Compila um pattern do usuário. Retorna `None` se desabilitado ou
/// regex inválido. Replica `compileUserPattern` em
/// `destructive.cjs:171-183`.
pub fn compile_user_pattern(pt: &Value) -> Option<Regex> {
    let enabled = !matches!(pt.get("enabled"), Some(Value::Bool(false)));
    if !enabled {
        return None;
    }
    let text = pt.get("text").and_then(Value::as_str).unwrap_or("");
    let kind = pt.get("kind").and_then(Value::as_str).unwrap_or("literal");
    let pattern = if kind == "regex" {
        format!("(?i){text}")
    } else {
        let escaped = escape_literal_with_wildcard(text);
        format!(r"(?i)\b{escaped}\b")
    };
    Regex::new(&pattern).ok()
}

/// Escapa literal preservando `*` como `.*`. Replica regex JS:
/// `text.replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*')`.
fn escape_literal_with_wildcard(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '.' | '+' | '?' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '*' => out.push_str(".*"),
            _ => out.push(c),
        }
    }
    out
}

// ── listPatterns ─────────────────────────────────────────────────────────

/// Lista patterns — CORE + USER. Replica `listPatterns` em
/// `destructive.cjs:186-210`.
pub fn list_patterns(mem_dir: &Path) -> Value {
    let cfg = load_user_config(mem_dir);
    let disabled: std::collections::HashSet<&str> =
        cfg.disabled_core.iter().map(|s| s.as_str()).collect();

    let user: Vec<Value> = cfg
        .patterns
        .iter()
        .map(|p| {
            // Preserva ordem: id, label, text, kind, severity, enabled,
            // origin, created_at.
            json!({
                "id": p.get("id").cloned().unwrap_or(Value::Null),
                "label": p.get("label").cloned().unwrap_or(Value::Null),
                "text": p.get("text").cloned().unwrap_or(Value::Null),
                "kind": p.get("kind").cloned().unwrap_or(Value::Null),
                "severity": p.get("severity").cloned().unwrap_or(Value::Null),
                "enabled": p.get("enabled").cloned().unwrap_or(Value::Bool(true)),
                "origin": "user",
                "created_at": p.get("created_at").cloned().unwrap_or(Value::Null),
            })
        })
        .collect();

    let core: Vec<Value> = CORE_PATTERNS
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "label": p.label,
                "text": p.text,
                "kind": "regex",
                "severity": p.severity,
                "enabled": !disabled.contains(p.id),
                "origin": "core",
                "readonly": true,
            })
        })
        .collect();

    json!({ "core": core, "user": user })
}

// ── detect ───────────────────────────────────────────────────────────────

/// Hit de [`detect`]. Paridade byte-a-byte com `destructive.cjs:221-226`.
#[derive(Debug, Clone)]
pub struct Hit {
    pub id: String,
    pub label: String,
    pub origin: String,
    pub severity: String,
    pub match_text: String,
}

impl Hit {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "label": self.label,
            "origin": self.origin,
            "severity": self.severity,
            "match": self.match_text,
        })
    }
}

/// Resultado de [`detect`].
#[derive(Debug, Clone)]
pub struct DetectResult {
    pub matched: bool,
    pub hits: Vec<Hit>,
}

impl DetectResult {
    pub fn to_json(&self) -> Value {
        json!({
            "matched": self.matched,
            "hits": self.hits.iter().map(Hit::to_json).collect::<Vec<_>>(),
        })
    }
}

/// Detecta intents destrutivos em `text`.
///
/// Replica `detect(memDir, text)` em `destructive.cjs:212-241`. CORE
/// primeiro (na ordem declarada), depois USER (na ordem do arquivo).
pub fn detect(mem_dir: &Path, text: &str) -> DetectResult {
    if text.is_empty() {
        return DetectResult {
            matched: false,
            hits: Vec::new(),
        };
    }
    let cfg = load_user_config(mem_dir);
    let disabled: std::collections::HashSet<&str> =
        cfg.disabled_core.iter().map(|s| s.as_str()).collect();
    let mut hits = Vec::new();

    for p in CORE_PATTERNS.iter() {
        if disabled.contains(p.id) {
            continue;
        }
        if let Some(m) = p.regex().find(text) {
            hits.push(Hit {
                id: p.id.to_string(),
                label: p.label.to_string(),
                origin: "core".into(),
                severity: p.severity.to_string(),
                match_text: slice_utf16(m.as_str(), 120),
            });
        }
    }

    for p in &cfg.patterns {
        let re = match compile_user_pattern(p) {
            Some(r) => r,
            None => continue,
        };
        if let Some(m) = re.find(text) {
            hits.push(Hit {
                id: p.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
                label: p
                    .get("label")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                origin: "user".into(),
                severity: p
                    .get("severity")
                    .and_then(Value::as_str)
                    .unwrap_or("high")
                    .to_string(),
                match_text: slice_utf16(m.as_str(), 120),
            });
        }
    }

    DetectResult {
        matched: !hits.is_empty(),
        hits,
    }
}

/// Alias de [`detect`]. Replica `testPattern` em `destructive.cjs:243`.
pub fn test_pattern(mem_dir: &Path, text: &str) -> DetectResult {
    detect(mem_dir, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn core_patterns_compile() {
        for p in CORE_PATTERNS.iter() {
            let _ = p.regex();
        }
    }

    #[test]
    fn detect_rm_rf() {
        let tmp = TempDir::new().unwrap();
        let r = detect(tmp.path(), "please run rm -rf /tmp/foo");
        assert!(r.matched);
        assert_eq!(r.hits[0].id, "core.rm_rf");
    }

    #[test]
    fn detect_apagar_tudo_pt() {
        let tmp = TempDir::new().unwrap();
        let r = detect(tmp.path(), "Por favor apague todo o projeto agora");
        assert!(r.matched);
        assert!(r.hits.iter().any(|h| h.id == "core.apagar_tudo"));
    }

    #[test]
    fn detect_no_match() {
        let tmp = TempDir::new().unwrap();
        let r = detect(tmp.path(), "hello world");
        assert!(!r.matched);
    }

    #[test]
    fn save_user_patterns_normalizes() {
        let tmp = TempDir::new().unwrap();
        let id_gen = FixedIdGen("user.deadbeefcafe".into());
        let clock = FixedIsoClock("2026-04-19T12:00:00.000Z".into());
        let input = vec![json!({
            "label": "Block prod wipe",
            "text": "wipe prod",
            "severity": "critical",
        })];
        let out = save_user_patterns(tmp.path(), &input, &id_gen, &clock);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["id"], "user.deadbeefcafe");
        assert_eq!(out[0]["kind"], "literal");
        assert_eq!(out[0]["severity"], "critical");
        assert_eq!(out[0]["enabled"], true);
        assert_eq!(out[0]["created_at"], "2026-04-19T12:00:00.000Z");
    }

    #[test]
    fn save_drops_empty_label_or_text() {
        let tmp = TempDir::new().unwrap();
        let id_gen = FixedIdGen("user.x".into());
        let clock = FixedIsoClock("2026-04-19T12:00:00.000Z".into());
        let input = vec![
            json!({"label":"","text":"foo"}),
            json!({"label":"ok","text":""}),
            json!({"label":"keep","text":"keep"}),
        ];
        let out = save_user_patterns(tmp.path(), &input, &id_gen, &clock);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["label"], "keep");
    }

    #[test]
    fn set_core_disabled_rejects_unknown() {
        let tmp = TempDir::new().unwrap();
        let e = set_core_disabled(tmp.path(), "core.does_not_exist", true).unwrap_err();
        assert!(matches!(e, SetCoreDisabledError::UnknownId(_)));
    }

    #[test]
    fn set_core_disabled_toggles() {
        let tmp = TempDir::new().unwrap();
        let a = set_core_disabled(tmp.path(), "core.rm_rf", true).unwrap();
        assert_eq!(a, vec!["core.rm_rf"]);
        let b = set_core_disabled(tmp.path(), "core.rm_rf", false).unwrap();
        assert!(b.is_empty());
    }

    #[test]
    fn disabled_core_skipped_in_detect() {
        let tmp = TempDir::new().unwrap();
        set_core_disabled(tmp.path(), "core.rm_rf", true).unwrap();
        let r = detect(tmp.path(), "rm -rf /foo");
        assert!(!r.hits.iter().any(|h| h.id == "core.rm_rf"));
    }

    #[test]
    fn user_pattern_literal_with_wildcard() {
        let tmp = TempDir::new().unwrap();
        let id_gen = FixedIdGen("user.x".into());
        let clock = FixedIsoClock("2026-04-19T12:00:00.000Z".into());
        save_user_patterns(
            tmp.path(),
            &[json!({
                "label": "kill-*",
                "text": "kill-*",
                "kind": "literal",
                "severity": "high",
            })],
            &id_gen,
            &clock,
        );
        let r = detect(tmp.path(), "please run kill-everything now");
        assert!(r.matched);
        assert_eq!(r.hits[0].origin, "user");
    }

    #[test]
    fn compile_user_pattern_disabled_returns_none() {
        let pt = json!({"text":"foo","kind":"literal","enabled":false});
        assert!(compile_user_pattern(&pt).is_none());
    }

    #[test]
    fn slice_utf16_handles_multibyte() {
        // "á" is 1 UTF-16 unit, 2 UTF-8 bytes.
        assert_eq!(slice_utf16("áéíóú", 3), "áéí");
        assert_eq!(slice_utf16("ascii", 100), "ascii");
    }
}
