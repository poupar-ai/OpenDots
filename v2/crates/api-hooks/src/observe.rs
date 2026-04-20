//! Lógica do hook `PostToolUse` — porta de `plugin/scripts/observe.cjs`.
//!
//! O Claude Code chama este hook depois de cada `Write | Edit |
//! MultiEdit | Bash | Task | Agent` com um JSON no stdin que traz
//! `session_id`, `tool_name`, `tool_input`, `tool_response`. A função
//! do hook é gravar uma observação em `observations.jsonl` e propagar
//! o evento para o grafo de padrões (`domain-graph::update_graph`).
//!
//! ### Divisão em camadas
//!
//! | Função                       | Responsabilidade                                    | Testado          |
//! |------------------------------|-----------------------------------------------------|------------------|
//! | [`decide_observation`]       | Pura. Input → `Option<Observation>`.               | Unit + parity    |
//! | [`summarize_output`]         | Pura. Extrai resumo de stdout de pytest/cargo/tsc. | Unit             |
//! | [`should_skip_bash_command`] | Pura. Aplica o regex `SKIP_CMDS`.                  | Unit             |
//! | [`capture_diff`]             | Efeito: chama `git`. Tipado pra ser mockável.      | Integration/parity|
//! | [`run`] (no main)            | Glue: lê stdin, chama decide + effects.            | Parity           |
//!
//! ### Paridade com `.cjs`
//!
//! O `.cjs` usa `process.exit(0)` em vários pontos pra sair cedo —
//! precisamos mapear esses early-returns em [`Decision::Skip`]. Também
//! precisamos preservar a **ordem de campos** da observação JSON (o
//! `.cjs` insere `type`, `session_id`, `file`, `ts`, `agent_id?`, `diff?`
//! em ordem específica) — feature `preserve_order` do `serde_json`
//! garante isso no Rust.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use regex::Regex;
use serde_json::{json, Map, Value};

/// Máximo de caracteres do trecho de diff no campo `preview`.
/// Idêntico ao `.cjs` (`const DIFF_PREVIEW_MAX = 240`).
pub const DIFF_PREVIEW_MAX: usize = 240;

/// Regex de comandos bash ignorados. Matches com o literal do `.cjs`:
/// `/^(ls|ll|la|pwd|echo\s|cat\s|head\s|tail\s|which\s|true|false|clear|history|sleep|printf)\b/`
///
/// Nota: `\s` no JS matches single whitespace. Em Rust `regex` usa
/// `\s` idêntico. `\b` também bate em ambos.
fn skip_re() -> Regex {
    Regex::new(
        r"^(ls|ll|la|pwd|echo\s|cat\s|head\s|tail\s|which\s|true|false|clear|history|sleep|printf)\b",
    )
    .expect("regex literal válido")
}

/// Resultado da decisão a partir do input do hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Gravar esta observação (o caller ainda pode enriquecer com `diff`
    /// antes de escrever, usando `augment_file_edit_with_diff`).
    Record(Observation),
    /// Sair cedo. Motivo para debug/tests.
    Skip(SkipReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// `tool_name` ausente.
    NoToolName,
    /// `tool_name` não está no set relevante.
    IrrelevantTool,
    /// `tool_response` tem `error` ou `is_error` truthy.
    ToolError,
    /// Write/Edit sem `file_path`/`notebook_path`.
    NoFilePath,
    /// Bash com `cmd` vazio ou que bate `SKIP_CMDS`.
    BashSkipped,
}

/// Estrutura da observação gravada em `observations.jsonl`. Guardamos
/// como `Value` pra preservar a ordem de inserção exata do `.cjs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub value: Value,
    pub kind: ObsKind,
    /// Para `FileEdit`, guardamos o path relativo para o caller chamar
    /// `capture_diff`. `None` para outros kinds.
    pub rel_file: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObsKind {
    FileEdit,
    Bash,
    AgentCall,
}

/// Decide, a partir do input bruto do stdin, o que fazer.
///
/// Replica o fluxo linear do `.cjs`:
/// 1. Se `tool_name` ausente → `process.exit(0)`.
/// 2. Se `tool_name` não está em `{Write, Edit, MultiEdit, Bash, Task, Agent}` → exit.
/// 3. Se `tool_response.error || tool_response.is_error` → exit.
/// 4. Write/Edit/MultiEdit → `file_edit`.
/// 5. Bash → `bash` (com skip de comandos triviais).
/// 6. Task/Agent → `agent_call`.
pub fn decide_observation(
    input: &Value,
    project_root: &Path,
    ts: &str,
    agent_id: Option<&str>,
) -> Decision {
    let session_id = input.get("session_id").cloned().unwrap_or(Value::Null);
    let tool_name = match input.get("tool_name").and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s,
        _ => return Decision::Skip(SkipReason::NoToolName),
    };

    let relevant = matches!(
        tool_name,
        "Write" | "Edit" | "MultiEdit" | "Bash" | "Task" | "Agent"
    );
    if !relevant {
        return Decision::Skip(SkipReason::IrrelevantTool);
    }

    // tool_response.error || tool_response.is_error — JS truthy.
    if let Some(resp) = input.get("tool_response") {
        if is_truthy(resp.get("error")) || is_truthy(resp.get("is_error")) {
            return Decision::Skip(SkipReason::ToolError);
        }
    }

    let tool_input = input.get("tool_input").cloned().unwrap_or(Value::Null);
    let tool_response = input.get("tool_response").cloned().unwrap_or(Value::Null);

    match tool_name {
        "Write" | "Edit" | "MultiEdit" => {
            build_file_edit(&session_id, &tool_input, project_root, ts, agent_id)
        }
        "Bash" => build_bash(&session_id, &tool_input, &tool_response, ts, agent_id),
        "Task" | "Agent" => build_agent_call(&session_id, &tool_input, ts, agent_id),
        _ => unreachable!("relevant check já cobriu"),
    }
}

fn build_file_edit(
    session_id: &Value,
    tool_input: &Value,
    project_root: &Path,
    ts: &str,
    agent_id: Option<&str>,
) -> Decision {
    // file_path OU notebook_path. `.cjs`: `tool_input?.file_path || tool_input?.notebook_path || ''`.
    let file_path = tool_input
        .get("file_path")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            tool_input
                .get("notebook_path")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        });
    let file_path = match file_path {
        Some(s) => s,
        None => return Decision::Skip(SkipReason::NoFilePath),
    };

    let prefix = format!("{}/", project_root.display());
    let rel_file = if file_path.starts_with(&prefix) {
        file_path[prefix.len()..].to_string()
    } else {
        file_path.to_string()
    };

    // Ordem de campos: type, session_id, file, ts, agent_id?, (diff adicionado depois).
    let mut obs = Map::new();
    obs.insert("type".into(), json!("file_edit"));
    obs.insert("session_id".into(), session_id.clone());
    obs.insert("file".into(), json!(rel_file));
    obs.insert("ts".into(), json!(ts));
    if let Some(a) = agent_id {
        obs.insert("agent_id".into(), json!(a));
    }

    Decision::Record(Observation {
        value: Value::Object(obs),
        kind: ObsKind::FileEdit,
        rel_file: Some(rel_file),
    })
}

fn build_bash(
    session_id: &Value,
    tool_input: &Value,
    tool_response: &Value,
    ts: &str,
    agent_id: Option<&str>,
) -> Decision {
    let cmd = tool_input
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if cmd.is_empty() || should_skip_bash_command(&cmd) {
        return Decision::Skip(SkipReason::BashSkipped);
    }

    // exit_code || exitCode || returnCode (nullish coalesce do .cjs).
    let exit = tool_response
        .get("exit_code")
        .filter(|v| !v.is_null())
        .or_else(|| tool_response.get("exitCode").filter(|v| !v.is_null()))
        .or_else(|| tool_response.get("returnCode").filter(|v| !v.is_null()))
        .cloned();

    // Ordem: type, session_id, cmd, exit, ts, agent_id?, output_summary?.
    let mut obs = Map::new();
    obs.insert("type".into(), json!("bash"));
    obs.insert("session_id".into(), session_id.clone());
    // `.slice(0, 200)` do JS é por code unit UTF-16. Approximação byte-aware:
    // usamos chars (code points) — para ASCII/português normal, bate 1:1.
    obs.insert("cmd".into(), json!(truncate_chars(&cmd, 200)));
    // exit pode estar ausente — nesse caso JS grava `undefined` que o
    // JSON.stringify omite. Replicamos NÃO inserindo o campo.
    if let Some(exit) = exit {
        obs.insert("exit".into(), exit);
    } else {
        // Divergência: `.cjs` insere `exit: undefined` → `JSON.stringify`
        // remove undefined de objetos mas só se for *exatamente* undefined.
        // Se o caller passa `null` explícito o stringify mantém "exit":null.
        // Nosso input: se os 3 campos estão todos null/ausentes, não temos
        // como distinguir "undefined" de "null" — o .cjs grava a chave
        // mesmo assim como `undefined` → SUMIDA. Então skip do field é
        // o comportamento correto.
    }
    obs.insert("ts".into(), json!(ts));
    if let Some(a) = agent_id {
        obs.insert("agent_id".into(), json!(a));
    }
    if let Some(summary) = summarize_output(&cmd, tool_response) {
        obs.insert("output_summary".into(), json!(summary));
    }

    Decision::Record(Observation {
        value: Value::Object(obs),
        kind: ObsKind::Bash,
        rel_file: None,
    })
}

fn build_agent_call(
    session_id: &Value,
    tool_input: &Value,
    ts: &str,
    agent_id: Option<&str>,
) -> Decision {
    let agent = tool_input
        .get("subagent_type")
        .and_then(Value::as_str)
        .unwrap_or("general-purpose")
        .to_lowercase();
    let desc = tool_input
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let prompt = tool_input
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or("");

    let mut obs = Map::new();
    obs.insert("type".into(), json!("agent_call"));
    obs.insert("session_id".into(), session_id.clone());
    obs.insert("agent".into(), json!(agent));
    obs.insert("description".into(), json!(truncate_chars(desc, 120)));
    obs.insert("prompt".into(), json!(truncate_chars(prompt, 200)));
    obs.insert("ts".into(), json!(ts));
    if let Some(a) = agent_id {
        obs.insert("agent_id".into(), json!(a));
    }

    Decision::Record(Observation {
        value: Value::Object(obs),
        kind: ObsKind::AgentCall,
        rel_file: None,
    })
}

// ─── Pure helpers ────────────────────────────────────────────────────

/// `true` se o comando bash bate `SKIP_CMDS` do `.cjs` (ls/pwd/echo/etc.).
pub fn should_skip_bash_command(cmd: &str) -> bool {
    skip_re().is_match(cmd)
}

/// `Boolean(v)` do JS aplicado a `Option<&Value>`. `None` e valores
/// falsy viram `false`.
fn is_truthy(v: Option<&Value>) -> bool {
    match v {
        None => false,
        Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0 && !f.is_nan()).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

/// `str.slice(0, n)` do JS. JS conta code units UTF-16; aqui contamos
/// code points Rust para aproximar. Não é byte-exato pra surrogates
/// (emoji BMP) mas para CLI outputs reais em PT/EN é idêntico.
fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Replica `summarizeOutput(cmd, tool_response)` do `.cjs`.
///
/// Concatena `stdout || output || content || stderr || result` (em
/// ordem), filtra vazios, pega os últimos 4000 chars, e tenta extrair
/// um resumo curto segundo o comando. Se nenhum parser específico
/// bate, devolve a última linha não-vazia (se < 180 chars).
pub fn summarize_output(cmd: &str, tool_response: &Value) -> Option<String> {
    let raw = [
        tool_response.get("stdout"),
        tool_response.get("output"),
        tool_response.get("content"),
        tool_response.get("stderr"),
        tool_response.get("result"),
    ]
    .into_iter()
    .filter_map(|v| v.and_then(Value::as_str))
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join("\n");

    if raw.is_empty() {
        return None;
    }
    // `out.slice(-4000)` — último 4000 chars.
    let out: String = {
        let chars: Vec<char> = raw.chars().collect();
        let start = chars.len().saturating_sub(4000);
        chars[start..].iter().collect()
    };
    let c = cmd.to_lowercase();

    if contains_whole_word(&c, "pytest") {
        if let Some(cap) =
            Regex::new(r"(\d+)\s+passed(?:,\s*(\d+)\s+failed)?(?:,\s*(\d+)\s+error)?")
                .unwrap()
                .captures(&out)
        {
            let passed = cap.get(1).map(|m| m.as_str()).unwrap_or("0");
            let failed = cap
                .get(2)
                .map(|m| format!(", {} failed", m.as_str()))
                .unwrap_or_default();
            let errors = cap
                .get(3)
                .map(|m| format!(", {} errors", m.as_str()))
                .unwrap_or_default();
            return Some(format!("pytest: {passed} passed{failed}{errors}"));
        }
    }
    if Regex::new(r"(npm|yarn|pnpm|bun)\s+(test|run\s+test)|\bjest\b|\bvitest\b")
        .unwrap()
        .is_match(&c)
    {
        if let Some(cap) = Regex::new(r"(?i)Tests?:?\s+(?:(\d+)\s+failed,\s*)?(\d+)\s+passed")
            .unwrap()
            .captures(&out)
        {
            let failed = cap
                .get(1)
                .map(|m| format!(", {} failed", m.as_str()))
                .unwrap_or_default();
            let passed = cap.get(2).map(|m| m.as_str()).unwrap_or("0");
            return Some(format!("tests: {passed} passed{failed}"));
        }
    }
    if Regex::new(r"\bcargo\s+(test|nextest)\b")
        .unwrap()
        .is_match(&c)
    {
        if let Some(cap) = Regex::new(r"(?i)(\d+)\s+passed[;,]\s*(\d+)\s+failed")
            .unwrap()
            .captures(&out)
        {
            let passed = cap.get(1).map(|m| m.as_str()).unwrap_or("0");
            let failed = cap.get(2).map(|m| m.as_str()).unwrap_or("0");
            return Some(format!("cargo: {passed} passed, {failed} failed"));
        }
    }
    if contains_whole_word(&c, "ruff") {
        if Regex::new(r"(?i)all checks passed").unwrap().is_match(&out) {
            return Some("ruff: ok".into());
        }
        if let Some(cap) = Regex::new(r"(?i)found\s+(\d+)\s+error")
            .unwrap()
            .captures(&out)
        {
            return Some(format!("ruff: {} errors", cap.get(1).unwrap().as_str()));
        }
    }
    if contains_whole_word(&c, "mypy") {
        if Regex::new(r"(?i)success:\s*no issues")
            .unwrap()
            .is_match(&out)
        {
            return Some("mypy: ok".into());
        }
        if let Some(cap) = Regex::new(r"(?i)found\s+(\d+)\s+error")
            .unwrap()
            .captures(&out)
        {
            return Some(format!("mypy: {} errors", cap.get(1).unwrap().as_str()));
        }
    }
    if Regex::new(r"\btsc\b|\btypecheck\b|type-check")
        .unwrap()
        .is_match(&c)
    {
        let errors = Regex::new(r"error\s+TS\d+")
            .unwrap()
            .find_iter(&out)
            .count();
        if errors > 0 {
            return Some(format!("tsc: {errors} errors"));
        }
        if Regex::new(r"(?i)\b0\s+errors?\b|no errors? found")
            .unwrap()
            .is_match(&out)
        {
            return Some("tsc: ok".into());
        }
    }
    if contains_whole_word(&c, "eslint") {
        if let Some(cap) =
            Regex::new(r"(?i)(\d+)\s+problems?\s*\((\d+)\s+errors?,\s*(\d+)\s+warnings?\)")
                .unwrap()
                .captures(&out)
        {
            let err = cap.get(2).unwrap().as_str();
            let warn = cap.get(3).unwrap().as_str();
            return Some(format!("eslint: {err} err, {warn} warn"));
        }
        if Regex::new(r"^\s*$").unwrap().is_match(&out) {
            return Some("eslint: ok".into());
        }
    }

    // Fallback: última linha não-vazia, < 180 chars.
    let last = out
        .split('\n')
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .next_back();
    if let Some(last) = last {
        if last.len() < 180 {
            return Some(last.to_string());
        }
    }
    None
}

fn contains_whole_word(haystack: &str, word: &str) -> bool {
    // Paridade com `/\bX\b/.test(...)`. Regex::new evita realocar é trivial aqui.
    Regex::new(&format!(r"\b{}\b", regex::escape(word)))
        .unwrap()
        .is_match(haystack)
}

// ─── Efeitos (git) ───────────────────────────────────────────────────

/// Resultado de [`capture_diff`]. Replica o objeto que o `.cjs` grava
/// em `obs.diff`:
/// - `{tracked: true, binary: true}` — arquivo tracked mas numstat
///   retornou `-`/`-` (binário).
/// - `{tracked: true, ins, del, preview}` — diff regular.
/// - `{tracked: false, ins: lines, del: 0}` — untracked: todas as
///   linhas contam como insert.
/// - `None` — git indisponível ou arquivo não existe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diff {
    TrackedBinary,
    TrackedRegular {
        ins: u64,
        del: u64,
        preview: Option<String>,
    },
    Untracked {
        ins: u64,
        del: u64,
    },
}

impl Diff {
    /// Serializa no mesmo formato que o `.cjs` grava em `obs.diff`.
    pub fn to_value(&self) -> Value {
        match self {
            Diff::TrackedBinary => json!({"tracked": true, "binary": true}),
            Diff::TrackedRegular { ins, del, preview } => {
                let mut m = Map::new();
                m.insert("tracked".into(), json!(true));
                m.insert("ins".into(), json!(ins));
                m.insert("del".into(), json!(del));
                m.insert(
                    "preview".into(),
                    preview.clone().map(Value::String).unwrap_or(Value::Null),
                );
                Value::Object(m)
            }
            Diff::Untracked { ins, del } => {
                let mut m = Map::new();
                m.insert("tracked".into(), json!(false));
                m.insert("ins".into(), json!(ins));
                m.insert("del".into(), json!(del));
                Value::Object(m)
            }
        }
    }
}

/// Spawna `git diff HEAD --numstat -- <relFile>` no `project_root`.
///
/// Porta de `captureDiff` do `.cjs`. Timeout fixo de 1500ms idêntico.
/// Retorna `None` quando git falha, arquivo não existe ou algo dá
/// errado — replica o `try { ... } catch { return null }`.
pub fn capture_diff(project_root: &Path, rel_file: &str) -> Option<Diff> {
    let numstat = run_git_timeout(
        project_root,
        &["diff", "HEAD", "--numstat", "--", rel_file],
        1500,
    )?;
    let numstat = numstat.trim();

    if !numstat.is_empty() {
        // numstat format: "INS\tDEL\tpath" ou "-\t-\tpath" para binário.
        let parts: Vec<&str> = numstat.split_whitespace().collect();
        if parts.len() < 2 {
            return None;
        }
        let ins = parts[0].parse::<u64>();
        let del = parts[1].parse::<u64>();
        match (ins, del) {
            (Ok(ins), Ok(del)) => Some(Diff::TrackedRegular {
                ins,
                del,
                preview: diff_preview(project_root, rel_file),
            }),
            _ => Some(Diff::TrackedBinary),
        }
    } else {
        // Sem diff contra HEAD: untracked OU idêntico ao HEAD.
        let abs = project_root.join(rel_file);
        if !abs.exists() {
            return None;
        }
        let content = std::fs::read_to_string(&abs).ok()?;
        let lines = if content.is_empty() {
            0
        } else {
            content.split('\n').count() as u64
        };
        Some(Diff::Untracked { ins: lines, del: 0 })
    }
}

fn diff_preview(project_root: &Path, rel_file: &str) -> Option<String> {
    let diff = run_git_timeout(
        project_root,
        &["diff", "HEAD", "--unified=0", "--", rel_file],
        1500,
    )?;
    if diff.is_empty() {
        return None;
    }
    let body: String = diff
        .split('\n')
        .filter(|l| {
            (l.starts_with('+') || l.starts_with('-'))
                && !l.starts_with("+++")
                && !l.starts_with("---")
        })
        .collect::<Vec<_>>()
        .join("\n");
    if body.is_empty() {
        return None;
    }
    let chars: Vec<char> = body.chars().collect();
    if chars.len() > DIFF_PREVIEW_MAX {
        let sliced: String = chars[..DIFF_PREVIEW_MAX].iter().collect();
        Some(format!("{sliced}…"))
    } else {
        Some(body)
    }
}

fn run_git_timeout(cwd: &Path, args: &[&str], timeout_ms: u64) -> Option<String> {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let out = child.wait_with_output().ok()?;
                return Some(String::from_utf8_lossy(&out.stdout).to_string());
            }
            Ok(Some(_)) => return None,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(_) => return None,
        }
    }
}

/// Enriquece uma observação `file_edit` com o campo `diff` capturado
/// pelo helper. Operação in-place para preservar ordem de chaves
/// (inserted no fim, depois de agent_id).
pub fn augment_file_edit_with_diff(obs: &mut Value, diff: &Diff) {
    if let Some(m) = obs.as_object_mut() {
        m.insert("diff".to_string(), diff.to_value());
    }
}

// Type-check helper: mantém `PathBuf` em público — senão clippy resmunga
// que importado sem uso quando tests estão desligados.
#[allow(dead_code)]
fn _keep_pathbuf(_p: &PathBuf) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pr() -> PathBuf {
        PathBuf::from("/project")
    }

    fn ts() -> &'static str {
        "2026-04-19T10:00:00.000Z"
    }

    // ─── decide_observation ──────────────────────────────────────────

    #[test]
    fn skip_when_no_tool_name() {
        let input = json!({"session_id": "s1"});
        assert_eq!(
            decide_observation(&input, &pr(), ts(), None),
            Decision::Skip(SkipReason::NoToolName)
        );
    }

    #[test]
    fn skip_when_irrelevant_tool() {
        let input = json!({"tool_name": "Read", "session_id": "s1"});
        assert_eq!(
            decide_observation(&input, &pr(), ts(), None),
            Decision::Skip(SkipReason::IrrelevantTool)
        );
    }

    #[test]
    fn skip_on_tool_error_flag() {
        let input = json!({
            "tool_name": "Bash",
            "session_id": "s1",
            "tool_input": {"command": "ls"},
            "tool_response": {"is_error": true}
        });
        assert_eq!(
            decide_observation(&input, &pr(), ts(), None),
            Decision::Skip(SkipReason::ToolError)
        );
    }

    #[test]
    fn skip_on_tool_error_message() {
        let input = json!({
            "tool_name": "Write",
            "session_id": "s1",
            "tool_input": {"file_path": "/x/a.py"},
            "tool_response": {"error": "disk full"}
        });
        assert_eq!(
            decide_observation(&input, &pr(), ts(), None),
            Decision::Skip(SkipReason::ToolError)
        );
    }

    // ─── file_edit ──────────────────────────────────────────────────

    #[test]
    fn file_edit_uses_relative_path_when_inside_project() {
        let input = json!({
            "tool_name": "Write",
            "session_id": "s1",
            "tool_input": {"file_path": "/project/src/a.py"}
        });
        let d = decide_observation(&input, &pr(), ts(), None);
        let Decision::Record(Observation {
            value: v,
            kind,
            rel_file,
        }) = d
        else {
            panic!("esperava Record, veio {:?}", d);
        };
        assert_eq!(kind, ObsKind::FileEdit);
        assert_eq!(rel_file.as_deref(), Some("src/a.py"));
        assert_eq!(v["type"], "file_edit");
        assert_eq!(v["file"], "src/a.py");
        assert_eq!(v["ts"], ts());
    }

    #[test]
    fn file_edit_keeps_absolute_path_when_outside_project() {
        let input = json!({
            "tool_name": "Edit",
            "session_id": "s1",
            "tool_input": {"file_path": "/other/x.py"}
        });
        let Decision::Record(o) = decide_observation(&input, &pr(), ts(), None) else {
            panic!("esperava Record");
        };
        assert_eq!(o.value["file"], "/other/x.py");
    }

    #[test]
    fn file_edit_accepts_notebook_path() {
        let input = json!({
            "tool_name": "MultiEdit",
            "session_id": "s1",
            "tool_input": {"notebook_path": "/project/nb.ipynb"}
        });
        let Decision::Record(o) = decide_observation(&input, &pr(), ts(), None) else {
            panic!("esperava Record");
        };
        assert_eq!(o.value["file"], "nb.ipynb");
    }

    #[test]
    fn file_edit_without_path_skips() {
        let input = json!({
            "tool_name": "Write",
            "session_id": "s1",
            "tool_input": {}
        });
        assert_eq!(
            decide_observation(&input, &pr(), ts(), None),
            Decision::Skip(SkipReason::NoFilePath)
        );
    }

    #[test]
    fn file_edit_attaches_agent_id_when_provided() {
        let input = json!({
            "tool_name": "Write",
            "session_id": "s1",
            "tool_input": {"file_path": "/project/a"}
        });
        let Decision::Record(o) = decide_observation(&input, &pr(), ts(), Some("mgr")) else {
            panic!("esperava Record");
        };
        assert_eq!(o.value["agent_id"], "mgr");
    }

    // ─── bash ───────────────────────────────────────────────────────

    #[test]
    fn bash_records_cmd_with_exit_code() {
        let input = json!({
            "tool_name": "Bash",
            "session_id": "s1",
            "tool_input": {"command": "cargo test"},
            "tool_response": {"exit_code": 0}
        });
        let Decision::Record(o) = decide_observation(&input, &pr(), ts(), None) else {
            panic!("esperava Record");
        };
        assert_eq!(o.kind, ObsKind::Bash);
        assert_eq!(o.value["cmd"], "cargo test");
        assert_eq!(o.value["exit"], 0);
    }

    #[test]
    fn bash_accepts_exit_code_or_camel_or_return_code() {
        for key in &["exit_code", "exitCode", "returnCode"] {
            let input = json!({
                "tool_name": "Bash",
                "session_id": "s1",
                "tool_input": {"command": "cargo test"},
                "tool_response": { *key: 7 }
            });
            let Decision::Record(o) = decide_observation(&input, &pr(), ts(), None) else {
                panic!("esperava Record para {key}");
            };
            assert_eq!(o.value["exit"], 7, "key {key}");
        }
    }

    #[test]
    fn bash_skips_trivial_commands() {
        for c in [
            "ls", "ls -la", "pwd", "echo oi", "cat f", "sleep 1", "clear",
        ] {
            let input = json!({
                "tool_name": "Bash",
                "session_id": "s1",
                "tool_input": {"command": c}
            });
            assert_eq!(
                decide_observation(&input, &pr(), ts(), None),
                Decision::Skip(SkipReason::BashSkipped),
                "cmd={c}"
            );
        }
    }

    #[test]
    fn bash_skips_empty_command() {
        let input = json!({
            "tool_name": "Bash",
            "session_id": "s1",
            "tool_input": {"command": "   "}
        });
        assert_eq!(
            decide_observation(&input, &pr(), ts(), None),
            Decision::Skip(SkipReason::BashSkipped)
        );
    }

    #[test]
    fn bash_truncates_cmd_at_200_chars() {
        let cmd = "a".repeat(250);
        let input = json!({
            "tool_name": "Bash",
            "session_id": "s1",
            "tool_input": {"command": cmd}
        });
        let Decision::Record(o) = decide_observation(&input, &pr(), ts(), None) else {
            panic!("esperava Record");
        };
        assert_eq!(o.value["cmd"].as_str().unwrap().len(), 200);
    }

    #[test]
    fn bash_omits_exit_when_no_return_code_present() {
        // Sem exit_code/exitCode/returnCode no tool_response → campo ausente.
        let input = json!({
            "tool_name": "Bash",
            "session_id": "s1",
            "tool_input": {"command": "cargo test"},
            "tool_response": {}
        });
        let Decision::Record(o) = decide_observation(&input, &pr(), ts(), None) else {
            panic!("esperava Record");
        };
        assert!(o.value.get("exit").is_none(), "exit deve ser omitido");
    }

    #[test]
    fn bash_records_output_summary_for_pytest() {
        let input = json!({
            "tool_name": "Bash",
            "session_id": "s1",
            "tool_input": {"command": "pytest tests/"},
            "tool_response": {
                "exit_code": 0,
                "stdout": "== 42 passed in 1.2s =="
            }
        });
        let Decision::Record(o) = decide_observation(&input, &pr(), ts(), None) else {
            panic!("esperava Record");
        };
        assert_eq!(o.value["output_summary"], "pytest: 42 passed");
    }

    // ─── agent_call ─────────────────────────────────────────────────

    #[test]
    fn agent_call_from_task() {
        let input = json!({
            "tool_name": "Task",
            "session_id": "s1",
            "tool_input": {"subagent_type": "Explore", "description": "busca x", "prompt": "roda y"}
        });
        let Decision::Record(o) = decide_observation(&input, &pr(), ts(), None) else {
            panic!("esperava Record");
        };
        assert_eq!(o.kind, ObsKind::AgentCall);
        assert_eq!(o.value["agent"], "explore"); // lowercase
        assert_eq!(o.value["description"], "busca x");
    }

    #[test]
    fn agent_call_default_type() {
        let input = json!({
            "tool_name": "Agent",
            "session_id": "s1",
            "tool_input": {}
        });
        let Decision::Record(o) = decide_observation(&input, &pr(), ts(), None) else {
            panic!("esperava Record");
        };
        assert_eq!(o.value["agent"], "general-purpose");
        assert_eq!(o.value["description"], "");
        assert_eq!(o.value["prompt"], "");
    }

    #[test]
    fn agent_call_truncates_description_and_prompt() {
        let input = json!({
            "tool_name": "Task",
            "session_id": "s1",
            "tool_input": {
                "description": "a".repeat(200),
                "prompt": "b".repeat(300)
            }
        });
        let Decision::Record(o) = decide_observation(&input, &pr(), ts(), None) else {
            panic!("esperava Record");
        };
        assert_eq!(o.value["description"].as_str().unwrap().len(), 120);
        assert_eq!(o.value["prompt"].as_str().unwrap().len(), 200);
    }

    // ─── summarize_output ───────────────────────────────────────────

    #[test]
    fn summarize_pytest_with_failures() {
        let resp = json!({"stdout": "3 passed, 1 failed in 0.5s"});
        assert_eq!(
            summarize_output("pytest x", &resp).as_deref(),
            Some("pytest: 3 passed, 1 failed")
        );
    }

    #[test]
    fn summarize_cargo_test() {
        let resp = json!({"stdout": "test result: ok. 10 passed; 2 failed"});
        assert_eq!(
            summarize_output("cargo test", &resp).as_deref(),
            Some("cargo: 10 passed, 2 failed")
        );
    }

    #[test]
    fn summarize_ruff_ok() {
        let resp = json!({"stdout": "All checks passed!"});
        assert_eq!(
            summarize_output("ruff check", &resp).as_deref(),
            Some("ruff: ok")
        );
    }

    #[test]
    fn summarize_ruff_errors() {
        let resp = json!({"stdout": "Found 7 errors"});
        assert_eq!(
            summarize_output("ruff check", &resp).as_deref(),
            Some("ruff: 7 errors")
        );
    }

    #[test]
    fn summarize_mypy_ok() {
        let resp = json!({"stdout": "Success: no issues found"});
        assert_eq!(
            summarize_output("mypy .", &resp).as_deref(),
            Some("mypy: ok")
        );
    }

    #[test]
    fn summarize_tsc_errors() {
        let resp = json!({"stdout": "src/a.ts(1,1): error TS2305\nsrc/b.ts(2,2): error TS2306"});
        assert_eq!(
            summarize_output("tsc --noEmit", &resp).as_deref(),
            Some("tsc: 2 errors")
        );
    }

    #[test]
    fn summarize_falls_back_to_last_line() {
        let resp = json!({"stdout": "line one\nline two\nfinal state"});
        assert_eq!(
            summarize_output("custom cmd", &resp).as_deref(),
            Some("final state")
        );
    }

    #[test]
    fn summarize_returns_none_when_empty() {
        let resp = json!({});
        assert!(summarize_output("any", &resp).is_none());
    }

    // ─── skip regex ─────────────────────────────────────────────────

    #[test]
    fn skip_re_matches_boundary_commands() {
        assert!(should_skip_bash_command("ls"));
        assert!(should_skip_bash_command("ls -la"));
        assert!(should_skip_bash_command("pwd"));
        assert!(should_skip_bash_command("echo hi"));
        assert!(should_skip_bash_command("clear"));
    }

    #[test]
    fn skip_re_does_not_match_similar_commands() {
        // Command começa com "ls" mas tem mais letras — NÃO skip.
        assert!(!should_skip_bash_command("lsof"));
        assert!(!should_skip_bash_command("echopipe"));
        // Comandos reais que não devem ser skipados.
        assert!(!should_skip_bash_command("cargo test"));
        assert!(!should_skip_bash_command("grep foo"));
    }

    // ─── diff formatter ─────────────────────────────────────────────

    #[test]
    fn diff_tracked_binary_serialization() {
        let v = Diff::TrackedBinary.to_value();
        assert_eq!(v, json!({"tracked": true, "binary": true}));
    }

    #[test]
    fn diff_tracked_regular_serialization() {
        let v = Diff::TrackedRegular {
            ins: 3,
            del: 1,
            preview: Some("+a\n-b".into()),
        }
        .to_value();
        assert_eq!(
            v,
            json!({"tracked": true, "ins": 3, "del": 1, "preview": "+a\n-b"})
        );
    }

    #[test]
    fn diff_tracked_regular_with_null_preview() {
        let v = Diff::TrackedRegular {
            ins: 3,
            del: 1,
            preview: None,
        }
        .to_value();
        assert_eq!(
            v,
            json!({"tracked": true, "ins": 3, "del": 1, "preview": null})
        );
    }

    #[test]
    fn diff_untracked_serialization() {
        let v = Diff::Untracked { ins: 10, del: 0 }.to_value();
        assert_eq!(v, json!({"tracked": false, "ins": 10, "del": 0}));
    }
}
