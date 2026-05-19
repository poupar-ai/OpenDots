//! # domain-firefighter
//!
//! Porta Rust de `plugin/scripts/firefighter.cjs` — **poller de
//! bombeiros** que consulta fontes (Datadog), avalia threshold e
//! dispara triage via `claude -p`.
//!
//! ## Escopo (Fase 8)
//!
//! Partes puras (testáveis byte-a-byte contra o `.cjs`):
//! - [`state_file`] / [`load_state`] / [`save_state`].
//! - [`prune_old_triages`].
//! - [`parse_args`] (CLI).
//! - [`build_triage_prompt`] (template do prompt).
//! - [`cooldown_state`] — lógica de cooldown/rate-limit extraída.
//!
//! Partes I/O (portadas com traits pra permitir injeção em testes;
//! não testadas por paridade porque o `.cjs` também depende de HTTPS +
//! spawn de `claude`):
//! - [`LogFetcher`] — abstrai `fetchDatadogLogs`.
//! - [`ClaudeRunner`] — abstrai `runClaude`.
//! - [`check_agent`] — pipeline que usa os dois traits.
//!
//! Cobertura do `.cjs`:
//! - `loadState`/`saveState`/`pruneOldTriages`: paridade byte-a-byte.
//! - `parseArgs`: paridade byte-a-byte.
//! - `buildTriagePrompt`: paridade byte-a-byte.
//! - `checkAgent`: paridade **semântica** (mesmos ramos de decisão
//!   para os mesmos inputs), sem cross-run HTTPS real.
//! - `fetchDatadogLogs`/`runClaude`: port struct-only, teste
//!   manual em integração (fora do escopo de paridade).

use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Nome do arquivo de estado. Replica `STATE_FILE` em
/// `firefighter.cjs:23`.
pub const STATE_FILE: &str = "firefighter_state.json";

// ── Clock ────────────────────────────────────────────────────────────────

/// Provedor de tempo — necessário pra cooldown e prune determinísticos
/// em testes.
pub trait Clock {
    /// Epoch em ms (UTC). Replica `Date.now()`.
    fn now_ms(&self) -> i64;
    /// `new Date().toISOString()`.
    fn now_iso(&self) -> String;
}

/// Clock padrão.
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
    fn now_iso(&self) -> String {
        format_iso_millis(self.now_ms())
    }
}

/// Clock fixo pra testes.
#[derive(Debug, Clone)]
pub struct FixedClock(pub i64);

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.0
    }
    fn now_iso(&self) -> String {
        format_iso_millis(self.0)
    }
}

/// ISO 8601 UTC com millis. Algoritmo Howard Hinnant sem chrono.
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
    let year: i64 = s[..4].parse().ok()?;
    let month: i64 = s[5..7].parse().ok()?;
    let day: i64 = s[8..10].parse().ok()?;
    let (hour, min, sec, ms) = if bytes.len() >= 20 {
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
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 }.div_euclid(400);
    let yoe = (y - era * 400) as u64;
    let mp: u32 = if m > 2 { m - 3 } else { m + 9 };
    let doy: u64 = ((153u64 * mp as u64 + 2) / 5) + d as u64 - 1;
    let doe: u64 = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

// ── State file ───────────────────────────────────────────────────────────

/// `<mem_dir>/firefighter_state.json`.
pub fn state_file(mem_dir: &Path) -> PathBuf {
    mem_dir.join(STATE_FILE)
}

/// Lê `firefighter_state.json`. Tolerante a parse errors:
/// `{}` em qualquer falha. Replica `loadState` em
/// `firefighter.cjs:29-33`.
pub fn load_state(mem_dir: &Path) -> Value {
    let raw = match fs::read_to_string(state_file(mem_dir)) {
        Ok(r) => r,
        Err(_) => return Value::Object(Map::new()),
    };
    serde_json::from_str(&raw).unwrap_or(Value::Object(Map::new()))
}

/// Escreve `firefighter_state.json` com `\n` final.
/// Replica `saveState` em `firefighter.cjs:35-37`.
pub fn save_state(mem_dir: &Path, state: &Value) -> std::io::Result<()> {
    let mut s = serde_json::to_string_pretty(state).unwrap_or_default();
    s.push('\n');
    fs::write(state_file(mem_dir), s)
}

/// Mantém apenas entries cujo `ts` é mais recente que `cutoff = now
/// - 3_600_000ms`. Replica `pruneOldTriages` em
/// `firefighter.cjs:39-42`.
pub fn prune_old_triages(entries: &[Value], clock: &dyn Clock) -> Vec<Value> {
    let cutoff = clock.now_ms() - 3_600_000;
    entries
        .iter()
        .filter(|e| {
            let ts = e.get("ts").and_then(Value::as_str).unwrap_or("");
            parse_iso_millis(ts).map(|ms| ms > cutoff).unwrap_or(false)
        })
        .cloned()
        .collect()
}

// ── parseArgs ────────────────────────────────────────────────────────────

/// Args parseados de `watch` CLI. Replica `parseArgs` em
/// `firefighter.cjs:270-280`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchArgs {
    pub interval: i64,
    pub once: bool,
    pub dry_run: bool,
    pub agent_name: Option<String>,
}

impl Default for WatchArgs {
    fn default() -> Self {
        Self {
            interval: 60,
            once: false,
            dry_run: false,
            agent_name: None,
        }
    }
}

/// Parse args. Flags desconhecidas são ignoradas (paridade com o loop
/// simples do `.cjs`). `--interval` sem valor numérico → 60 (default
/// replicado via `parseInt(x, 10) || 60`).
pub fn parse_args(argv: &[&str]) -> WatchArgs {
    let mut out = WatchArgs::default();
    let mut i = 0;
    while i < argv.len() {
        match argv[i] {
            "--once" => out.once = true,
            "--dry-run" => out.dry_run = true,
            "--interval" => {
                i += 1;
                if let Some(v) = argv.get(i) {
                    out.interval = parse_int_loose(v).unwrap_or(60);
                } else {
                    out.interval = 60;
                }
            }
            "--agent" => {
                i += 1;
                if let Some(v) = argv.get(i) {
                    out.agent_name = Some((*v).to_string());
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// Replica `parseInt(s, 10) || 60` — lê dígitos consecutivos do início;
/// `NaN` / `0` viram `None` (JS `|| 60` cai no default).
fn parse_int_loose(s: &str) -> Option<i64> {
    let s = s.trim_start();
    let (sign, rest) = match s.as_bytes().first() {
        Some(b'-') => (-1i64, &s[1..]),
        Some(b'+') => (1, &s[1..]),
        _ => (1, s),
    };
    let mut end = 0;
    for (i, c) in rest.char_indices() {
        if c.is_ascii_digit() {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        return None;
    }
    let n: i64 = rest[..end].parse().ok()?;
    let n = n * sign;
    if n == 0 {
        return None;
    }
    Some(n)
}

// ── buildTriagePrompt ────────────────────────────────────────────────────

/// Config `watch` usada no prompt.
#[derive(Debug, Clone)]
pub struct WatchCfg<'a> {
    pub query: &'a str,
    pub threshold: i64,
    pub window_minutes: i64,
    pub cooldown_seconds: i64,
    pub max_triages_per_hour: i64,
}

/// Monta o prompt de triage. Replica `buildTriagePrompt` em
/// `firefighter.cjs:107-135`.
///
/// `logs` entra como JSON (cada item é um evento do DD `data[]`).
/// Apenas os 10 primeiros logs aparecem no resumo.
pub fn build_triage_prompt(
    agent_name: &str,
    watch: &WatchCfg,
    mini_context: &str,
    logs: &[Value],
) -> String {
    let summary = logs
        .iter()
        .take(10)
        .enumerate()
        .map(|(i, l)| format_log_line(i + 1, l))
        .collect::<Vec<_>>()
        .join("\n");

    let summary = if summary.is_empty() {
        "_(sem detalhes)_".to_string()
    } else {
        summary
    };

    format!(
        "Você é o bombeiro **{agent_name}**. Incêndio detectado:\n\
         - Query: `{query}`\n\
         - Eventos encontrados: {events} (threshold era {threshold})\n\
         - Janela: últimos {window} minutos\n\
         \n\
         ## Seu runbook (mini-contexto)\n\
         {mini_context}\n\
         \n\
         ## Amostra de logs (até 10)\n\
         {summary}\n\
         \n\
         ## Sua tarefa\n\
         1. Classifique a severidade (baixa / média / alta / crítica).\n\
         2. Hipótese de causa raiz (1-3 frases).\n\
         3. Próximo passo sugerido (investigar X, rodar Y, consultar agente Z).\n\
         4. Se achar que precisa de ação (motor), diga qual — mas **nunca execute**. Apenas escale ao Manager.\n\
         \n\
         Seja direto. Português. Máximo 12 linhas.",
        query = watch.query,
        events = logs.len(),
        threshold = watch.threshold,
        window = watch.window_minutes,
    )
}

fn format_log_line(idx: usize, l: &Value) -> String {
    let attrs = l
        .get("attributes")
        .and_then(|a| a.get("attributes"))
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));

    let msg = l
        .get("attributes")
        .and_then(|a| a.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            // JSON.stringify(attrs).slice(0, 200)
            let s = serde_json::to_string(&attrs).unwrap_or_default();
            slice_utf16(&s, 200)
        });

    let svc = attrs.get("service").and_then(Value::as_str).unwrap_or("?");
    let host = attrs.get("host").and_then(Value::as_str).unwrap_or("?");
    let ts = l
        .get("attributes")
        .and_then(|a| a.get("timestamp"))
        .and_then(Value::as_str)
        .unwrap_or("?");

    format!(
        "[{idx}] {ts} service={svc} host={host}\n    {msg}",
        msg = slice_utf16(&msg, 400),
    )
}

fn slice_utf16(s: &str, n: usize) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    if units.len() <= n {
        return s.to_string();
    }
    String::from_utf16_lossy(&units[..n])
}

// ── Cooldown / rate limit ────────────────────────────────────────────────

/// Estado interno por agente (dentro do state global).
/// Campos do `.cjs`: `last_triage_at`, `triages_last_hour`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentState {
    pub last_triage_at: Option<String>,
    pub triages_last_hour: Vec<Value>,
}

impl AgentState {
    /// Deriva `AgentState` de `state[name]` — com defaults.
    pub fn from_value(v: Option<&Value>) -> Self {
        let obj = match v.and_then(Value::as_object) {
            Some(o) => o,
            None => return Self::default(),
        };
        let last_triage_at = obj
            .get("last_triage_at")
            .and_then(Value::as_str)
            .map(str::to_string);
        let triages_last_hour = obj
            .get("triages_last_hour")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Self {
            last_triage_at,
            triages_last_hour,
        }
    }

    /// Serializa de volta pra `Value` preservando ordem
    /// `last_triage_at, triages_last_hour`.
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert(
            "last_triage_at".into(),
            self.last_triage_at
                .as_ref()
                .map(|s| Value::String(s.clone()))
                .unwrap_or(Value::Null),
        );
        m.insert(
            "triages_last_hour".into(),
            Value::Array(self.triages_last_hour.clone()),
        );
        Value::Object(m)
    }
}

/// Decisão antes de disparar o fetch. Replica os ramos de
/// `checkAgent` em `firefighter.cjs:159-177`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CooldownDecision {
    /// Fetch pode prosseguir. Retorna o state atualizado (prune feito).
    Proceed,
    /// Ainda em cooldown — N segundos faltando.
    Cooldown { seconds_left: i64 },
    /// Passou o teto `max_triages_per_hour`. Inclui contagem atual.
    RateLimited { current: usize, max: i64 },
}

/// Aplica prune, avalia cooldown e rate-limit.
/// **Modifica** `state.triages_last_hour` in-place (prune).
pub fn cooldown_state(
    state: &mut AgentState,
    watch: &WatchCfg,
    clock: &dyn Clock,
) -> CooldownDecision {
    let entries = state.triages_last_hour.clone();
    state.triages_last_hour =
        prune_old_triages(&entries, clock);

    if let Some(last) = &state.last_triage_at {
        let last_ms = parse_iso_millis(last).unwrap_or(0);
        let since_ms = clock.now_ms() - last_ms;
        let cooldown_ms = watch.cooldown_seconds * 1000;
        if since_ms < cooldown_ms {
            // Math.ceil((cooldown_ms - since_ms) / 1000)
            let delta_ms = cooldown_ms - since_ms;
            let seconds_left = (delta_ms + 999) / 1000;
            return CooldownDecision::Cooldown { seconds_left };
        }
    }

    let count = state.triages_last_hour.len();
    if count as i64 >= watch.max_triages_per_hour {
        return CooldownDecision::RateLimited {
            current: count,
            max: watch.max_triages_per_hour,
        };
    }

    CooldownDecision::Proceed
}

// ── Fetch + Claude runner (traits) ───────────────────────────────────────

/// Abstrai `fetchDatadogLogs`. Em produção: implementação que faz
/// HTTPS. Em testes: mock.
pub trait LogFetcher {
    type Error: std::fmt::Display;
    fn fetch(&self, watch: &WatchCfg) -> Result<Vec<Value>, Self::Error>;
}

/// Resultado de `runClaude` em `.cjs`. `code: -1` sinaliza erro de
/// spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Abstrai `runClaude`. Em testes: stub.
pub trait ClaudeRunner {
    fn run(&self, prompt: &str, cwd: &Path, model: Option<&str>) -> ClaudeResult;
}

// ── checkAgent (orquestração) ────────────────────────────────────────────

/// Outcome de [`check_agent`]. Paridade estrutural com o valor de
/// retorno do `.cjs` (que é um objeto com 1 dos campos por branch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    Skipped(String),
    Cooldown(String),
    RateLimited(String),
    Error(String),
    Ok(String),
    WouldTriage {
        events: usize,
        prompt_chars: usize,
    },
    Triaged {
        events: usize,
        exit_code: i32,
    },
}

impl CheckOutcome {
    /// Converte pra `Value` com a mesma key do `.cjs`. Útil pra
    /// comparação cross-lang.
    pub fn to_value(&self) -> Value {
        match self {
            Self::Skipped(s) => json!({ "skipped": s }),
            Self::Cooldown(s) => json!({ "cooldown": s }),
            Self::RateLimited(s) => json!({ "rate_limited": s }),
            Self::Error(s) => json!({ "error": s }),
            Self::Ok(s) => json!({ "ok": s }),
            Self::WouldTriage {
                events,
                prompt_chars,
            } => json!({
                "would_triage": true,
                "events": events,
                "prompt_chars": prompt_chars,
            }),
            Self::Triaged { events, exit_code } => json!({
                "triaged": true,
                "events": events,
                "exit_code": exit_code,
            }),
        }
    }
}

/// Input de [`check_agent`].
pub struct CheckInput<'a> {
    pub mem_dir: &'a Path,
    pub project_root: &'a Path,
    pub name: &'a str,
    pub watch: &'a WatchCfg<'a>,
    pub mini_context: &'a str,
    pub model: Option<&'a str>,
    pub context_file: Option<&'a str>,
    pub dry_run: bool,
}

/// Erro genérico emitido pelo pipeline (só aparece se save_state
/// falhar).
#[derive(Debug, Error)]
pub enum CheckAgentError {
    #[error("save_state failed: {0}")]
    SaveState(#[from] std::io::Error),
}

/// Pipeline completo: prune → cooldown → rate → fetch → threshold →
/// triage. Retorna [`CheckOutcome`] + estado atualizado.
///
/// Replica `checkAgent` em `firefighter.cjs:159-229`.
#[allow(clippy::too_many_arguments)]
pub fn check_agent(
    input: &CheckInput,
    state: &mut Value,
    fetcher: &dyn LogFetcher<Error = String>,
    runner: Option<&dyn ClaudeRunner>,
    clock: &dyn Clock,
) -> CheckOutcome {
    // `state[name] || {}`
    let obj = state.as_object_mut().expect("state must be object");
    if !obj.contains_key(input.name) {
        obj.insert(input.name.to_string(), json!({
            "last_triage_at": null,
            "triages_last_hour": []
        }));
    }
    let mut agent_state = AgentState::from_value(obj.get(input.name));

    match cooldown_state(&mut agent_state, input.watch, clock) {
        CooldownDecision::Cooldown { seconds_left } => {
            obj.insert(input.name.into(), agent_state.to_value());
            return CheckOutcome::Cooldown(format!("{seconds_left}s"));
        }
        CooldownDecision::RateLimited { current, max } => {
            obj.insert(input.name.into(), agent_state.to_value());
            return CheckOutcome::RateLimited(format!(
                "{current}/{max} na última hora"
            ));
        }
        CooldownDecision::Proceed => {}
    }

    let logs = match fetcher.fetch(input.watch) {
        Ok(l) => l,
        Err(e) => {
            obj.insert(input.name.into(), agent_state.to_value());
            return CheckOutcome::Error(e.to_string());
        }
    };

    if (logs.len() as i64) < input.watch.threshold {
        obj.insert(input.name.into(), agent_state.to_value());
        return CheckOutcome::Ok(format!("{}/{} eventos", logs.len(), input.watch.threshold));
    }

    let prompt = build_triage_prompt(input.name, input.watch, input.mini_context, &logs);

    if input.dry_run {
        obj.insert(input.name.into(), agent_state.to_value());
        return CheckOutcome::WouldTriage {
            events: logs.len(),
            // JS `.length` em strings é UTF-16 units.
            prompt_chars: prompt.encode_utf16().count(),
        };
    }

    let runner = runner.expect("runner is required when dry_run is false");
    let r = runner.run(&prompt, input.project_root, input.model);

    let ts = clock.now_iso();
    agent_state.last_triage_at = Some(ts.clone());
    agent_state.triages_last_hour.push(json!({
        "ts": ts,
        "events": logs.len(),
    }));

    let mut obs = Map::new();
    obs.insert("type".into(), Value::String("firefighter_triage".into()));
    obs.insert("ts".into(), Value::String(ts.clone()));
    obs.insert("agent".into(), Value::String(input.name.into()));
    obs.insert("query".into(), Value::String(input.watch.query.into()));
    obs.insert("events".into(), Value::from(logs.len()));
    obs.insert("threshold".into(), Value::from(input.watch.threshold));
    obs.insert("diagnosis".into(), Value::String(r.stdout.trim().into()));
    obs.insert("exit_code".into(), Value::from(r.code));
    if r.code != 0 {
        obs.insert(
            "stderr".into(),
            Value::String(slice_utf16(&r.stderr, 300)),
        );
    }
    let _ = append_observation(input.mem_dir, &Value::Object(obs));

    obj.insert(input.name.into(), agent_state.to_value());
    CheckOutcome::Triaged {
        events: logs.len(),
        exit_code: r.code,
    }
}

/// `<mem_dir>/observations.jsonl` ← append line. Migrado pra
/// core-storage::JsonlRepository: rotação automática a 50k linhas/10MB,
/// reads transparentes via concat de partes. Hot path em observação
/// de PostToolUse — pode receber milhares de linhas/min.
pub fn append_observation(mem_dir: &Path, obs: &Value) -> std::io::Result<()> {
    let r = core_storage::JsonlRepository::<Value>::open_default(
        mem_dir.join("observations.jsonl"),
    )
    .map_err(|e| match e {
        core_storage::StorageError::Io(io) => io,
        core_storage::StorageError::Serialize(_) => {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "serialize")
        }
    })?;
    r.append(obs).map_err(|e| match e {
        core_storage::StorageError::Io(io) => io,
        core_storage::StorageError::Serialize(_) => {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "serialize")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn state_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let v = json!({"foo": {"last_triage_at": "2026-04-19T00:00:00.000Z", "triages_last_hour": []}});
        save_state(tmp.path(), &v).unwrap();
        let r = load_state(tmp.path());
        assert_eq!(r, v);
    }

    #[test]
    fn load_state_missing_returns_empty_object() {
        let tmp = TempDir::new().unwrap();
        let v = load_state(tmp.path());
        assert!(v.as_object().unwrap().is_empty());
    }

    #[test]
    fn prune_drops_old_entries() {
        let c = FixedClock(3_600_000 + 1_000_000); // now = 1h+1000s after epoch
        let entries = vec![
            // within hour → keep
            json!({"ts":"1970-01-01T00:59:00.000Z","events":1}),
            // more than 1h ago (exactly 1h → cutoff is NOT inclusive, so drop)
            json!({"ts":"1970-01-01T00:00:00.000Z","events":2}),
        ];
        let out = prune_old_triages(&entries, &c);
        // cutoff = 4600000 - 3600000 = 1000000. entry 0 ts=59min=59*60000=3540000 > 1000000 → keep
        // entry 1 ts=0 > 1000000? no → drop
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["events"], 1);
    }

    #[test]
    fn parse_args_all_flags() {
        let a = parse_args(&["--once", "--dry-run", "--interval", "30", "--agent", "foo"]);
        assert!(a.once);
        assert!(a.dry_run);
        assert_eq!(a.interval, 30);
        assert_eq!(a.agent_name.as_deref(), Some("foo"));
    }

    #[test]
    fn parse_args_interval_invalid_falls_back() {
        let a = parse_args(&["--interval", "abc"]);
        assert_eq!(a.interval, 60);
    }

    #[test]
    fn build_triage_prompt_empty_logs() {
        let w = WatchCfg {
            query: "service:foo status:error",
            threshold: 5,
            window_minutes: 10,
            cooldown_seconds: 300,
            max_triages_per_hour: 3,
        };
        let p = build_triage_prompt("fogo", &w, "runbook", &[]);
        assert!(p.contains("Você é o bombeiro **fogo**"));
        assert!(p.contains("_(sem detalhes)_"));
        assert!(p.contains("threshold era 5"));
    }

    #[test]
    fn cooldown_in_window() {
        let c = FixedClock(10_000); // 10s
        let mut s = AgentState {
            last_triage_at: Some(format_iso_millis(0)), // t=0
            triages_last_hour: vec![],
        };
        let w = WatchCfg {
            query: "",
            threshold: 1,
            window_minutes: 1,
            cooldown_seconds: 300,
            max_triages_per_hour: 3,
        };
        let d = cooldown_state(&mut s, &w, &c);
        // sinceMs = 10000, cooldown = 300000. delta = 290000 → ceil/1000 = 290.
        assert_eq!(d, CooldownDecision::Cooldown { seconds_left: 290 });
    }

    #[test]
    fn rate_limit_fires() {
        let c = FixedClock(10_000_000); // well past any cooldown
        let mut s = AgentState {
            last_triage_at: None,
            triages_last_hour: (0..3)
                .map(|i| json!({"ts": format_iso_millis(9_000_000 + i as i64), "events": 1}))
                .collect(),
        };
        let w = WatchCfg {
            query: "",
            threshold: 1,
            window_minutes: 1,
            cooldown_seconds: 60,
            max_triages_per_hour: 3,
        };
        let d = cooldown_state(&mut s, &w, &c);
        assert!(matches!(d, CooldownDecision::RateLimited { current: 3, max: 3 }));
    }
}
