//! # domain-bus
//!
//! Porta Rust de `plugin/scripts/bus.cjs` — **message bus entre
//! agentes**. Camada de garantias sobre handoffs: tracking de estado,
//! retry, timeout, compactação.
//!
//! Tudo em JSONL append-only (`<mem_dir>/messages/YYYY-MM-DD.jsonl`),
//! estado derivado por replay. Zero infraestrutura.
//!
//! ## Escopo (Fase 6)
//!
//! - Paths + [`ensure_msg_dir`] / [`list_day_files`].
//! - [`get_config`] / [`set_config`] — retenção configurável.
//! - [`migrate_legacy`] — splita `messages.jsonl` por dia.
//! - [`read_events`] — leitura filtrada.
//! - [`emit`] — append de evento com timestamp.
//! - [`compact_old_files`] — apaga chunks antigos.
//! - [`derive_state`] / [`trace_summary`] — replay de estado.
//! - [`get_message`] / [`get_pending`] / [`get_state`] — queries.
//! - [`sweep_pending`] — retry + timeout com callbacks.
//!
//! Paridade byte-a-byte validada por `tests/parity_*.rs` contra o
//! `.cjs` original via monkey-patch de `Date` no Node.

use serde_json::{Map, Value};
pub mod trace_index;
pub use trace_index::TraceIndex;

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

// ── Constantes ───────────────────────────────────────────────────────────

/// Nome do sub-diretório de chunks diários. Replica `MSG_DIR = 'messages'`.
pub const MSG_DIR: &str = "messages";
/// Arquivo monolítico antigo — migrado no primeiro read/write.
pub const LEGACY_FILE: &str = "messages.jsonl";
/// Nome do arquivo de config (retention_days).
pub const CONFIG_FILE: &str = "bus-config.json";

/// Dias de retenção default (14).
pub const DEFAULT_RETENTION: u32 = 14;
/// Mínimo aceito em `set_config.retention_days`.
pub const MIN_RETENTION: u32 = 1;
/// Máximo aceito em `set_config.retention_days`.
pub const MAX_RETENTION: u32 = 365;

/// Deadline `created → delivered`. 90s.
pub const DELIVERY_DEADLINE_MS: i64 = 90 * 1000;
/// Deadline `delivered → responded`. 5min.
pub const RESPONSE_DEADLINE_MS: i64 = 5 * 60 * 1000;
/// Número máximo de tentativas antes de `failed`.
pub const MAX_ATTEMPTS: u32 = 3;

// ── Clock trait ──────────────────────────────────────────────────────────

/// Provedor de tempo. `SystemClock` em produção, `FixedClock` em testes.
pub trait Clock {
    /// ISO 8601 UTC com millis (`YYYY-MM-DDTHH:MM:SS.sssZ`).
    fn now_iso(&self) -> String;
    /// Epoch em milissegundos (UTC).
    fn now_ms(&self) -> i64;
}

/// Clock do sistema. Usa [`std::time::SystemTime`] + formatação
/// Gregoriana sem dependências.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_iso(&self) -> String {
        format_iso_millis(self.now_ms())
    }
    fn now_ms(&self) -> i64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_default()
    }
}

/// Clock fixo — útil pra testes de paridade.
#[derive(Debug, Clone)]
pub struct FixedClock(pub i64);

impl Clock for FixedClock {
    fn now_iso(&self) -> String {
        format_iso_millis(self.0)
    }
    fn now_ms(&self) -> i64 {
        self.0
    }
}

/// Converte ms since epoch → ISO 8601 UTC com millis.
///
/// Algoritmo Gregoriano (Howard Hinnant) — sem `chrono`.
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

// ── Paths ────────────────────────────────────────────────────────────────

/// `<mem_dir>/messages/`.
pub fn msg_dir(mem_dir: &Path) -> PathBuf {
    mem_dir.join(MSG_DIR)
}

/// `<mem_dir>/messages.jsonl`.
pub fn legacy_file(mem_dir: &Path) -> PathBuf {
    mem_dir.join(LEGACY_FILE)
}

/// `<mem_dir>/bus-config.json`.
pub fn config_file(mem_dir: &Path) -> PathBuf {
    mem_dir.join(CONFIG_FILE)
}

/// Cria `<mem_dir>/messages/` se ausente (idempotente, ignora erros).
/// Replica `ensureMsgDir` em `bus.cjs:46-50`.
pub fn ensure_msg_dir(mem_dir: &Path) -> PathBuf {
    let d = msg_dir(mem_dir);
    let _ = fs::create_dir_all(&d);
    d
}

/// Extrai `YYYY-MM-DD` de um timestamp ISO, ou retorna o dia atual
/// usando [`Clock`]. Replica `dayKey(tsIso)` em `bus.cjs:52-58`.
pub fn day_key(ts_iso: Option<&str>, clock: &dyn Clock) -> String {
    if let Some(s) = ts_iso {
        if s.len() >= 10 && is_valid_date_prefix(&s[..10]) {
            return s[..10].to_string();
        }
    }
    clock.now_iso()[..10].to_string()
}

fn is_valid_date_prefix(s: &str) -> bool {
    // matches ^\d{4}-\d{2}-\d{2}
    let bytes = s.as_bytes();
    bytes.len() == 10
        && bytes[..4].iter().all(|b| b.is_ascii_digit())
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(|b| b.is_ascii_digit())
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(|b| b.is_ascii_digit())
}

/// `<mem_dir>/messages/YYYY-MM-DD.jsonl`.
pub fn day_file(mem_dir: &Path, day: &str) -> PathBuf {
    msg_dir(mem_dir).join(format!("{day}.jsonl"))
}

/// Lista arquivos `YYYY-MM-DD.jsonl` em ordem alfabética.
///
/// Replica `listDayFiles(memDir)` em `bus.cjs:64-73`. Garante que
/// `msgDir` exista (ignora erros).
pub fn list_day_files(mem_dir: &Path) -> Vec<String> {
    ensure_msg_dir(mem_dir);
    let d = msg_dir(mem_dir);
    let entries = match fs::read_dir(&d) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| is_day_file_name(n))
        .collect();
    names.sort();
    names
}

fn is_day_file_name(n: &str) -> bool {
    // ^\d{4}-\d{2}-\d{2}\.jsonl$
    if n.len() != 16 {
        return false;
    }
    is_valid_date_prefix(&n[..10]) && &n[10..] == ".jsonl"
}

// ── Config ───────────────────────────────────────────────────────────────

/// Lê `bus-config.json`. Retorna defaults + patch merged.
/// Replica `getConfig(memDir)` em `bus.cjs:77-87`.
pub fn get_config(mem_dir: &Path) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert(
        "retention_days".into(),
        Value::from(DEFAULT_RETENTION as i64),
    );
    let raw = match fs::read_to_string(config_file(mem_dir)) {
        Ok(r) => r,
        Err(_) => return out,
    };
    let parsed: Value = match serde_json::from_str(&raw) {
        Ok(p) => p,
        Err(_) => return out,
    };
    if let Some(obj) = parsed.as_object() {
        for (k, v) in obj {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// Erro de [`set_config`].
#[derive(Debug, Error)]
pub enum SetConfigError {
    #[error("retention_days deve ser número entre {0} e {1}")]
    RetentionOutOfRange(u32, u32),
}

/// Atualiza config. Só valida `retention_days` — outros campos passam
/// crus. Replica `setConfig(memDir, patch)` em `bus.cjs:89-103`.
///
/// NOTA: o `.cjs` escreve `JSON.stringify(next, null, 2)` **sem** `\n`
/// no fim. Replicamos.
pub fn set_config(
    mem_dir: &Path,
    patch: &Value,
) -> Result<Map<String, Value>, SetConfigError> {
    let mut next = get_config(mem_dir);
    if let Some(obj) = patch.as_object() {
        if let Some(v) = obj.get("retention_days") {
            let n = match v {
                Value::Number(n) => n.as_f64(),
                Value::String(s) => s.parse::<f64>().ok(),
                Value::Bool(true) => Some(1.0),
                Value::Bool(false) => Some(0.0),
                Value::Null => None,
                _ => None,
            };
            let n = match n {
                Some(x) if x.is_finite() => x,
                _ => {
                    return Err(SetConfigError::RetentionOutOfRange(
                        MIN_RETENTION,
                        MAX_RETENTION,
                    ))
                }
            };
            if n < MIN_RETENTION as f64 || n > MAX_RETENTION as f64 {
                return Err(SetConfigError::RetentionOutOfRange(
                    MIN_RETENTION,
                    MAX_RETENTION,
                ));
            }
            next.insert(
                "retention_days".into(),
                Value::from(n.floor() as i64),
            );
        }
    }
    let s = serde_json::to_string_pretty(&Value::Object(next.clone()))
        .unwrap_or_default();
    let _ = fs::write(config_file(mem_dir), s);
    Ok(next)
}

// ── Migração ─────────────────────────────────────────────────────────────

/// Resultado de [`migrate_legacy`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrateResult {
    pub migrated: usize,
    pub days: usize,
}

/// Splita `messages.jsonl` (legacy) em arquivos diários.
///
/// Replica `migrateLegacy(memDir)` em `bus.cjs:109-136`. Idempotente:
/// depois do split, renomeia o legacy para `messages.jsonl.migrated`.
/// Chamadas subsequentes são no-op (`migrated: 0, days: 0`).
pub fn migrate_legacy(mem_dir: &Path, clock: &dyn Clock) -> MigrateResult {
    let legacy = legacy_file(mem_dir);
    if !legacy.exists() {
        return MigrateResult {
            migrated: 0,
            days: 0,
        };
    }
    ensure_msg_dir(mem_dir);
    let raw = match fs::read_to_string(&legacy) {
        Ok(r) => r,
        Err(_) => {
            return MigrateResult {
                migrated: 0,
                days: 0,
            }
        }
    };
    let lines: Vec<&str> = raw.split('\n').filter(|s| !s.is_empty()).collect();
    let mut order: Vec<String> = Vec::new();
    let mut grouped: Map<String, Value> = Map::new();
    for line in &lines {
        let ev: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ts = ev.get("ts").and_then(Value::as_str);
        if !ev.is_object() || ts.is_none() {
            continue;
        }
        let day = day_key(ts, clock);
        if !grouped.contains_key(&day) {
            order.push(day.clone());
            grouped.insert(day.clone(), Value::Array(Vec::new()));
        }
        if let Some(arr) = grouped.get_mut(&day).and_then(Value::as_array_mut) {
            arr.push(Value::String((*line).to_string()));
        }
    }

    let days_count = order.len();
    for day in &order {
        let f = day_file(mem_dir, day);
        let existing = fs::read_to_string(&f).unwrap_or_default();
        let prefix = if !existing.is_empty() && !existing.ends_with('\n') {
            format!("{existing}\n")
        } else {
            existing
        };
        let arr = grouped
            .get(day)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let day_lines: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        let content = format!("{prefix}{}\n", day_lines.join("\n"));
        let _ = fs::write(&f, content);
    }
    let _ = fs::rename(&legacy, legacy.with_extension("jsonl.migrated"));
    MigrateResult {
        migrated: lines.len(),
        days: days_count,
    }
}

// ── Leitura ──────────────────────────────────────────────────────────────

/// Opções de [`read_events`]. Campos `None` = sem filtro.
#[derive(Debug, Default, Clone)]
pub struct ReadEventsOpts<'a> {
    pub since_days: Option<u32>,
    pub since_date: Option<&'a str>,
}

fn parse_file(path: &Path, out: &mut Vec<Value>) {
    let raw = match fs::read_to_string(path) {
        Ok(r) => r,
        Err(_) => return,
    };
    for line in raw.split('\n') {
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            out.push(v);
        }
    }
}

/// Lê eventos de todos os chunks do `msg_dir` (com filtros opcionais).
///
/// Replica `readEvents(memDir, opts)` em `bus.cjs:158-178`. Dispara
/// migração do legacy na primeira chamada.
pub fn read_events(mem_dir: &Path, opts: &ReadEventsOpts, clock: &dyn Clock) -> Vec<Value> {
    migrate_legacy(mem_dir, clock);
    let days = list_day_files(mem_dir);
    if days.is_empty() {
        return Vec::new();
    }

    let mut cutoff: Option<String> = None;
    if let Some(n) = opts.since_days {
        if n > 0 {
            let now_ms = clock.now_ms();
            // d.setUTCDate(d.getUTCDate() - (sinceDays - 1))
            let shift_ms = (n as i64 - 1) * 86_400_000;
            let ts = now_ms - shift_ms;
            cutoff = Some(format_iso_millis(ts)[..10].to_string());
        }
    }
    if let Some(date) = opts.since_date {
        match &cutoff {
            None => cutoff = Some(date.to_string()),
            Some(c) if date > c.as_str() => cutoff = Some(date.to_string()),
            _ => {}
        }
    }

    let mut out = Vec::new();
    for name in days {
        let day = &name[..name.len() - ".jsonl".len()];
        if let Some(c) = &cutoff {
            if day < c.as_str() {
                continue;
            }
        }
        parse_file(&msg_dir(mem_dir).join(&name), &mut out);
    }
    out
}

// ── Escrita ──────────────────────────────────────────────────────────────

/// Adiciona um evento ao chunk do dia. Retorna o evento com `ts`
/// preenchido (ou `None` se o evento é inválido).
///
/// Replica `emit(memDir, ev)` em `bus.cjs:182-192`. Valida que o evento
/// tem `type` e `trace_id`. Spread do JS: `{ts, ...ev}` — `ev.ts` se
/// definido sobrescreve o gerado.
pub fn emit(mem_dir: &Path, ev: &Value, clock: &dyn Clock) -> Option<Value> {
    if !is_emit_valid(ev) {
        return None;
    }
    migrate_legacy(mem_dir, clock);
    ensure_msg_dir(mem_dir);
    let ts = clock.now_iso();
    let mut entry = Map::new();
    entry.insert("ts".into(), Value::String(ts.clone()));
    if let Some(obj) = ev.as_object() {
        for (k, v) in obj {
            entry.insert(k.clone(), v.clone());
        }
    }
    let effective_ts = entry
        .get("ts")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or(ts);
    let day = day_key(Some(&effective_ts), clock);
    let file = day_file(mem_dir, &day);
    let line = serde_json::to_string(&Value::Object(entry.clone())).unwrap_or_default();
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&file) {
        let _ = writeln!(f, "{line}");
    }
    Some(Value::Object(entry))
}

fn is_emit_valid(ev: &Value) -> bool {
    let obj = match ev.as_object() {
        Some(o) => o,
        None => return false,
    };
    let has_type = obj
        .get("type")
        .map(|v| !matches!(v, Value::Null) && truthy(v))
        .unwrap_or(false);
    let has_trace = obj
        .get("trace_id")
        .map(|v| !matches!(v, Value::Null) && truthy(v))
        .unwrap_or(false);
    has_type && has_trace
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

// ── Compactação ──────────────────────────────────────────────────────────

/// Opções de [`compact_old_files`].
#[derive(Debug, Default, Clone)]
pub struct CompactOpts<'a> {
    pub retention_days: Option<f64>,
    pub now: Option<&'a str>,
}

/// Resultado de [`compact_old_files`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactResult {
    pub deleted: Vec<String>,
    pub kept: Vec<String>,
    pub retention_days: u32,
    pub cutoff: String,
}

/// Apaga chunks mais antigos que `retention_days`.
///
/// Replica `compactOldFiles(memDir, opts)` em `bus.cjs:201-221`.
pub fn compact_old_files(
    mem_dir: &Path,
    opts: &CompactOpts,
    clock: &dyn Clock,
) -> CompactResult {
    let cfg = get_config(mem_dir);
    let days: u32 = if let Some(r) = opts.retention_days {
        if r.is_finite() && r > 0.0 {
            r.floor() as u32
        } else {
            cfg.get("retention_days")
                .and_then(Value::as_i64)
                .map(|i| i as u32)
                .unwrap_or(DEFAULT_RETENTION)
        }
    } else {
        cfg.get("retention_days")
            .and_then(Value::as_i64)
            .map(|i| i as u32)
            .unwrap_or(DEFAULT_RETENTION)
    };
    let ref_ms = if let Some(now) = opts.now {
        parse_iso_millis(now).unwrap_or_else(|| clock.now_ms())
    } else {
        clock.now_ms()
    };
    let shifted = ref_ms - (days as i64 * 86_400_000);
    let cutoff = format_iso_millis(shifted)[..10].to_string();

    let mut deleted = Vec::new();
    let mut kept = Vec::new();
    for name in list_day_files(mem_dir) {
        let day = name[..name.len() - ".jsonl".len()].to_string();
        let full = msg_dir(mem_dir).join(&name);
        if day < cutoff {
            if fs::remove_file(&full).is_ok() {
                deleted.push(day);
            }
        } else {
            kept.push(day);
        }
    }
    CompactResult {
        deleted,
        kept,
        retention_days: days,
        cutoff,
    }
}

fn parse_iso_millis(s: &str) -> Option<i64> {
    // Aceita: YYYY-MM-DD, YYYY-MM-DDTHH:MM:SS.sssZ, YYYY-MM-DDTHH:MM:SSZ, números
    if let Ok(n) = s.parse::<i64>() {
        return Some(n);
    }
    let bytes = s.as_bytes();
    if bytes.len() < 10 || !is_valid_date_prefix(&s[..10]) {
        return None;
    }
    let year: i64 = s[..4].parse().ok()?;
    let month: i64 = s[5..7].parse().ok()?;
    let day: i64 = s[8..10].parse().ok()?;

    let (hour, min, sec, ms) = if bytes.len() >= 11 {
        let h: i64 = s[11..13].parse().ok()?;
        let mi: i64 = s[14..16].parse().ok()?;
        let sec: i64 = s[17..19].parse().ok()?;
        let ms: i64 = if bytes.len() >= 23 && &bytes[19..20] == b"." {
            s[20..23].parse().unwrap_or(0)
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
    // Howard Hinnant (inversa de days → civil).
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 }.div_euclid(400);
    let yoe = (y - era * 400) as u64;
    let mp: u32 = if m > 2 { m - 3 } else { m + 9 };
    let doy: u64 = ((153u64 * mp as u64 + 2) / 5) + d as u64 - 1;
    let doe: u64 = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

// ── State derivation ─────────────────────────────────────────────────────

fn stage_order(t: &str) -> i64 {
    match t {
        "created" | "retry" => 1,
        "delivered" => 2,
        "responded" => 3,
        "acked" | "reported" => 4,
        "timed_out" | "failed" => 5,
        _ => 0,
    }
}

/// Reduz eventos a último estado por trace_id.
///
/// Replica `deriveState(events)` em `bus.cjs:237-246`.
pub fn derive_state(events: &[Value]) -> BTreeMap<String, Value> {
    // JS Map preserva insertion order; BTreeMap é lexicográfico. O
    // consumo downstream (`getPending`/`getState`) itera em ORDEM DE
    // INSERÇÃO. Usamos `IndexMap` via serde_json se quisermos preservar.
    // serde_json não expõe IndexMap público. Usamos Vec + HashMap.
    // Mas pra simplicidade, usamos um wrapper manual preservando ordem.
    let mut order: Vec<String> = Vec::new();
    let mut by_trace: BTreeMap<String, Value> = BTreeMap::new();
    let _ = &mut order;
    for ev in events {
        let tid = match ev.get("trace_id").and_then(Value::as_str) {
            Some(t) => t.to_string(),
            None => continue,
        };
        let t = ev.get("type").and_then(Value::as_str).unwrap_or_default();
        let new_order = stage_order(t);
        match by_trace.get(&tid) {
            None => {
                order.push(tid.clone());
                by_trace.insert(tid, ev.clone());
            }
            Some(cur) => {
                let cur_t = cur.get("type").and_then(Value::as_str).unwrap_or_default();
                let cur_order = stage_order(cur_t);
                if new_order >= cur_order {
                    by_trace.insert(tid, ev.clone());
                }
            }
        }
    }
    by_trace
}

/// Ordered version — preserva ordem de primeira inserção.
pub fn derive_state_ordered(events: &[Value]) -> Vec<(String, Value)> {
    let mut order: Vec<String> = Vec::new();
    let mut by_trace: std::collections::HashMap<String, Value> =
        std::collections::HashMap::new();
    for ev in events {
        let tid = match ev.get("trace_id").and_then(Value::as_str) {
            Some(t) => t.to_string(),
            None => continue,
        };
        let t = ev.get("type").and_then(Value::as_str).unwrap_or_default();
        let new_order = stage_order(t);
        match by_trace.get(&tid) {
            None => {
                order.push(tid.clone());
                by_trace.insert(tid, ev.clone());
            }
            Some(cur) => {
                let cur_t = cur.get("type").and_then(Value::as_str).unwrap_or_default();
                let cur_order = stage_order(cur_t);
                if new_order >= cur_order {
                    by_trace.insert(tid, ev.clone());
                }
            }
        }
    }
    order
        .into_iter()
        .map(|k| {
            let v = by_trace.remove(&k).unwrap();
            (k, v)
        })
        .collect()
}

/// Resultado de [`trace_summary`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceSummary {
    pub created: Option<Value>,
    pub latest: Value,
    pub attempts: usize,
    pub events: Vec<Value>,
}

/// Extrai resumo de um trace específico.
///
/// Replica `traceSummary(events, trace_id)` em `bus.cjs:250-257`.
pub fn trace_summary(events: &[Value], trace_id: &str) -> Option<TraceSummary> {
    let related: Vec<Value> = events
        .iter()
        .filter(|e| e.get("trace_id").and_then(Value::as_str) == Some(trace_id))
        .cloned()
        .collect();
    if related.is_empty() {
        return None;
    }
    let created = related
        .iter()
        .find(|e| e.get("type").and_then(Value::as_str) == Some("created"))
        .cloned();
    let latest = related.last().cloned().unwrap();
    let attempts = related
        .iter()
        .filter(|e| {
            matches!(
                e.get("type").and_then(Value::as_str),
                Some("delivered") | Some("retry")
            )
        })
        .count();
    Some(TraceSummary {
        created,
        latest,
        attempts,
        events: related,
    })
}

// ── Queries ──────────────────────────────────────────────────────────────

/// Lê janela completa (retention_days) e retorna trace summary.
///
/// Replica `getMessage(memDir, trace_id)` em `bus.cjs:261-266`.
pub fn get_message(mem_dir: &Path, trace_id: &str, clock: &dyn Clock) -> Option<TraceSummary> {
    let cfg = get_config(mem_dir);
    let retention = cfg
        .get("retention_days")
        .and_then(Value::as_i64)
        .map(|i| i as u32)
        .unwrap_or(DEFAULT_RETENTION);
    let events = read_events(
        mem_dir,
        &ReadEventsOpts {
            since_days: Some(retention),
            since_date: None,
        },
        clock,
    );
    trace_summary(&events, trace_id)
}

/// Retorna traces não-terminais.
///
/// Replica `getPending(memDir)` em `bus.cjs:269-284`.
pub fn get_pending(mem_dir: &Path, clock: &dyn Clock) -> Vec<Value> {
    let events = read_events(
        mem_dir,
        &ReadEventsOpts {
            since_days: Some(2),
            since_date: None,
        },
        clock,
    );
    let states = derive_state_ordered(&events);
    let mut pending = Vec::new();
    for (trace_id, last) in &states {
        let t = last.get("type").and_then(Value::as_str).unwrap_or_default();
        if matches!(
            t,
            "responded" | "acked" | "reported" | "failed" | "timed_out"
        ) {
            continue;
        }
        let summary = match trace_summary(&events, trace_id) {
            Some(s) => s,
            None => continue,
        };
        // `{ trace_id, state: last.type, ...summary }` — summary é
        // spread após `state`.
        let mut m = Map::new();
        m.insert("trace_id".into(), Value::String(trace_id.clone()));
        m.insert("state".into(), Value::String(t.to_string()));
        // Spread de `summary` — JS spread de um objeto com keys `created,
        // latest, attempts, events`. Se `state` já foi definido e summary
        // não tem `state`, ordem: trace_id, state, created, latest,
        // attempts, events.
        m.insert(
            "created".into(),
            summary.created.clone().unwrap_or(Value::Null),
        );
        m.insert("latest".into(), summary.latest.clone());
        m.insert("attempts".into(), Value::from(summary.attempts));
        m.insert("events".into(), Value::Array(summary.events.clone()));
        pending.push(Value::Object(m));
    }
    pending
}

/// Opções de [`get_state`].
#[derive(Debug, Clone)]
pub struct GetStateOpts {
    /// Quantos eventos no fim do log devolver. Default: 50.
    pub recent: usize,
    /// Janela de leitura (dias). None = cfg.retention_days.
    pub since_days: Option<u32>,
}

impl Default for GetStateOpts {
    fn default() -> Self {
        Self {
            recent: 50,
            since_days: None,
        }
    }
}

/// Panorama agregado.
///
/// Replica `getState(memDir, opts)` em `bus.cjs:287-304`.
pub fn get_state(mem_dir: &Path, opts: &GetStateOpts, clock: &dyn Clock) -> Value {
    let cfg = get_config(mem_dir);
    let retention = cfg
        .get("retention_days")
        .and_then(Value::as_i64)
        .map(|i| i as u32)
        .unwrap_or(DEFAULT_RETENTION);
    let win = opts.since_days.unwrap_or(retention);
    let events = read_events(
        mem_dir,
        &ReadEventsOpts {
            since_days: Some(win),
            since_date: None,
        },
        clock,
    );
    let states = derive_state_ordered(&events);

    // Buckets na mesma ordem do .cjs.
    let bucket_keys = [
        "created",
        "delivered",
        "responded",
        "acked",
        "reported",
        "retry",
        "timed_out",
        "failed",
    ];
    let mut buckets: Map<String, Value> = Map::new();
    for k in &bucket_keys {
        buckets.insert((*k).to_string(), Value::Array(Vec::new()));
    }
    for (trace_id, last) in &states {
        let t = last.get("type").and_then(Value::as_str).unwrap_or_default();
        if !buckets.contains_key(t) {
            continue;
        }
        let mut m = Map::new();
        m.insert("trace_id".into(), Value::String(trace_id.clone()));
        if let Some(obj) = last.as_object() {
            for (k, v) in obj {
                m.insert(k.clone(), v.clone());
            }
        }
        if let Some(arr) = buckets.get_mut(t).and_then(Value::as_array_mut) {
            arr.push(Value::Object(m));
        }
    }

    let mut totals = Map::new();
    for k in &bucket_keys {
        let len = buckets
            .get(*k)
            .and_then(Value::as_array)
            .map(|a| a.len())
            .unwrap_or(0);
        totals.insert((*k).to_string(), Value::from(len));
    }

    let mut pending = Vec::new();
    for k in ["created", "delivered", "retry"] {
        if let Some(arr) = buckets.get(k).and_then(Value::as_array) {
            pending.extend_from_slice(arr);
        }
    }

    let recent_events: Vec<Value> = if events.len() > opts.recent {
        events[events.len() - opts.recent..].to_vec()
    } else {
        events.clone()
    };

    let day_files = list_day_files(mem_dir);

    let mut out = Map::new();
    out.insert("totals".into(), Value::Object(totals));
    out.insert("pending".into(), Value::Array(pending));
    out.insert("recent_events".into(), Value::Array(recent_events));
    out.insert(
        "retention_days".into(),
        Value::from(retention as i64),
    );
    out.insert(
        "day_files".into(),
        Value::Array(day_files.into_iter().map(Value::String).collect()),
    );
    Value::Object(out)
}

// ── Sweep ────────────────────────────────────────────────────────────────

/// Outcome de um trace no sweep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SweepOutcome {
    Retry { attempt: u32 },
    Failed,
}

/// Resultado de [`sweep_pending`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepResult {
    pub retried: Vec<(String, SweepOutcome)>,
    pub timed_out: Vec<String>,
}

/// Callback opaco que o caller opcionalmente fornece para reagir a
/// retry/timeout detectados pelo sweep.
pub type SweepCallback<'a> = Box<dyn FnMut(&Value) + 'a>;

/// Callbacks disparados durante o sweep. Podem ser no-ops.
#[derive(Default)]
pub struct SweepCallbacks<'a> {
    pub on_retry: Option<SweepCallback<'a>>,
    pub on_timeout: Option<SweepCallback<'a>>,
}

/// Opções de [`sweep_pending`].
#[derive(Debug, Default, Clone)]
pub struct SweepOpts<'a> {
    /// Timestamp para "agora". None = [`Clock::now_ms`].
    pub now: Option<&'a str>,
}

/// Faz retry/timeout com base nos deadlines. Replica `sweepPending`
/// em `bus.cjs:321-367`.
pub fn sweep_pending(
    mem_dir: &Path,
    opts: &SweepOpts,
    callbacks: &mut SweepCallbacks<'_>,
    clock: &dyn Clock,
) -> SweepResult {
    let events = read_events(
        mem_dir,
        &ReadEventsOpts {
            since_days: Some(2),
            since_date: None,
        },
        clock,
    );
    let states = derive_state_ordered(&events);
    let now_ms = if let Some(n) = opts.now {
        parse_iso_millis(n).unwrap_or_else(|| clock.now_ms())
    } else {
        clock.now_ms()
    };

    let mut retried = Vec::new();
    let mut timed_out = Vec::new();

    for (trace_id, last) in &states {
        let summary = match trace_summary(&events, trace_id) {
            Some(s) if s.created.is_some() => s,
            _ => continue,
        };
        let last_ts = last.get("ts").and_then(Value::as_str).unwrap_or("");
        let last_ms = parse_iso_millis(last_ts).unwrap_or(0);
        let age = now_ms - last_ms;
        let t = last.get("type").and_then(Value::as_str).unwrap_or_default();

        if (t == "created" || t == "retry") && age > DELIVERY_DEADLINE_MS {
            if summary.attempts as u32 >= MAX_ATTEMPTS {
                let mut ev = Map::new();
                ev.insert("type".into(), Value::String("failed".into()));
                ev.insert("trace_id".into(), Value::String(trace_id.clone()));
                ev.insert(
                    "reason".into(),
                    Value::String("max_delivery_attempts".into()),
                );
                ev.insert("attempts".into(), Value::from(summary.attempts));
                emit(mem_dir, &Value::Object(ev), clock);
                retried.push((trace_id.clone(), SweepOutcome::Failed));
                continue;
            }
            let mut ev = Map::new();
            ev.insert("type".into(), Value::String("retry".into()));
            ev.insert("trace_id".into(), Value::String(trace_id.clone()));
            ev.insert(
                "attempt".into(),
                Value::from((summary.attempts + 1) as i64),
            );
            ev.insert(
                "reason".into(),
                Value::String("delivery_deadline".into()),
            );
            emit(mem_dir, &Value::Object(ev), clock);
            if let Some(cb) = callbacks.on_retry.as_mut() {
                if let Some(c) = &summary.created {
                    cb(c);
                }
            }
            retried.push((
                trace_id.clone(),
                SweepOutcome::Retry {
                    attempt: (summary.attempts + 1) as u32,
                },
            ));
        } else if t == "delivered" && age > RESPONSE_DEADLINE_MS {
            let mut ev = Map::new();
            ev.insert("type".into(), Value::String("timed_out".into()));
            ev.insert("trace_id".into(), Value::String(trace_id.clone()));
            ev.insert("stage".into(), Value::String("response".into()));
            ev.insert("waited_ms".into(), Value::from(age));
            emit(mem_dir, &Value::Object(ev), clock);
            if let Some(cb) = callbacks.on_timeout.as_mut() {
                if let Some(c) = &summary.created {
                    cb(c);
                }
            }
            timed_out.push(trace_id.clone());
        }
    }

    SweepResult { retried, timed_out }
}

#[allow(dead_code)]
fn _unused_io_marker(_e: io::Error) {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct Clk(i64);
    impl Clock for Clk {
        fn now_iso(&self) -> String {
            format_iso_millis(self.0)
        }
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    #[test]
    fn iso_round_trip() {
        assert_eq!(format_iso_millis(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            format_iso_millis(1_776_600_000_000),
            "2026-04-19T12:00:00.000Z"
        );
        assert_eq!(
            parse_iso_millis("2026-04-19T12:00:00.000Z"),
            Some(1_776_600_000_000)
        );
        assert_eq!(
            parse_iso_millis("2026-04-19"),
            Some(1_776_556_800_000)
        );
    }

    #[test]
    fn day_key_extracts_or_defaults() {
        let c = Clk(1_776_600_000_000);
        assert_eq!(day_key(Some("2026-04-19T12:00:00Z"), &c), "2026-04-19");
        assert_eq!(day_key(Some("invalid"), &c), "2026-04-19");
        assert_eq!(day_key(None, &c), "2026-04-19");
    }

    #[test]
    fn emit_and_read_round_trip() {
        let tmp = TempDir::new().unwrap();
        let c = Clk(1_776_600_000_000);
        let ev = serde_json::json!({"type":"created","trace_id":"t1","foo":"bar"});
        let entry = emit(tmp.path(), &ev, &c).unwrap();
        assert_eq!(entry["ts"], "2026-04-19T12:00:00.000Z");
        assert_eq!(entry["type"], "created");
        let events = read_events(tmp.path(), &ReadEventsOpts::default(), &c);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["trace_id"], "t1");
    }

    #[test]
    fn emit_rejects_invalid() {
        let tmp = TempDir::new().unwrap();
        let c = Clk(0);
        assert!(emit(tmp.path(), &serde_json::json!({}), &c).is_none());
        assert!(emit(tmp.path(), &serde_json::json!({"type":"x"}), &c).is_none());
        assert!(emit(tmp.path(), &serde_json::json!({"trace_id":"t1"}), &c).is_none());
    }

    #[test]
    fn config_defaults_and_set() {
        let tmp = TempDir::new().unwrap();
        let c = get_config(tmp.path());
        assert_eq!(c["retention_days"], 14);
        let n = set_config(tmp.path(), &serde_json::json!({"retention_days": 30})).unwrap();
        assert_eq!(n["retention_days"], 30);
        let c2 = get_config(tmp.path());
        assert_eq!(c2["retention_days"], 30);
    }

    #[test]
    fn config_invalid_range() {
        let tmp = TempDir::new().unwrap();
        let e = set_config(tmp.path(), &serde_json::json!({"retention_days": 0})).unwrap_err();
        assert!(matches!(e, SetConfigError::RetentionOutOfRange(..)));
        let e = set_config(tmp.path(), &serde_json::json!({"retention_days": 400})).unwrap_err();
        assert!(matches!(e, SetConfigError::RetentionOutOfRange(..)));
    }

    #[test]
    fn derive_state_promotes() {
        let events = vec![
            serde_json::json!({"trace_id":"t1","type":"created","ts":"2026-04-19T00:00:00.000Z"}),
            serde_json::json!({"trace_id":"t1","type":"delivered","ts":"2026-04-19T00:00:01.000Z"}),
            serde_json::json!({"trace_id":"t1","type":"responded","ts":"2026-04-19T00:00:02.000Z"}),
        ];
        let states = derive_state(&events);
        assert_eq!(states["t1"]["type"], "responded");
    }

    #[test]
    fn compact_deletes_old() {
        let tmp = TempDir::new().unwrap();
        let c = Clk(1_776_600_000_000); // 2026-04-19
        // Fabricar chunks.
        ensure_msg_dir(tmp.path());
        for day in ["2026-03-01", "2026-04-10", "2026-04-19"] {
            let f = day_file(tmp.path(), day);
            fs::write(
                &f,
                format!(
                    "{{\"ts\":\"{day}T00:00:00.000Z\",\"type\":\"created\",\"trace_id\":\"t\"}}\n"
                ),
            )
            .unwrap();
        }
        let r = compact_old_files(
            tmp.path(),
            &CompactOpts {
                retention_days: Some(14.0),
                now: Some("2026-04-19T00:00:00.000Z"),
            },
            &c,
        );
        // cutoff = 2026-04-05. Tudo antes disso deleta.
        assert!(r.deleted.contains(&"2026-03-01".to_string()));
        assert!(!r.deleted.contains(&"2026-04-10".to_string()));
    }

    #[test]
    fn legacy_migrated_to_day_files() {
        let tmp = TempDir::new().unwrap();
        let c = Clk(0);
        let legacy = legacy_file(tmp.path());
        fs::write(
            &legacy,
            "{\"ts\":\"2026-04-19T00:00:00Z\",\"type\":\"created\",\"trace_id\":\"t1\"}\n{\"ts\":\"2026-04-20T00:00:00Z\",\"type\":\"created\",\"trace_id\":\"t2\"}\n",
        ).unwrap();
        let r = migrate_legacy(tmp.path(), &c);
        assert_eq!(r.migrated, 2);
        assert_eq!(r.days, 2);
        assert!(tmp.path().join("messages/2026-04-19.jsonl").exists());
        assert!(tmp.path().join("messages/2026-04-20.jsonl").exists());
        assert!(tmp.path().join("messages.jsonl.migrated").exists());
        assert!(!legacy.exists());
    }
}
