//! Lógica do hook `UserPromptSubmit` — porta de
//! `plugin/scripts/session-init.cjs`.
//!
//! Divisão em camadas:
//!
//! | Função                          | Responsabilidade                                   |
//! |---------------------------------|----------------------------------------------------|
//! | [`is_correction_marker`]        | Regex do detector de correções (roda em todo turno).|
//! | [`compact_obs`]                 | Comprime `last_obs` no registro de correction.      |
//! | [`build_correction_record`]     | Monta a entrada em `corrections.jsonl`.            |
//! | [`format_obs`]                  | Renderiza uma observação como linha markdown.      |
//! | [`render_md_block`]             | Remove `# title` do head de um `.md` e prepende `title`.|
//! | [`format_git_block`]            | Renderiza `GitContext` como lista de linhas md.   |
//! | [`format_adrs_block`]           | Idem para ADRs.                                   |
//! | [`format_sessions_block`]       | Idem para últimas sessões.                         |
//! | [`format_corrections_block`]    | Idem para correções.                               |
//! | [`format_decisions_log_block`]  | Idem para decisões log bruto.                     |
//! | [`format_gotchas_log_block`]    | Idem para gotchas log.                            |
//! | [`format_observations_block`]   | Idem para observações recentes.                   |
//! | [`apply_token_budget`]          | Orçamento priority-aware.                          |
//! | [`render_final_context`]        | Junta blocos + rodapé de telemetria.              |
//! | [`onboarding_message`]          | Texto estático do "novo projeto detectado".       |
//!
//! Efeitos de disco/git moram no binário (`src/bin/session_init.rs`).

use serde_json::{json, Map, Value};
use std::sync::LazyLock;

/// Regex dos marcadores de correção — mesma literal do `.cjs`:
/// `/^(\s*)(não\b|nao\b|pare\b|espera\b|calma\b|errado\b|incorret[oa]\b|
/// volta\b|desfaz\b|stop\b|wait\b|don'?t\b|no[,!.\s]|wrong\b|incorrect\b|
/// undo\b|revert\b)/i`
///
/// O `.cjs` usa `no[,!.\s]` (qualquer um de `,`, `!`, `.`, espaço) depois
/// de "no"; `don'?t` aceita "don't" e "dont". `\b` antes dos demais
/// marca word boundary.
static CORRECTION_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)^(\s*)(não\b|nao\b|pare\b|espera\b|calma\b|errado\b|incorret[oa]\b|volta\b|desfaz\b|stop\b|wait\b|don'?t\b|no[,!.\s]|wrong\b|incorrect\b|undo\b|revert\b)",
    )
    .expect("regex correction markers válida")
});

pub fn is_correction_marker(user_prompt: &str) -> bool {
    if user_prompt.is_empty() || user_prompt.len() > 600 {
        return false;
    }
    CORRECTION_RE.is_match(user_prompt)
}

/// Comprime uma observação para guardar como `previous_action` num
/// registro de correction. Replica `compactObs` do `.cjs`.
pub fn compact_obs(o: &Value) -> Value {
    match o.get("type").and_then(Value::as_str) {
        Some("file_edit") => json!({
            "type": "file_edit",
            "file": o.get("file").cloned().unwrap_or(Value::Null)
        }),
        Some("bash") => {
            let cmd = o.get("cmd").and_then(Value::as_str).unwrap_or("");
            json!({
                "type": "bash",
                "cmd": chars_take(cmd, 120)
            })
        }
        Some(t) => json!({ "type": t }),
        None => json!({ "type": Value::Null }),
    }
}

pub fn build_correction_record(
    session_id: Option<&str>,
    user_prompt: &str,
    previous: Option<&Value>,
    ts: &str,
) -> Value {
    let mut m = Map::new();
    m.insert(
        "session_id".into(),
        session_id.map(|s| json!(s)).unwrap_or(Value::Null),
    );
    m.insert("prompt".into(), json!(chars_take(user_prompt, 500)));
    m.insert(
        "previous_action".into(),
        match previous {
            Some(p) => compact_obs(p),
            None => Value::Null,
        },
    );
    m.insert("ts".into(), json!(ts));
    Value::Object(m)
}

/// Onboarding estático. Replica string literal do `.cjs:94-103`.
pub fn onboarding_message() -> &'static str {
    "## 🆕 Novo projeto detectado pelo project-mem\n\nEste é o primeiro uso do **project-mem** neste projeto. Antes de responder ao pedido do usuário:\n\n1. Analise a estrutura do projeto (arquivos raiz, manifesto de dependências, pastas principais, 2-3 arquivos de código).\n2. Apresente um resumo: stack, arquitetura, convenções identificadas, como rodar/testar.\n3. Pergunte ao desenvolvedor: **\"Há convenções, restrições ou contexto importante que devo saber sobre este projeto?\"**\n4. Com base na análise + resposta do dev, crie o arquivo `.claude-project-memory/PROJECT.md` usando o skill `/mem-setup` como referência de estrutura.\n\n> Após criar o PROJECT.md, ele será injetado automaticamente em todas as sessões futuras."
}

/// Bloco: lê conteúdo cru de um `.md`, strip a primeira linha `# ...`,
/// trim, e prepende `title` como primeira linha. Retorna `None` se
/// vazio.
pub fn render_md_block(title: &str, content: &str) -> Option<Vec<String>> {
    let trimmed = content.trim();
    // Remove primeira linha `# ...\n` se existir.
    let head_re = regex::Regex::new(r"(?m)^#\s+.*\n").unwrap();
    let stripped = head_re.replacen(trimmed, 1, "").trim().to_string();
    if stripped.is_empty() {
        return None;
    }
    Some(vec![title.to_string(), stripped])
}

// ── Git context ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentCommit {
    pub sha: String,
    pub msg: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitContext {
    pub branch: String,
    pub modified: Vec<String>,
    pub recent_commits: Vec<RecentCommit>,
}

/// Renderiza `GitContext` como bloco markdown. Vazio → retorna vec vazio.
pub fn format_git_block(git: &GitContext) -> Vec<String> {
    let mut out = Vec::new();
    out.push("## Estado do git".to_string());
    out.push(format!("- Branch: `{}`", git.branch));
    if !git.modified.is_empty() {
        let shown: Vec<String> = git
            .modified
            .iter()
            .take(10)
            .map(|f| format!("`{f}`"))
            .collect();
        let more = if git.modified.len() > 10 {
            format!(" (+{})", git.modified.len() - 10)
        } else {
            String::new()
        };
        out.push(format!("- Modificados: {}{}", shown.join(", "), more));
    }
    if !git.recent_commits.is_empty() {
        out.push("- Commits recentes:".to_string());
        for c in &git.recent_commits {
            out.push(format!("  - `{}` {}", c.sha, c.msg));
        }
    }
    out
}

// ── ADRs ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdrEntry {
    pub file: String,
    pub title: String,
}

pub fn format_adrs_block(adrs: &[AdrEntry]) -> Vec<String> {
    let mut out = Vec::new();
    out.push("## ADRs do repositório".to_string());
    for adr in adrs {
        out.push(format!("- `{}` — {}", adr.file, adr.title));
    }
    out
}

/// Extrai o título de um ADR: primeira linha `# <texto>` do markdown.
/// Fallback: nome do arquivo sem `.md`. Trunca em 100 chars.
pub fn adr_title_from_content(file_name: &str, content: &str) -> String {
    let re = regex::Regex::new(r"(?m)^#\s+(.+)$").unwrap();
    let title = re
        .captures(content)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_else(|| {
            file_name
                .strip_suffix(".md")
                .unwrap_or(file_name)
                .to_string()
        });
    chars_take(&title, 100)
}

// ── Blocos de listas (sessões, correções, decisões, gotchas, obs) ────

/// Últimas 5 sessões com `type=end` + `summary`, ordem reversa.
pub fn format_sessions_block(sessions: &[Value]) -> Vec<String> {
    let ended: Vec<&Value> = sessions
        .iter()
        .filter(|s| s.get("type") == Some(&json!("end")) && s.get("summary").is_some())
        .collect();
    let tail: Vec<&&Value> = ended.iter().rev().take(5).collect();
    if tail.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    out.push("## Sessões recentes".to_string());
    for s in tail {
        let date = s
            .get("ended_at")
            .and_then(Value::as_str)
            .or_else(|| s.get("started_at").and_then(Value::as_str))
            .map(|x| chars_take(x, 10))
            .unwrap_or_default();
        let kind = match s.get("kind").and_then(Value::as_str) {
            Some(k) if !k.is_empty() => format!(" _[{k}]_"),
            _ => String::new(),
        };
        let summary = s.get("summary").and_then(Value::as_str).unwrap_or("");
        out.push(format!("- **{date}**{kind}: {summary}"));
    }
    out
}

/// Últimas 8 correções, reverse.
pub fn format_corrections_block(corrections: &[Value]) -> Vec<String> {
    let tail: Vec<&Value> = corrections.iter().rev().take(8).collect();
    if tail.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    out.push("## Correções do dev (não repetir)".to_string());
    for c in tail {
        let date = c
            .get("ts")
            .and_then(Value::as_str)
            .map(|s| chars_take(s, 10))
            .unwrap_or_default();
        let prev = c.get("previous_action").cloned().unwrap_or(Value::Null);
        let prev_str = match prev.get("type").and_then(Value::as_str) {
            Some("file_edit") => {
                let file = prev.get("file").and_then(Value::as_str).unwrap_or("");
                format!(" (após editar {file})")
            }
            Some("bash") => {
                let cmd = prev.get("cmd").and_then(Value::as_str).unwrap_or("");
                format!(" (após rodar `{cmd}`)")
            }
            _ => String::new(),
        };
        let prompt = c
            .get("prompt")
            .and_then(Value::as_str)
            .map(|s| chars_take(s, 160))
            .unwrap_or_default();
        out.push(format!("- **{date}**{prev_str}: \"{prompt}\""));
    }
    out
}

pub fn format_decisions_log_block(decisions: &[Value]) -> Vec<String> {
    let tail: Vec<&Value> = decisions.iter().rev().take(5).collect();
    if tail.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    out.push("## Decisões recentes (log bruto)".to_string());
    for d in tail {
        let date = d
            .get("ts")
            .and_then(Value::as_str)
            .map(|s| chars_take(s, 10))
            .unwrap_or_default();
        let what = d
            .get("what")
            .and_then(Value::as_str)
            .or_else(|| d.get("text").and_then(Value::as_str))
            .unwrap_or("");
        let why = match d.get("why").and_then(Value::as_str) {
            Some(w) if !w.is_empty() => format!(" — _{w}_"),
            _ => String::new(),
        };
        out.push(format!("- **{date}**: {what}{why}"));
    }
    out
}

pub fn format_gotchas_log_block(gotchas: &[Value]) -> Vec<String> {
    let tail: Vec<&Value> = gotchas.iter().rev().take(5).collect();
    if tail.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    out.push("## Armadilhas detectadas".to_string());
    for g in tail {
        let date = g
            .get("ts")
            .and_then(Value::as_str)
            .map(|s| chars_take(s, 10))
            .unwrap_or_default();
        let description = g.get("description").and_then(Value::as_str).unwrap_or("");
        let file = match g.get("file").and_then(Value::as_str) {
            Some(f) if !f.is_empty() => format!(" — `{f}`"),
            _ => String::new(),
        };
        out.push(format!("- **{date}**: {description}{file}"));
    }
    out
}

pub fn format_observations_block(obs: &[Value], current_session: Option<&str>) -> Vec<String> {
    let filtered: Vec<&Value> = obs
        .iter()
        .filter(
            |o| match (current_session, o.get("session_id").and_then(Value::as_str)) {
                (Some(cur), Some(sid)) => sid != cur,
                _ => true,
            },
        )
        .collect();
    let tail: Vec<&&Value> = filtered.iter().rev().take(40).rev().collect();
    if tail.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    out.push("## Observações recentes".to_string());
    for o in tail {
        let ts = o
            .get("ts")
            .and_then(Value::as_str)
            .map(|s| chars_take(s, 16).replace('T', " "))
            .unwrap_or_default();
        out.push(format_obs(&ts, o));
    }
    out
}

pub fn format_obs(ts: &str, o: &Value) -> String {
    match o.get("type").and_then(Value::as_str) {
        Some("file_edit") => {
            let file = o.get("file").and_then(Value::as_str).unwrap_or("");
            let stat = match o.get("diff") {
                Some(d) if d.get("ins").and_then(Value::as_i64).is_some() => {
                    let ins = d.get("ins").and_then(Value::as_i64).unwrap_or(0);
                    let del = d.get("del").and_then(Value::as_i64).unwrap_or(0);
                    let novo = matches!(d.get("tracked"), Some(Value::Bool(false)));
                    format!(" (+{ins}/-{del}{})", if novo { " novo" } else { "" })
                }
                _ => String::new(),
            };
            format!("- `{ts}` EDIT `{file}`{stat}")
        }
        Some("bash") => {
            let cmd = o
                .get("cmd")
                .and_then(Value::as_str)
                .map(|s| chars_take(s, 120))
                .unwrap_or_default();
            let exit = match o.get("exit") {
                Some(v) if !v.is_null() => format!(" → exit {v}"),
                _ => String::new(),
            };
            let out_summary = match o.get("output_summary").and_then(Value::as_str) {
                Some(s) if !s.is_empty() => format!(" _{s}_"),
                _ => String::new(),
            };
            format!("- `{ts}` BASH `{cmd}`{exit}{out_summary}")
        }
        Some("agent_call") => {
            let agent = o.get("agent").and_then(Value::as_str).unwrap_or("");
            let desc = match o.get("description").and_then(Value::as_str) {
                Some(d) if !d.is_empty() => format!(" — {d}"),
                _ => String::new(),
            };
            format!("- `{ts}` AGENT `{agent}`{desc}")
        }
        Some("note") => {
            let content = o.get("content").and_then(Value::as_str).unwrap_or("");
            format!("- `{ts}` NOTE: {content}")
        }
        Some(t) => format!("- `{ts}` {t}"),
        None => format!("- `{ts}` "),
    }
}

// ── Orçamento de tokens ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub name: String,
    pub priority: i64,
    pub text: String,
    pub tokens: usize,
}

/// Ordena por priority asc (menor = mais importante) e inclui blocos
/// enquanto couberem no `budget`. Retorna `(included, skipped, total_tokens)`.
pub fn apply_token_budget(blocks: Vec<Block>, budget: usize) -> (Vec<Block>, Vec<Block>, usize) {
    let mut sorted = blocks;
    sorted.sort_by_key(|b| b.priority);
    let mut included = Vec::new();
    let mut skipped = Vec::new();
    let mut total: usize = 0;
    for b in sorted {
        if total + b.tokens <= budget {
            total += b.tokens;
            included.push(b);
        } else {
            skipped.push(b);
        }
    }
    (included, skipped, total)
}

/// Monta o markdown final com os `included` (separados por `\n\n---\n`)
/// + comentário de telemetria no fim.
pub fn render_final_context(
    included: &[Block],
    skipped: &[Block],
    total_tokens: usize,
    budget: usize,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (i, b) in included.iter().enumerate() {
        parts.push(b.text.clone());
        if i + 1 < included.len() {
            parts.push("\n\n---\n".to_string());
        }
    }
    if !skipped.is_empty() {
        let names: Vec<String> = skipped.iter().map(|b| b.name.clone()).collect();
        parts.push(format!(
            "\n\n<!-- project-mem: ~{total_tokens}/{budget}t · omitido: {} -->",
            names.join(", ")
        ));
    } else if !included.is_empty() {
        parts.push(format!(
            "\n\n<!-- project-mem: ~{total_tokens}/{budget}t -->"
        ));
    }
    parts.join("\n")
}

/// Monta o objeto JSON de output do hook com `additional_context`.
/// Paridade exata com o objeto emitido pelo `.cjs`.
pub fn render_hook_payload(additional_context: Option<&str>) -> String {
    let mut root = Map::new();
    root.insert("continue".into(), json!(true));
    root.insert("suppressOutput".into(), json!(true));
    if let Some(ctx) = additional_context {
        let mut inner = Map::new();
        inner.insert("hookEventName".into(), json!("UserPromptSubmit"));
        inner.insert("additionalContext".into(), json!(ctx));
        root.insert("hookSpecificOutput".into(), Value::Object(inner));
    }
    serde_json::to_string(&Value::Object(root)).expect("stringify ok")
}

pub fn render_session_start_record(session_id: Option<&str>, started_at: &str, cwd: &str) -> Value {
    let mut m = Map::new();
    m.insert("type".into(), json!("start"));
    m.insert(
        "session_id".into(),
        session_id.map(|s| json!(s)).unwrap_or(Value::Null),
    );
    m.insert("started_at".into(), json!(started_at));
    m.insert("cwd".into(), json!(cwd));
    Value::Object(m)
}

// ── Helpers internos ─────────────────────────────────────────────────

fn chars_take(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Correction marker regex ──────────────────────────────────────

    #[test]
    fn correction_detects_nao() {
        assert!(is_correction_marker("não, faz outra coisa"));
        assert!(is_correction_marker("nao faz isso"));
    }

    #[test]
    fn correction_detects_stop_wait_undo() {
        assert!(is_correction_marker("stop that"));
        assert!(is_correction_marker("wait, wrong"));
        assert!(is_correction_marker("undo please"));
        assert!(is_correction_marker("revert this"));
    }

    #[test]
    fn correction_detects_dont_variants() {
        assert!(is_correction_marker("don't do it"));
        assert!(is_correction_marker("dont do that"));
    }

    #[test]
    fn correction_detects_no_with_punctuation_or_space() {
        assert!(is_correction_marker("no, that's wrong"));
        assert!(is_correction_marker("no! don't"));
        assert!(is_correction_marker("no that"));
    }

    #[test]
    fn correction_rejects_long_prompts() {
        let long = "não ".repeat(200); // > 600 chars
        assert!(!is_correction_marker(&long));
    }

    #[test]
    fn correction_rejects_empty() {
        assert!(!is_correction_marker(""));
    }

    #[test]
    fn correction_rejects_normal_prompt() {
        assert!(!is_correction_marker("Crie um componente de botão"));
        assert!(!is_correction_marker("Please add a test"));
    }

    #[test]
    fn correction_accepts_leading_whitespace() {
        assert!(is_correction_marker("   não"));
    }

    // ── compact_obs ──────────────────────────────────────────────────

    #[test]
    fn compact_file_edit() {
        let o = json!({"type": "file_edit", "file": "a.rs", "ts": "x"});
        assert_eq!(
            compact_obs(&o),
            json!({"type": "file_edit", "file": "a.rs"})
        );
    }

    #[test]
    fn compact_bash_truncates_cmd() {
        let long = "x".repeat(200);
        let o = json!({"type": "bash", "cmd": long, "ts": "x"});
        let c = compact_obs(&o);
        assert_eq!(c["type"], "bash");
        assert_eq!(c["cmd"].as_str().unwrap().chars().count(), 120);
    }

    #[test]
    fn compact_other_type_keeps_type() {
        let o = json!({"type": "agent_call", "agent": "x"});
        assert_eq!(compact_obs(&o), json!({"type": "agent_call"}));
    }

    // ── build_correction_record ──────────────────────────────────────

    #[test]
    fn correction_record_shape() {
        let prev = json!({"type": "file_edit", "file": "x.rs"});
        let v = build_correction_record(Some("sid"), "não faz", Some(&prev), "TS");
        assert_eq!(v["session_id"], "sid");
        assert_eq!(v["prompt"], "não faz");
        assert_eq!(v["previous_action"]["type"], "file_edit");
        assert_eq!(v["previous_action"]["file"], "x.rs");
        assert_eq!(v["ts"], "TS");
    }

    #[test]
    fn correction_record_no_previous() {
        let v = build_correction_record(Some("sid"), "stop", None, "TS");
        assert_eq!(v["previous_action"], Value::Null);
    }

    // ── render_md_block ──────────────────────────────────────────────

    #[test]
    fn md_block_strips_leading_header() {
        let content = "# Decisões\n\n- item 1\n- item 2\n";
        let got = render_md_block("## Decisões registradas", content).unwrap();
        assert_eq!(got[0], "## Decisões registradas");
        assert_eq!(got[1], "- item 1\n- item 2");
    }

    #[test]
    fn md_block_returns_none_when_empty() {
        assert!(render_md_block("## x", "   \n\n").is_none());
        assert!(render_md_block("## x", "").is_none());
    }

    // ── format_git_block ─────────────────────────────────────────────

    #[test]
    fn git_block_minimal() {
        let g = GitContext {
            branch: "main".into(),
            modified: vec![],
            recent_commits: vec![],
        };
        let out = format_git_block(&g);
        assert_eq!(out, vec!["## Estado do git", "- Branch: `main`"]);
    }

    #[test]
    fn git_block_truncates_modified_list() {
        let modified: Vec<String> = (0..15).map(|i| format!("f{i}.rs")).collect();
        let g = GitContext {
            branch: "b".into(),
            modified,
            recent_commits: vec![],
        };
        let out = format_git_block(&g);
        let joined = out.join("\n");
        assert!(joined.contains("`f0.rs`"));
        assert!(joined.contains("`f9.rs`"));
        assert!(!joined.contains("`f10.rs`"));
        assert!(joined.contains("(+5)"));
    }

    #[test]
    fn git_block_with_commits() {
        let g = GitContext {
            branch: "main".into(),
            modified: vec![],
            recent_commits: vec![
                RecentCommit {
                    sha: "abc".into(),
                    msg: "first".into(),
                },
                RecentCommit {
                    sha: "def".into(),
                    msg: "second".into(),
                },
            ],
        };
        let out = format_git_block(&g).join("\n");
        assert!(out.contains("- Commits recentes:"));
        assert!(out.contains("  - `abc` first"));
        assert!(out.contains("  - `def` second"));
    }

    // ── format_adrs_block ────────────────────────────────────────────

    #[test]
    fn adrs_block_formats_entries() {
        let adrs = vec![
            AdrEntry {
                file: "docs/adr/0001.md".into(),
                title: "Use Postgres".into(),
            },
            AdrEntry {
                file: "docs/adr/0002.md".into(),
                title: "Event sourcing".into(),
            },
        ];
        let out = format_adrs_block(&adrs).join("\n");
        assert!(out.starts_with("## ADRs do repositório"));
        assert!(out.contains("- `docs/adr/0001.md` — Use Postgres"));
    }

    #[test]
    fn adr_title_falls_back_to_filename() {
        assert_eq!(adr_title_from_content("0001.md", ""), "0001");
        assert_eq!(
            adr_title_from_content("0001.md", "# Primary title\n\nbody"),
            "Primary title"
        );
    }

    #[test]
    fn adr_title_truncates_at_100_chars() {
        let big = "# ".to_string() + &"a".repeat(200);
        let t = adr_title_from_content("x.md", &big);
        assert_eq!(t.chars().count(), 100);
    }

    // ── format_sessions_block ────────────────────────────────────────

    #[test]
    fn sessions_block_picks_last_5_reversed() {
        let sessions: Vec<Value> = (1..=7)
            .map(|i| {
                json!({
                    "type": "end",
                    "ended_at": format!("2026-04-1{i}T10:00:00Z"),
                    "kind": "feature",
                    "summary": format!("item {i}")
                })
            })
            .collect();
        let out = format_sessions_block(&sessions).join("\n");
        assert!(out.starts_with("## Sessões recentes"));
        // Últimos 5 em ordem reversa: 7, 6, 5, 4, 3
        let p7 = out.find("item 7").unwrap();
        let p3 = out.find("item 3").unwrap();
        assert!(p7 < p3);
        assert!(!out.contains("item 2"));
        assert!(!out.contains("item 1"));
    }

    #[test]
    fn sessions_block_ignores_start_sessions() {
        let sessions = vec![
            json!({"type": "start", "session_id": "x"}),
            json!({"type": "end", "ended_at": "2026-04-19", "summary": "s1"}),
        ];
        let out = format_sessions_block(&sessions).join("\n");
        assert!(out.contains("s1"));
    }

    // ── format_corrections_block ─────────────────────────────────────

    #[test]
    fn corrections_block_formats_with_previous_action() {
        let corrections = vec![json!({
            "ts": "2026-04-19T10:00:00Z",
            "prompt": "não, erre menos",
            "previous_action": {"type": "file_edit", "file": "x.rs"}
        })];
        let out = format_corrections_block(&corrections).join("\n");
        assert!(out.contains("## Correções do dev (não repetir)"));
        assert!(out.contains("- **2026-04-19** (após editar x.rs): \"não, erre menos\""));
    }

    #[test]
    fn corrections_block_null_previous_action() {
        let corrections = vec![json!({
            "ts": "2026-04-19T10:00:00Z",
            "prompt": "stop",
            "previous_action": null
        })];
        let out = format_corrections_block(&corrections).join("\n");
        assert!(out.contains("- **2026-04-19**: \"stop\""));
    }

    // ── format_decisions_log_block ───────────────────────────────────

    #[test]
    fn decisions_log_with_why() {
        let d = vec![json!({
            "ts": "2026-04-19T", "what": "Usar Asaas", "why": "Pix nativo"
        })];
        let out = format_decisions_log_block(&d).join("\n");
        assert!(out.contains("- **2026-04-19**: Usar Asaas — _Pix nativo_"));
    }

    #[test]
    fn decisions_log_fallback_to_text_when_no_what() {
        let d = vec![json!({"ts": "x", "text": "algo"})];
        let out = format_decisions_log_block(&d).join("\n");
        assert!(out.contains("algo"));
    }

    // ── format_gotchas_log_block ─────────────────────────────────────

    #[test]
    fn gotchas_log_with_file() {
        let g = vec![json!({
            "ts": "2026-04-19", "description": "cargo test falhou", "file": "lib.rs"
        })];
        let out = format_gotchas_log_block(&g).join("\n");
        assert!(out.contains("- **2026-04-19**: cargo test falhou — `lib.rs`"));
    }

    // ── format_observations_block ────────────────────────────────────

    #[test]
    fn observations_block_file_edit_with_diff() {
        let obs = vec![json!({
            "type": "file_edit",
            "session_id": "other",
            "file": "a.rs",
            "ts": "2026-04-19T10:00:00Z",
            "diff": {"ins": 5, "del": 2, "tracked": true}
        })];
        let out = format_observations_block(&obs, Some("current")).join("\n");
        assert!(out.contains("EDIT `a.rs` (+5/-2)"));
    }

    #[test]
    fn observations_block_file_edit_untracked() {
        let obs = vec![json!({
            "type": "file_edit",
            "session_id": "other",
            "file": "a.rs",
            "ts": "2026-04-19T10:00:00Z",
            "diff": {"ins": 3, "del": 0, "tracked": false}
        })];
        let out = format_observations_block(&obs, Some("c")).join("\n");
        assert!(out.contains("(+3/-0 novo)"));
    }

    #[test]
    fn observations_block_bash_with_summary() {
        let obs = vec![json!({
            "type": "bash",
            "session_id": "other",
            "cmd": "cargo test",
            "exit": 0,
            "output_summary": "3 passed",
            "ts": "2026-04-19T10:00:00Z"
        })];
        let out = format_observations_block(&obs, Some("c")).join("\n");
        assert!(out.contains("BASH `cargo test` → exit 0 _3 passed_"));
    }

    #[test]
    fn observations_block_agent_call() {
        let obs = vec![json!({
            "type": "agent_call",
            "session_id": "other",
            "agent": "explore",
            "description": "find x",
            "ts": "2026-04-19T10:00:00Z"
        })];
        let out = format_observations_block(&obs, Some("c")).join("\n");
        assert!(out.contains("AGENT `explore` — find x"));
    }

    #[test]
    fn observations_block_filters_current_session() {
        let obs = vec![
            json!({"type": "bash", "session_id": "cur", "cmd": "x", "ts": "t1"}),
            json!({"type": "bash", "session_id": "other", "cmd": "y", "ts": "t2"}),
        ];
        let out = format_observations_block(&obs, Some("cur")).join("\n");
        assert!(out.contains("`y`"));
        assert!(!out.contains("`x`"));
    }

    #[test]
    fn observations_block_limits_to_40() {
        let obs: Vec<Value> = (0..100)
            .map(|i| {
                json!({
                    "type": "bash",
                    "session_id": "other",
                    "cmd": format!("cmd_{i}"),
                    "ts": format!("2026-04-19T10:{:02}:00Z", i)
                })
            })
            .collect();
        let out = format_observations_block(&obs, Some("c"));
        // 1 header + 40 items
        assert_eq!(out.len(), 41);
    }

    // ── apply_token_budget ───────────────────────────────────────────

    #[test]
    fn budget_sorts_by_priority_and_stops_at_limit() {
        let blocks = vec![
            Block {
                name: "a".into(),
                priority: 2,
                text: "A".into(),
                tokens: 30,
            },
            Block {
                name: "b".into(),
                priority: 1,
                text: "B".into(),
                tokens: 40,
            },
            Block {
                name: "c".into(),
                priority: 3,
                text: "C".into(),
                tokens: 35,
            },
        ];
        let (inc, skip, total) = apply_token_budget(blocks, 80);
        // Sorted by priority: b(40) → a(30) → c(35)
        // b: 40 total=40 OK; a: 30 total=70 OK; c: 35 total=105 > 80 SKIP.
        assert_eq!(inc.len(), 2);
        assert_eq!(inc[0].name, "b");
        assert_eq!(inc[1].name, "a");
        assert_eq!(skip.len(), 1);
        assert_eq!(skip[0].name, "c");
        assert_eq!(total, 70);
    }

    #[test]
    fn budget_zero_skips_everything() {
        let blocks = vec![Block {
            name: "a".into(),
            priority: 1,
            text: "A".into(),
            tokens: 10,
        }];
        let (inc, skip, total) = apply_token_budget(blocks, 0);
        assert_eq!(inc.len(), 0);
        assert_eq!(skip.len(), 1);
        assert_eq!(total, 0);
    }

    // ── render_final_context ─────────────────────────────────────────

    #[test]
    fn final_context_with_separator_and_footer() {
        let included = vec![
            Block {
                name: "a".into(),
                priority: 1,
                text: "AA".into(),
                tokens: 2,
            },
            Block {
                name: "b".into(),
                priority: 2,
                text: "BB".into(),
                tokens: 2,
            },
        ];
        let got = render_final_context(&included, &[], 4, 100);
        assert!(got.contains("AA"));
        assert!(got.contains("BB"));
        assert!(got.contains("---"));
        assert!(got.contains("<!-- project-mem: ~4/100t -->"));
    }

    #[test]
    fn final_context_footer_with_skipped_names() {
        let included = vec![Block {
            name: "a".into(),
            priority: 1,
            text: "A".into(),
            tokens: 2,
        }];
        let skipped = vec![Block {
            name: "obs".into(),
            priority: 10,
            text: "x".into(),
            tokens: 100,
        }];
        let got = render_final_context(&included, &skipped, 2, 50);
        assert!(got.contains("omitido: obs"));
    }

    #[test]
    fn final_context_empty_when_nothing() {
        let got = render_final_context(&[], &[], 0, 100);
        assert_eq!(got, "");
    }

    // ── render_hook_payload ──────────────────────────────────────────

    #[test]
    fn hook_payload_without_ctx() {
        let got = render_hook_payload(None);
        assert_eq!(got, r#"{"continue":true,"suppressOutput":true}"#);
    }

    #[test]
    fn hook_payload_with_ctx() {
        let got = render_hook_payload(Some("hello"));
        assert!(got.contains(r#""hookEventName":"UserPromptSubmit""#));
        assert!(got.contains(r#""additionalContext":"hello""#));
    }

    // ── render_session_start_record ──────────────────────────────────

    #[test]
    fn start_record_field_order() {
        let v = render_session_start_record(Some("s"), "TS", "/p");
        let s = serde_json::to_string(&v).unwrap();
        assert_eq!(
            s,
            r#"{"type":"start","session_id":"s","started_at":"TS","cwd":"/p"}"#
        );
    }
}
