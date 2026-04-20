//! Domain-nudge — porta de `plugin/scripts/nudge.cjs` (lógica pura + I/O
//! determinístico).
//!
//! # Escopo
//! Expõe as funções da memória proativa dos agentes autônomos: rate-limit,
//! re-render dos MDs, construção do prompt, extração do JSON da resposta, e
//! persistência de decisões/gotchas (delegando ao `domain-graph` para o
//! grafo).
//!
//! # Fora de escopo
//! - `runNudgeClaude` (spawn do CLI) — pertence a `adapter-claude-cli`.
//! - `scheduleNudge` (orquestração fire-and-forget) — pertence à camada de API.
//! - `appendObservation` + `agent_logs` append — pertence ao adapter de
//!   observabilidade; não é lógica de domínio.

use std::fs;
use std::path::{Path, PathBuf};

use domain_graph::Clock;
use regex::Regex;
use serde_json::{json, Value};

pub use domain_graph::{FixedClock, SystemClock};

// ── Constantes ──────────────────────────────────────────────────────────

/// Intervalo mínimo entre nudges por agente (`MIN_GAP_MS` em `nudge.cjs:20`).
pub const MIN_GAP_MS: i64 = 60 * 60 * 1000;

/// Tamanho mínimo da resposta para disparar nudge (`MIN_RESPONSE_LEN`,
/// `nudge.cjs:21`).
pub const MIN_RESPONSE_LEN: usize = 120;

const NUDGE_STATE_FILE: &str = "nudge_state.json";

// ── Helpers ──────────────────────────────────────────────────────────────

/// Fatiamento equivalente a `String.prototype.slice(0, n)` em JS — opera
/// sobre code-units UTF-16, não bytes nem chars Rust.
fn slice_utf16(s: &str, n: usize) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    if units.len() <= n {
        return s.to_string();
    }
    String::from_utf16_lossy(&units[..n])
}

/// Lê um `.jsonl`, ignorando linhas vazias/corruptas. Retorna vazio se o
/// arquivo não existir.
fn read_jsonl(file: &Path) -> Vec<Value> {
    let Ok(raw) = fs::read_to_string(file) else {
        return Vec::new();
    };
    raw.split('\n')
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

// ── Estado (rate-limit por agente) ──────────────────────────────────────

pub fn state_path(mem_dir: &Path) -> PathBuf {
    mem_dir.join(NUDGE_STATE_FILE)
}

/// Carrega o state. Retorna objeto vazio se ausente ou corrupto — paridade
/// com `loadState` em `nudge.cjs:33`.
pub fn load_state(mem_dir: &Path) -> Value {
    let path = state_path(mem_dir);
    let Ok(raw) = fs::read_to_string(&path) else {
        return json!({});
    };
    serde_json::from_str(&raw).unwrap_or_else(|_| json!({}))
}

/// Persiste o state em disco (pretty-print com 2 espaços + `\n` final).
/// Swallow de erros para paridade com `saveState` em `nudge.cjs:38`.
pub fn save_state(mem_dir: &Path, state: &Value) {
    let Ok(mut body) = serde_json::to_string_pretty(state) else {
        return;
    };
    body.push('\n');
    let _ = fs::write(state_path(mem_dir), body);
}

/// Verdadeiro se o agente está em cooldown (nudge recente). Recebe `now_ms`
/// para ser testável — em produção use `SystemClock.now_ms()`.
pub fn within_cooldown(state: &Value, agent: &str, now_ms: i64) -> bool {
    let Some(s) = state.get(agent) else {
        return false;
    };
    let Some(last_ts) = s.get("last_ts").and_then(|v| v.as_str()) else {
        return false;
    };
    let Some(last_ms) = parse_iso_to_ms(last_ts) else {
        return false;
    };
    (now_ms - last_ms) < MIN_GAP_MS
}

/// Equivalente a `new Date(s).getTime()` — retorna `None` em vez de `NaN`.
fn parse_iso_to_ms(s: &str) -> Option<i64> {
    // Delegamos ao parser do domain-graph via função pública? Não há uma.
    // Reimplementamos o subset necessário: ISO `YYYY-MM-DDTHH:MM:SS.sssZ`
    // (formato canônico produzido por `Date.prototype.toISOString`).
    let bytes = s.as_bytes();
    if bytes.len() < 10 {
        return None;
    }
    let year: i64 = s.get(..4)?.parse().ok()?;
    let month: i64 = s.get(5..7)?.parse().ok()?;
    let day: i64 = s.get(8..10)?.parse().ok()?;
    let (hour, min, sec, ms) = if bytes.len() >= 19 {
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
    let days = days_from_civil(year, month, day);
    Some(days * 86_400_000 + hour * 3_600_000 + min * 60_000 + sec * 1_000 + ms)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// ── Re-render dos markdowns ─────────────────────────────────────────────

/// Re-renderiza `DECISIONS.md` a partir de `decisions.jsonl`. No-op se não
/// houver decisões. Paridade com `renderDecisionsMd` em `nudge.cjs:60`.
pub fn render_decisions_md(mem_dir: &Path) {
    let jsonl = mem_dir.join("decisions.jsonl");
    let md = mem_dir.join("DECISIONS.md");
    let mut decisions = read_jsonl(&jsonl);
    if decisions.is_empty() {
        return;
    }
    decisions.reverse();
    let mut out: Vec<String> = vec!["# Decisões do projeto".to_string(), String::new()];
    for d in &decisions {
        let date = d
            .get("ts")
            .and_then(|v| v.as_str())
            .map(|s| slice_utf16(s, 10))
            .unwrap_or_default();
        let title = d
            .get("what")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                d.get("text")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
            })
            .map(str::to_string)
            .unwrap_or_else(|| "(sem título)".to_string());
        out.push(format!("## {date} — {title}"));
        if let Some(why) = d.get("why").and_then(|v| v.as_str()) {
            if !why.is_empty() {
                out.push(String::new());
                out.push(why.to_string());
            }
        }
        out.push(String::new());
    }
    let _ = fs::write(md, out.join("\n"));
}

/// Re-renderiza `GOTCHAS.md` a partir de `gotchas.jsonl`, deduplicando por
/// `(file, description[:60])`. Paridade com `renderGotchasMd` em
/// `nudge.cjs:75`.
pub fn render_gotchas_md(mem_dir: &Path) {
    let jsonl = mem_dir.join("gotchas.jsonl");
    let md = mem_dir.join("GOTCHAS.md");
    let gotchas = read_jsonl(&jsonl);
    if gotchas.is_empty() {
        return;
    }
    let mut seen = std::collections::HashSet::new();
    let mut unique: Vec<&Value> = Vec::new();
    for g in gotchas.iter().rev() {
        let file_key = js_template_string(g.get("file"));
        let desc = g
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let key = format!("{}|{}", file_key, slice_utf16(desc, 60));
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        unique.push(g);
    }
    if unique.is_empty() {
        return;
    }
    let mut out: Vec<String> = vec!["# Armadilhas conhecidas".to_string(), String::new()];
    for g in unique {
        let date = g
            .get("ts")
            .and_then(|v| v.as_str())
            .map(|s| slice_utf16(s, 10))
            .unwrap_or_default();
        let file_disp = g
            .get("file")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("(sem arquivo)");
        out.push(format!("## {date} — `{file_disp}`"));
        out.push(String::new());
        let desc = g
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        out.push(desc.to_string());
        if let Some(hint) = g.get("failure_hint").and_then(|v| v.as_str()) {
            if !hint.is_empty() {
                out.push(String::new());
                out.push(format!("_Sintoma:_ {hint}"));
            }
        }
        out.push(String::new());
    }
    let _ = fs::write(md, out.join("\n"));
}

/// Equivalente a `${value}` em template literal JS. Para Value::Null
/// retorna `"null"`, para ausente (None) retorna `"undefined"`.
fn js_template_string(v: Option<&Value>) -> String {
    match v {
        None => "undefined".to_string(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => other.to_string(),
    }
}

// ── Prompt + extração ───────────────────────────────────────────────────

/// Constrói o prompt que é enviado ao `claude -p` para o nudge. Paridade
/// byte-a-byte com `buildNudgePrompt` em `nudge.cjs:103`.
pub fn build_nudge_prompt(agent: &str, response_text: &str, trigger: &str) -> String {
    let trimmed = slice_utf16(response_text, 2000);
    PROMPT_TEMPLATE
        .replace("__AGENT__", agent)
        .replace("__TRIGGER__", trigger)
        .replace("__TRIMMED__", &trimmed)
}

const PROMPT_TEMPLATE: &str = r#"Você é o agente **__AGENT__**. Acabou de executar uma tarefa em modo autônomo (trigger: __TRIGGER__). Sua resposta foi registrada no canal do agente.

## Sua resposta
__TRIMMED__

## Sua tarefa agora
Olhe SÓ pra sua resposta acima. Você identificou algo concreto que vale persistir na memória do projeto?

Responda EXCLUSIVAMENTE com UM JSON em UMA linha. Sem markdown. Sem comentário. Sem texto antes nem depois:

- `{"kind":"none"}` — rotina, nada significativo, sem material pra promover.
- `{"kind":"decision","what":"<o que foi decidido>","why":"<por quê, 1-2 frases>"}` — decisão técnica do seu escopo (com motivação), digna de DECISIONS.md.
- `{"kind":"gotcha","file":"<arquivo ou área>","description":"<bug + conserto em 1-2 frases>","failure_hint":"<sintoma, opcional>"}` — armadilha concreta (bug encontrado + correção aplicada).

Critérios:
- Decisão: específica + com motivo real. "Fiz X" sem por quê → none.
- Gotcha: erro concreto com correção. Frustração genérica → none.
- Em dúvida → none. Melhor perder sinal fraco do que poluir a memória.

Responda só o JSON."#;

/// Extrai o primeiro `{…}` do texto (greedy, cruza linhas) e tenta parsear
/// como JSON. Retorna `None` se nenhum JSON válido for encontrado. Paridade
/// com `extractJson` em `nudge.cjs:127`.
pub fn extract_json(text: &str) -> Option<Value> {
    if text.is_empty() {
        return None;
    }
    static RE_INIT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE_INIT.get_or_init(|| Regex::new(r"(?s)\{.*\}").unwrap());
    let m = re.find(text)?;
    serde_json::from_str(m.as_str()).ok()
}

// ── Persist ─────────────────────────────────────────────────────────────

/// Resultado de uma tentativa de persistência.
#[derive(Debug, Clone, PartialEq)]
pub enum PersistResult {
    /// Sem conteúdo para persistir (what/description vazios). Equivalente a
    /// `return null` em `nudge.cjs`.
    Skipped,
    /// Entry persistida com sucesso. Contém o JSON exato gravado.
    Ok(Value),
    /// Erro de I/O durante o append/render. Equivalente a
    /// `return { err: e.message }` em `nudge.cjs`.
    Err(String),
}

/// Persiste uma decisão em `decisions.jsonl`, re-renderiza `DECISIONS.md`
/// e chama `domain_graph::on_decision`. Paridade com `persistDecision` em
/// `nudge.cjs:160`.
pub fn persist_decision(
    mem_dir: &Path,
    agent: &str,
    parsed: &Value,
    trigger: &str,
    clock: &dyn Clock,
) -> PersistResult {
    let what = parsed
        .get("what")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    let why = parsed
        .get("why")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    if what.is_empty() {
        return PersistResult::Skipped;
    }
    let entry = json!({
        "what": what,
        "why": why,
        "ts": clock.now_iso(),
        "session_id": format!("nudge:{agent}"),
        "source": "nudge",
        "agent": agent,
        "trigger": trigger,
    });
    let file = mem_dir.join("decisions.jsonl");
    if let Err(e) = append_jsonl(&file, &entry) {
        return PersistResult::Err(e.to_string());
    }
    render_decisions_md(mem_dir);
    domain_graph::on_decision(mem_dir, &entry, &[], clock);
    PersistResult::Ok(entry)
}

/// Persiste uma gotcha em `gotchas.jsonl`, re-renderiza `GOTCHAS.md` e
/// chama `domain_graph::on_gotcha`. Paridade com `persistGotcha` em
/// `nudge.cjs:181`.
pub fn persist_gotcha(
    mem_dir: &Path,
    agent: &str,
    parsed: &Value,
    trigger: &str,
    clock: &dyn Clock,
) -> PersistResult {
    let description = parsed
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    if description.is_empty() {
        return PersistResult::Skipped;
    }
    let file_field = trimmed_or_null(parsed.get("file"));
    let failure_hint = trimmed_or_null(parsed.get("failure_hint"));
    let entry = json!({
        "file": file_field,
        "description": description,
        "failure_hint": failure_hint,
        "ts": clock.now_iso(),
        "source": "nudge",
        "agent": agent,
        "trigger": trigger,
    });
    let file = mem_dir.join("gotchas.jsonl");
    if let Err(e) = append_jsonl(&file, &entry) {
        return PersistResult::Err(e.to_string());
    }
    render_gotchas_md(mem_dir);
    domain_graph::on_gotcha(mem_dir, &entry, clock);
    PersistResult::Ok(entry)
}

/// Replica `(parsed.x || '').trim() || null` do `.cjs`: retorna `Null` se
/// ausente/vazio após trim, senão string trim.
fn trimmed_or_null(v: Option<&Value>) -> Value {
    let s = v.and_then(|v| v.as_str()).unwrap_or_default().trim();
    if s.is_empty() {
        Value::Null
    } else {
        Value::String(s.to_string())
    }
}

fn append_jsonl(path: &Path, entry: &Value) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let mut line = serde_json::to_string(entry)?;
    line.push('\n');
    f.write_all(line.as_bytes())
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn read_file(p: &Path) -> String {
        fs::read_to_string(p).unwrap()
    }

    // ── slice_utf16 ─────────────────────────────────────────────────────

    #[test]
    fn slice_utf16_shorter_than_limit() {
        assert_eq!(slice_utf16("abc", 10), "abc");
    }

    #[test]
    fn slice_utf16_truncates_ascii() {
        assert_eq!(slice_utf16("abcdefgh", 3), "abc");
    }

    #[test]
    fn slice_utf16_bmp_char() {
        // Cada char é 1 code unit.
        assert_eq!(slice_utf16("áéíóú", 3), "áéí");
    }

    #[test]
    fn slice_utf16_surrogate_pair() {
        // Emoji "🙂" = 2 code units (surrogate pair).
        // Slicing a 1 deixa só o high-surrogate — String::from_utf16_lossy
        // substitui por U+FFFD.
        let s = "a🙂b";
        let r = slice_utf16(s, 2);
        // JS: "a🙂".slice(0,2) = "a" + high-surrogate isolado. Encode-lossy
        // troca por REPLACEMENT CHARACTER.
        assert_eq!(r, "a\u{FFFD}");
    }

    // ── State ───────────────────────────────────────────────────────────

    #[test]
    fn load_state_missing_returns_empty_object() {
        let dir = tempdir().unwrap();
        assert_eq!(load_state(dir.path()), json!({}));
    }

    #[test]
    fn load_state_corrupt_returns_empty_object() {
        let dir = tempdir().unwrap();
        fs::write(state_path(dir.path()), "nao-eh-json").unwrap();
        assert_eq!(load_state(dir.path()), json!({}));
    }

    #[test]
    fn save_then_load_state_roundtrip() {
        let dir = tempdir().unwrap();
        let state = json!({
            "internal_firefighter": {"last_ts": "2026-04-19T10:00:00.000Z", "trigger": "triage"}
        });
        save_state(dir.path(), &state);
        let raw = read_file(&state_path(dir.path()));
        assert!(raw.ends_with('\n'));
        assert!(raw.starts_with('{'));
        assert_eq!(load_state(dir.path()), state);
    }

    // ── within_cooldown ─────────────────────────────────────────────────

    #[test]
    fn within_cooldown_no_state_returns_false() {
        let state = json!({});
        assert!(!within_cooldown(&state, "frontend", 10_000));
    }

    #[test]
    fn within_cooldown_no_last_ts_returns_false() {
        let state = json!({"frontend": {"trigger": "x"}});
        assert!(!within_cooldown(&state, "frontend", 10_000));
    }

    #[test]
    fn within_cooldown_invalid_ts_returns_false() {
        let state = json!({"frontend": {"last_ts": "nao-iso"}});
        assert!(!within_cooldown(&state, "frontend", 10_000));
    }

    #[test]
    fn within_cooldown_recent_returns_true() {
        // last_ts = epoch-0 iso. now = 59min → ainda < 60min.
        let state = json!({"frontend": {"last_ts": "1970-01-01T00:00:00.000Z"}});
        assert!(within_cooldown(&state, "frontend", 59 * 60 * 1000));
    }

    #[test]
    fn within_cooldown_expired_returns_false() {
        // 61min depois: fora da janela.
        let state = json!({"frontend": {"last_ts": "1970-01-01T00:00:00.000Z"}});
        assert!(!within_cooldown(&state, "frontend", 61 * 60 * 1000));
    }

    // ── parse_iso_to_ms ─────────────────────────────────────────────────

    #[test]
    fn parse_iso_epoch() {
        assert_eq!(parse_iso_to_ms("1970-01-01T00:00:00.000Z"), Some(0));
    }

    #[test]
    fn parse_iso_roundtrip_matches_format_iso_millis() {
        use domain_graph::format_iso_millis;
        for ms in [0_i64, 1, 1_000, 1_776_602_096_789, 2_000_000_000_000] {
            let iso = format_iso_millis(ms);
            assert_eq!(
                parse_iso_to_ms(&iso),
                Some(ms),
                "roundtrip diverge para ms={ms}"
            );
        }
    }

    // ── Render decisions ────────────────────────────────────────────────

    #[test]
    fn render_decisions_md_no_file_is_noop() {
        let dir = tempdir().unwrap();
        render_decisions_md(dir.path());
        assert!(!dir.path().join("DECISIONS.md").exists());
    }

    #[test]
    fn render_decisions_md_reverse_order() {
        let dir = tempdir().unwrap();
        let jsonl = dir.path().join("decisions.jsonl");
        fs::write(
            &jsonl,
            "{\"what\":\"A\",\"why\":\"a\",\"ts\":\"2026-01-01T00:00:00.000Z\"}\n\
             {\"what\":\"B\",\"why\":\"b\",\"ts\":\"2026-01-02T00:00:00.000Z\"}\n",
        )
        .unwrap();
        render_decisions_md(dir.path());
        let md = read_file(&dir.path().join("DECISIONS.md"));
        // Reverse: B aparece antes de A.
        let ia = md.find("— A").unwrap();
        let ib = md.find("— B").unwrap();
        assert!(ib < ia);
        assert!(md.contains("2026-01-02 — B"));
    }

    #[test]
    fn render_decisions_md_fallback_title() {
        let dir = tempdir().unwrap();
        let jsonl = dir.path().join("decisions.jsonl");
        fs::write(&jsonl, "{\"ts\":\"2026-04-19T00:00:00.000Z\"}\n").unwrap();
        render_decisions_md(dir.path());
        let md = read_file(&dir.path().join("DECISIONS.md"));
        assert!(md.contains("(sem título)"));
    }

    #[test]
    fn render_decisions_md_prefers_what_over_text() {
        let dir = tempdir().unwrap();
        let jsonl = dir.path().join("decisions.jsonl");
        fs::write(
            &jsonl,
            "{\"what\":\"wins\",\"text\":\"loses\",\"ts\":\"2026-04-19T00:00:00.000Z\"}\n",
        )
        .unwrap();
        render_decisions_md(dir.path());
        let md = read_file(&dir.path().join("DECISIONS.md"));
        assert!(md.contains("— wins"));
        assert!(!md.contains("loses"));
    }

    // ── Render gotchas ──────────────────────────────────────────────────

    #[test]
    fn render_gotchas_md_dedup_same_key() {
        let dir = tempdir().unwrap();
        let jsonl = dir.path().join("gotchas.jsonl");
        fs::write(
            &jsonl,
            "{\"file\":\"a.rs\",\"description\":\"dup\",\"ts\":\"2026-01-01T00:00:00.000Z\"}\n\
             {\"file\":\"a.rs\",\"description\":\"dup\",\"ts\":\"2026-01-02T00:00:00.000Z\"}\n",
        )
        .unwrap();
        render_gotchas_md(dir.path());
        let md = read_file(&dir.path().join("GOTCHAS.md"));
        // Mais recente aparece, mas só UMA seção.
        let count = md.matches("## 2026").count();
        assert_eq!(count, 1);
        assert!(md.contains("2026-01-02"));
    }

    #[test]
    fn render_gotchas_md_no_file_fallback() {
        let dir = tempdir().unwrap();
        let jsonl = dir.path().join("gotchas.jsonl");
        fs::write(
            &jsonl,
            "{\"file\":null,\"description\":\"x\",\"ts\":\"2026-04-19T00:00:00.000Z\"}\n",
        )
        .unwrap();
        render_gotchas_md(dir.path());
        let md = read_file(&dir.path().join("GOTCHAS.md"));
        assert!(md.contains("(sem arquivo)"));
    }

    #[test]
    fn render_gotchas_md_failure_hint_appears() {
        let dir = tempdir().unwrap();
        let jsonl = dir.path().join("gotchas.jsonl");
        fs::write(
            &jsonl,
            "{\"file\":\"x\",\"description\":\"d\",\"failure_hint\":\"boom\",\"ts\":\"2026-04-19T00:00:00.000Z\"}\n",
        )
        .unwrap();
        render_gotchas_md(dir.path());
        let md = read_file(&dir.path().join("GOTCHAS.md"));
        assert!(md.contains("_Sintoma:_ boom"));
    }

    // ── Prompt ──────────────────────────────────────────────────────────

    #[test]
    fn build_nudge_prompt_contains_fields() {
        let p = build_nudge_prompt("frontend", "resposta curta", "auto_handoff");
        assert!(p.contains("**frontend**"));
        assert!(p.contains("trigger: auto_handoff"));
        assert!(p.contains("resposta curta"));
        assert!(p.contains("{\"kind\":\"none\"}"));
    }

    #[test]
    fn build_nudge_prompt_truncates_response() {
        let long = "a".repeat(3000);
        let p = build_nudge_prompt("x", &long, "t");
        // Só 2000 'a's estarão no prompt (+ o resto do template).
        let count = p.matches('a').count();
        // 'a' também aparece no template ("tarefa", "agora"...) — validamos
        // só que não chegou a 3000.
        assert!(count < 2500, "truncamento falhou: {count} 'a's");
        assert!(count >= 2000);
    }

    // ── extract_json ────────────────────────────────────────────────────

    #[test]
    fn extract_json_empty_returns_none() {
        assert!(extract_json("").is_none());
    }

    #[test]
    fn extract_json_no_brace_returns_none() {
        assert!(extract_json("nada por aqui").is_none());
    }

    #[test]
    fn extract_json_simple() {
        let r = extract_json("prefixo {\"kind\":\"none\"} sufixo").unwrap();
        assert_eq!(r, json!({"kind": "none"}));
    }

    #[test]
    fn extract_json_greedy_multiline() {
        let text = "lixo\n{\"kind\":\"decision\",\n\"what\":\"x\"}\nmais lixo";
        let r = extract_json(text).unwrap();
        assert_eq!(r["kind"], "decision");
        assert_eq!(r["what"], "x");
    }

    #[test]
    fn extract_json_invalid_inside_returns_none() {
        assert!(extract_json("{not json at all}").is_none());
    }

    // ── Persist ─────────────────────────────────────────────────────────

    #[test]
    fn persist_decision_empty_what_skips() {
        let dir = tempdir().unwrap();
        let clock = FixedClock(0);
        let r = persist_decision(
            dir.path(),
            "frontend",
            &json!({"what": "", "why": "algo"}),
            "triage",
            &clock,
        );
        assert_eq!(r, PersistResult::Skipped);
        assert!(!dir.path().join("decisions.jsonl").exists());
    }

    #[test]
    fn persist_decision_writes_entry() {
        let dir = tempdir().unwrap();
        let clock = FixedClock(1_700_000_000_000);
        let r = persist_decision(
            dir.path(),
            "frontend",
            &json!({"what": "decidi X", "why": "porque"}),
            "auto_handoff",
            &clock,
        );
        let entry = match r {
            PersistResult::Ok(v) => v,
            other => panic!("esperado Ok, veio {other:?}"),
        };
        assert_eq!(entry["what"], "decidi X");
        assert_eq!(entry["session_id"], "nudge:frontend");
        assert_eq!(entry["source"], "nudge");
        assert_eq!(entry["agent"], "frontend");
        assert_eq!(entry["trigger"], "auto_handoff");
        let jsonl = read_file(&dir.path().join("decisions.jsonl"));
        assert!(jsonl.ends_with('\n'));
        assert!(read_file(&dir.path().join("DECISIONS.md")).contains("— decidi X"));
    }

    #[test]
    fn persist_decision_trims_what_why() {
        let dir = tempdir().unwrap();
        let clock = FixedClock(0);
        let r = persist_decision(
            dir.path(),
            "x",
            &json!({"what": "  do  ", "why": "\t  reason  "}),
            "t",
            &clock,
        );
        let entry = match r {
            PersistResult::Ok(v) => v,
            _ => panic!(),
        };
        assert_eq!(entry["what"], "do");
        assert_eq!(entry["why"], "reason");
    }

    #[test]
    fn persist_gotcha_empty_description_skips() {
        let dir = tempdir().unwrap();
        let clock = FixedClock(0);
        let r = persist_gotcha(
            dir.path(),
            "x",
            &json!({"file": "a", "description": "   "}),
            "t",
            &clock,
        );
        assert_eq!(r, PersistResult::Skipped);
    }

    #[test]
    fn persist_gotcha_writes_entry_with_nulls() {
        let dir = tempdir().unwrap();
        let clock = FixedClock(0);
        let r = persist_gotcha(dir.path(), "x", &json!({"description": "bug"}), "t", &clock);
        let entry = match r {
            PersistResult::Ok(v) => v,
            _ => panic!(),
        };
        assert_eq!(entry["file"], Value::Null);
        assert_eq!(entry["failure_hint"], Value::Null);
        assert_eq!(entry["description"], "bug");
    }

    #[test]
    fn persist_gotcha_writes_entry_full() {
        let dir = tempdir().unwrap();
        let clock = FixedClock(0);
        let r = persist_gotcha(
            dir.path(),
            "x",
            &json!({"file": " a.rs ", "description": "d", "failure_hint": " boom "}),
            "t",
            &clock,
        );
        let entry = match r {
            PersistResult::Ok(v) => v,
            _ => panic!(),
        };
        assert_eq!(entry["file"], "a.rs");
        assert_eq!(entry["failure_hint"], "boom");
    }
}
