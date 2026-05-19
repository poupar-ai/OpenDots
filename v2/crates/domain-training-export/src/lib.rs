//! `domain-training-export` — extrai chat history dos agentes pra dataset
//! de fine-tune (SFT).
//!
//! Hexagonal: módulo de domínio puro, lê só do storage (JSONL existente)
//! e devolve estruturas. Adapter HTTP é separado em `api-http`.
//!
//! Caso de uso principal: fine-tune de modelos menores (Haiku, Llama-3-8B,
//! etc) com a identidade/comportamento dos agentes do projeto. Cada
//! agente vira um dataset próprio (filtrado por `agent`); cada conversa
//! contínua vira **uma sample** com a sequência de turns.
//!
//! Formatos suportados:
//! - `AnthropicSft`: `{"messages":[{"role":"user","content":"..."},...]}` (JSONL, 1 sample/linha)
//! - `OpenaiSft`: igual mas adiciona suporte a `system` role inicial
//! - `MessagesJsonl`: mesmo shape do AnthropicSft, sem opiniões

use core_storage::JsonlRepository;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("agente sem chat: {0}")]
    NoChatHistory(String),
    #[error("storage: {0}")]
    Storage(#[from] core_storage::StorageError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    AnthropicSft,
    OpenaiSft,
    MessagesJsonl,
}

impl Format {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "anthropic-sft" | "anthropic_sft" => Some(Self::AnthropicSft),
            "openai-sft" | "openai_sft" => Some(Self::OpenaiSft),
            "messages" | "messages-jsonl" | "messages_jsonl" => Some(Self::MessagesJsonl),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub agent: String,
    pub format: Format,
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
    /// Gap (ms) entre turns acima do qual quebra em nova session.
    /// Default: 600_000 (10 min).
    pub max_gap_ms: u64,
    /// Sessions com menos turns que isso são descartadas (1 turn = ruído).
    pub min_turns_per_sample: usize,
    /// Quando true, prepende um `{role:"system",content:<system_prompt>}`
    /// na sample. Se `system_prompt` for None, ignora.
    pub system_prompt: Option<String>,
    /// Quando true (default), descarta turns marcados como `blocked` ou
    /// com `code != 0` (erros do agente). Mantém só "clean" pro fine-tune.
    pub clean_only: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            agent: String::new(),
            format: Format::AnthropicSft,
            from_ms: None,
            to_ms: None,
            max_gap_ms: 600_000,
            min_turns_per_sample: 2,
            system_prompt: None,
            clean_only: true,
        }
    }
}

fn chat_path(mem_dir: &Path, agent: &str) -> PathBuf {
    mem_dir.join("agent_chats").join(format!("{agent}.jsonl"))
}

/// Lê o JSONL do agente, filtra/agrupa por session, devolve samples no
/// formato escolhido. Cada sample = 1 `Value` JSON pronto pra escrever
/// numa linha.
pub fn export(mem_dir: &Path, opts: &ExportOptions) -> Result<Vec<Value>, ExportError> {
    let path = chat_path(mem_dir, &opts.agent);
    if !path.exists() {
        return Err(ExportError::NoChatHistory(opts.agent.clone()));
    }
    let repo: JsonlRepository<Value> = JsonlRepository::open_default(path)?;
    let raw_turns = repo.read_all();

    // Filtros básicos
    let filtered: Vec<Value> = raw_turns
        .into_iter()
        .filter(|t| {
            // Janela temporal (campo `ts` em ISO ou `ts_ms`)
            let ts_ms = extract_ts_ms(t);
            if let Some(min) = opts.from_ms {
                if let Some(t) = ts_ms {
                    if t < min {
                        return false;
                    }
                }
            }
            if let Some(max) = opts.to_ms {
                if let Some(t) = ts_ms {
                    if t > max {
                        return false;
                    }
                }
            }
            // clean_only
            if opts.clean_only {
                if t.get("blocked").and_then(Value::as_bool).unwrap_or(false) {
                    return false;
                }
                if t.get("code").and_then(Value::as_i64).unwrap_or(0) != 0 {
                    return false;
                }
            }
            // tem role + content
            let role_ok = t.get("role").and_then(Value::as_str).is_some();
            let content_ok = t.get("content").and_then(Value::as_str).is_some();
            role_ok && content_ok
        })
        .collect();

    // Agrupa em sessions por gap temporal
    let sessions = group_by_gap(&filtered, opts.max_gap_ms);

    // Formata cada session como uma sample no formato escolhido
    let mut out = Vec::with_capacity(sessions.len());
    for session in sessions {
        if session.len() < opts.min_turns_per_sample {
            continue;
        }
        let messages = build_messages(&session, opts);
        let sample = match opts.format {
            Format::AnthropicSft | Format::MessagesJsonl => {
                json!({ "messages": messages })
            }
            Format::OpenaiSft => {
                // Mesma shape — diferença é convencional. Reservado pra
                // expansão futura (loss_weight, etc).
                json!({ "messages": messages })
            }
        };
        out.push(sample);
    }
    Ok(out)
}

fn build_messages(session: &[Value], opts: &ExportOptions) -> Vec<Value> {
    let mut msgs = Vec::with_capacity(session.len() + 1);
    if let Some(sys) = &opts.system_prompt {
        if !sys.is_empty() {
            msgs.push(json!({ "role": "system", "content": sys }));
        }
    }
    for turn in session {
        let role = turn.get("role").and_then(Value::as_str).unwrap_or("user");
        let content = turn.get("content").and_then(Value::as_str).unwrap_or("");
        // Normaliza roles: chat de agente costuma usar "assistant" ou "agent"
        let normalized_role = match role {
            "agent" | "assistant" => "assistant",
            _ => "user",
        };
        msgs.push(json!({ "role": normalized_role, "content": content }));
    }
    msgs
}

/// Quebra a lista em sessions sempre que dois turns consecutivos têm
/// gap > `max_gap_ms`. Sessões vazias filtradas.
fn group_by_gap(turns: &[Value], max_gap_ms: u64) -> Vec<Vec<Value>> {
    let mut sessions: Vec<Vec<Value>> = Vec::new();
    let mut current: Vec<Value> = Vec::new();
    let mut last_ms: Option<u64> = None;
    for t in turns {
        let ts = extract_ts_ms(t);
        if let (Some(prev), Some(now)) = (last_ms, ts) {
            if now > prev && now - prev > max_gap_ms && !current.is_empty() {
                sessions.push(std::mem::take(&mut current));
            }
        }
        last_ms = ts.or(last_ms);
        current.push(t.clone());
    }
    if !current.is_empty() {
        sessions.push(current);
    }
    sessions
}

/// Extrai ts em ms desde epoch. Suporta `ts_ms` (u64 direto) ou `ts` (ISO 8601).
fn extract_ts_ms(t: &Value) -> Option<u64> {
    if let Some(ms) = t.get("ts_ms").and_then(Value::as_u64) {
        return Some(ms);
    }
    let iso = t.get("ts").and_then(Value::as_str)?;
    parse_iso_to_ms(iso)
}

/// Parser ISO 8601 mínimo: `YYYY-MM-DDTHH:MM:SS[.frac]Z`.
/// Não cobre TZ offsets — entradas sem `Z` retornam None.
fn parse_iso_to_ms(s: &str) -> Option<u64> {
    let s = s.trim_end_matches('Z');
    let (date, time) = s.split_once('T')?;
    let mut date_parts = date.split('-');
    let y: i64 = date_parts.next()?.parse().ok()?;
    let m: u64 = date_parts.next()?.parse().ok()?;
    let d: u64 = date_parts.next()?.parse().ok()?;
    let (hms, frac) = match time.split_once('.') {
        Some((a, b)) => (a, b),
        None => (time, "0"),
    };
    let mut hms_parts = hms.split(':');
    let hh: u64 = hms_parts.next()?.parse().ok()?;
    let mm: u64 = hms_parts.next()?.parse().ok()?;
    let ss: u64 = hms_parts.next()?.parse().ok()?;
    // Fração — usa só os 3 primeiros dígitos como ms
    let frac_ms: u64 = frac.chars().take(3).collect::<String>().parse().unwrap_or(0);
    Some(naive_to_unix_ms(y, m, d, hh, mm, ss, frac_ms))
}

/// Converte (y,m,d,h,m,s,frac_ms) UTC pra ms desde epoch via algoritmo
/// do calendário gregoriano (válido pra y >= 1970). Sem deps de crate.
fn naive_to_unix_ms(y: i64, m: u64, d: u64, hh: u64, mm: u64, ss: u64, ms: u64) -> u64 {
    // Days from 1970-01-01 to y-m-d:
    let mut year = y;
    let mut month = m as i64;
    if month <= 2 {
        year -= 1;
        month += 12;
    }
    let a = year / 100;
    let b = a / 4;
    let c = 2 - a + b;
    let e = (365.25 * (year + 4716) as f64) as i64;
    let f = (30.6001 * (month + 1) as f64) as i64;
    let jdn = c + d as i64 + e + f - 1524;
    let days_from_epoch = jdn - 2440588;
    let secs = days_from_epoch as u64 * 86_400 + hh * 3600 + mm * 60 + ss;
    secs * 1000 + ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn mk_chat_file(mem: &Path, agent: &str, lines: &[Value]) {
        let dir = mem.join("agent_chats");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{agent}.jsonl"));
        let mut out = String::new();
        for l in lines {
            out.push_str(&serde_json::to_string(l).unwrap());
            out.push('\n');
        }
        std::fs::write(p, out).unwrap();
    }

    #[test]
    fn export_groups_by_gap_and_filters_role_content() {
        let tmp = tempdir().unwrap();
        let mem = tmp.path();
        // 4 turns: 2 contínuos (session A), 2 após 15min (session B)
        mk_chat_file(
            mem,
            "manager",
            &[
                json!({"ts":"2026-05-19T12:00:00Z","role":"user","content":"oi"}),
                json!({"ts":"2026-05-19T12:00:05Z","role":"assistant","content":"olá"}),
                json!({"ts":"2026-05-19T12:15:30Z","role":"user","content":"de novo"}),
                json!({"ts":"2026-05-19T12:15:31Z","role":"assistant","content":"oi de volta"}),
            ],
        );
        let opts = ExportOptions {
            agent: "manager".into(),
            ..Default::default()
        };
        let samples = export(mem, &opts).unwrap();
        assert_eq!(samples.len(), 2, "espera 2 sessions");
        let s0 = samples[0].get("messages").and_then(Value::as_array).unwrap();
        assert_eq!(s0.len(), 2);
        assert_eq!(s0[0]["role"], "user");
        assert_eq!(s0[1]["role"], "assistant");
    }

    #[test]
    fn export_drops_blocked_and_error_turns_when_clean_only() {
        let tmp = tempdir().unwrap();
        let mem = tmp.path();
        mk_chat_file(
            mem,
            "manager",
            &[
                json!({"ts":"2026-05-19T12:00:00Z","role":"user","content":"oi"}),
                json!({"ts":"2026-05-19T12:00:05Z","role":"assistant","content":"x","blocked":true}),
                json!({"ts":"2026-05-19T12:00:10Z","role":"assistant","content":"y","code":1}),
            ],
        );
        let opts = ExportOptions {
            agent: "manager".into(),
            min_turns_per_sample: 1,
            ..Default::default()
        };
        let samples = export(mem, &opts).unwrap();
        // 1 turn limpo após filtros — vira 1 session
        assert_eq!(samples.len(), 1);
        let msgs = samples[0].get("messages").and_then(Value::as_array).unwrap();
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn system_prompt_prepended_when_set() {
        let tmp = tempdir().unwrap();
        let mem = tmp.path();
        mk_chat_file(
            mem,
            "manager",
            &[
                json!({"ts":"2026-05-19T12:00:00Z","role":"user","content":"oi"}),
                json!({"ts":"2026-05-19T12:00:05Z","role":"assistant","content":"olá"}),
            ],
        );
        let opts = ExportOptions {
            agent: "manager".into(),
            system_prompt: Some("Você é o manager.".into()),
            ..Default::default()
        };
        let samples = export(mem, &opts).unwrap();
        let msgs = samples[0].get("messages").and_then(Value::as_array).unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "Você é o manager.");
    }

    #[test]
    fn no_chat_file_returns_err() {
        let tmp = tempdir().unwrap();
        let opts = ExportOptions {
            agent: "ghost".into(),
            ..Default::default()
        };
        let err = export(tmp.path(), &opts).unwrap_err();
        assert!(matches!(err, ExportError::NoChatHistory(_)));
    }
}
