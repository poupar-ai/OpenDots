//! Lógica do hook `SessionEnd` — porta de `plugin/scripts/session-end.cjs`.
//!
//! Três camadas:
//!
//! | Função                     | Responsabilidade                                   | Tipo      |
//! |----------------------------|----------------------------------------------------|-----------|
//! | [`classify_session`]       | Pura. Aplica heurística `docs/config/bugfix/...`. | Unit      |
//! | [`detect_gotchas`]         | Pura. Encontra triplas fail→edit→success.          | Unit      |
//! | [`build_summary`]          | Pura. Monta o resumo textual `Editado/Rodou/Notas`.| Unit      |
//! | [`build_session_end_record`]| Pura. JSON `{ type, session_id, ended_at, ...}`.  | Unit      |
//! | [`render_decisions_md`]    | Pura. Renderização de JSONL → Markdown.            | Unit      |
//! | [`render_gotchas_md`]      | Idem.                                              | Unit      |
//! | [`render_glossary_md`]     | Idem.                                              | Unit      |
//!
//! Efeitos (`append_jsonl`, `increment_stats`, `update_index_for_files`,
//! `on_gotcha`) moram no binário.

use serde_json::{json, Map, Value};

/// Constrói o resumo textual lido em `session-end.cjs`:
/// - "Editado: a.rs, b.rs"
/// - "Rodou: cargo test; npm run build"
/// - "Notas: ..."
///
/// Fallback: `"<N> operações"` quando nenhuma parte se aplica.
pub fn build_summary(session_obs: &[Value]) -> String {
    let edited_files: Vec<String> = dedupe_keep_first(
        session_obs
            .iter()
            .filter(|o| o.get("type") == Some(&json!("file_edit")))
            .filter_map(|o| o.get("file").and_then(Value::as_str).map(last_path_segment))
            .collect(),
    )
    .into_iter()
    .take(8)
    .collect();

    let cmds: Vec<String> = dedupe_keep_first(
        session_obs
            .iter()
            .filter(|o| o.get("type") == Some(&json!("bash")))
            .filter_map(|o| o.get("cmd").and_then(Value::as_str).map(first_four_words))
            .collect(),
    )
    .into_iter()
    .take(5)
    .collect();

    let notes: Vec<String> = session_obs
        .iter()
        .filter(|o| o.get("type") == Some(&json!("note")))
        .filter_map(|o| {
            o.get("content")
                .and_then(Value::as_str)
                .map(|s| s.to_string())
        })
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

fn last_path_segment(s: &str) -> String {
    s.rsplit('/').next().unwrap_or(s).to_string()
}

fn first_four_words(s: &str) -> String {
    s.split_whitespace().take(4).collect::<Vec<_>>().join(" ")
}

fn dedupe_keep_first(items: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for i in items {
        if seen.insert(i.clone()) {
            out.push(i);
        }
    }
    out
}

/// Classifica a sessão em um dos:
/// `docs`, `config`, `bugfix`, `refactor`, `feature`, `exploration`, `unknown`.
///
/// Replica `classifySession` de `session-end.cjs`.
pub fn classify_session(obs: &[Value]) -> &'static str {
    let files: Vec<String> = obs
        .iter()
        .filter(|o| o.get("type") == Some(&json!("file_edit")))
        .map(|o| {
            o.get("file")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        })
        .collect();
    let cmds: Vec<String> = obs
        .iter()
        .filter(|o| o.get("type") == Some(&json!("bash")))
        .map(|o| {
            o.get("cmd")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_lowercase()
        })
        .collect();

    let all_text = {
        let mut v = files.clone();
        v.extend(cmds.clone());
        v.join(" ").to_lowercase()
    };

    let docs_re = regex::Regex::new(r"(?i)\.md$|readme|docs/").unwrap();
    let only_docs = !files.is_empty() && files.iter().all(|f| docs_re.is_match(f));
    if only_docs {
        return "docs";
    }

    let config_re = regex::Regex::new(
        r"(?i)\.(env|config|toml|yml|yaml|json)$|dockerfile|\.dockerignore|makefile",
    )
    .unwrap();
    let code_re = regex::Regex::new(r"(?i)\.(py|ts|tsx|js|jsx|rs|go|java)$").unwrap();
    if files.iter().any(|f| config_re.is_match(f)) && !files.iter().any(|f| code_re.is_match(f)) {
        return "config";
    }

    let bugfix_cmd_re = regex::Regex::new(r"\b(fix|bugfix|debug|hotfix)\b").unwrap();
    let bug_text_re = regex::Regex::new(r"\bfix\b|\bbug\b").unwrap();
    let test_re = regex::Regex::new(r"(?i)test").unwrap();
    if cmds.iter().any(|c| bugfix_cmd_re.is_match(c))
        || (bug_text_re.is_match(&all_text) && files.iter().any(|f| test_re.is_match(f)))
    {
        return "bugfix";
    }

    let refactor_re = regex::Regex::new(r"\brefactor\b|\bmove\b|\brename\b").unwrap();
    if cmds.iter().any(|c| refactor_re.is_match(c)) {
        return "refactor";
    }

    let has_tests = files.iter().any(|f| test_re.is_match(f));
    let has_src = files.iter().any(|f| code_re.is_match(f));
    if has_tests && has_src {
        return "feature";
    }
    if has_src {
        return "feature";
    }

    let file_edits = obs
        .iter()
        .filter(|o| o.get("type") == Some(&json!("file_edit")))
        .count();
    if file_edits == 0 && !cmds.is_empty() {
        return "exploration";
    }
    "unknown"
}

/// Detecta triplas `bash(fail) → file_edit → bash(success)` com os 3
/// primeiros tokens de `cmd` iguais. Retorna o registro de gotcha para
/// cada ocorrência (caller persiste + propaga pra graph).
///
/// Replica `detectGotchas` de `session-end.cjs`.
pub fn detect_gotchas(session_obs: &[Value], session_id: &str, ts: &str) -> Vec<Value> {
    let mut out = Vec::new();
    if session_obs.len() < 3 {
        return out;
    }
    for i in 0..session_obs.len().saturating_sub(2) {
        let a = &session_obs[i];
        let b = &session_obs[i + 1];
        let c = &session_obs[i + 2];

        if a.get("type") != Some(&json!("bash")) {
            continue;
        }
        let a_exit = a.get("exit").and_then(Value::as_i64).unwrap_or(0);
        if a_exit == 0 {
            continue;
        }
        if b.get("type") != Some(&json!("file_edit")) {
            continue;
        }
        if c.get("type") != Some(&json!("bash")) {
            continue;
        }
        let a_cmd = a.get("cmd").and_then(Value::as_str).unwrap_or("");
        let c_cmd = c.get("cmd").and_then(Value::as_str).unwrap_or("");
        let same_cmd = first_three_words(a_cmd) == first_three_words(c_cmd);
        if !same_cmd {
            continue;
        }
        let c_exit = c.get("exit").and_then(Value::as_i64).unwrap_or(1);
        if c_exit != 0 {
            continue;
        }

        let b_file = b
            .get("file")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let description = format!("`{}` falhou até editar `{}`", chars_take(a_cmd, 80), b_file);
        let failure_hint = a
            .get("output_summary")
            .cloned()
            .filter(|v| !v.is_null())
            .unwrap_or(Value::Null);

        let mut g = Map::new();
        g.insert("session_id".into(), json!(session_id));
        g.insert("file".into(), json!(b_file));
        g.insert("command".into(), json!(chars_take(a_cmd, 120)));
        g.insert("failure_hint".into(), failure_hint);
        g.insert("description".into(), json!(description));
        g.insert("ts".into(), json!(ts));
        out.push(Value::Object(g));
    }
    out
}

fn first_three_words(s: &str) -> String {
    s.split_whitespace().take(3).collect::<Vec<_>>().join(" ")
}

fn chars_take(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Constrói o registro que vai em `sessions.jsonl` como `{type:'end', ...}`.
pub fn build_session_end_record(
    session_id: Option<&str>,
    ended_at: &str,
    obs_count: usize,
    kind: &str,
    summary: &str,
) -> Value {
    let mut m = Map::new();
    m.insert("type".into(), json!("end"));
    m.insert(
        "session_id".into(),
        match session_id {
            Some(s) => json!(s),
            None => Value::Null,
        },
    );
    m.insert("ended_at".into(), json!(ended_at));
    m.insert("obs_count".into(), json!(obs_count));
    m.insert("kind".into(), json!(kind));
    m.insert("summary".into(), json!(summary));
    Value::Object(m)
}

// ── Regeneração de MDs ─────────────────────────────────────────────────

/// Reverte + renderiza decisions em `# Decisões do projeto`.
/// Retorna `None` se `decisions` está vazio (nenhum MD deve ser escrito).
pub fn render_decisions_md(decisions: &[Value]) -> Option<String> {
    if decisions.is_empty() {
        return None;
    }
    let reversed: Vec<&Value> = decisions.iter().rev().collect();
    let mut out: Vec<String> = Vec::new();
    out.push("# Decisões do projeto".into());
    out.push(String::new());
    for d in reversed {
        let date = d
            .get("ts")
            .and_then(Value::as_str)
            .map(|s| chars_take(s, 10))
            .unwrap_or_default();
        let title = d
            .get("what")
            .and_then(Value::as_str)
            .or_else(|| d.get("text").and_then(Value::as_str))
            .unwrap_or("(sem título)")
            .to_string();
        out.push(format!("## {date} — {title}"));
        if let Some(why) = d.get("why").and_then(Value::as_str) {
            out.push(String::new());
            out.push(why.to_string());
        }
        out.push(String::new());
    }
    Some(out.join("\n"))
}

/// Renderiza gotchas em `# Armadilhas conhecidas`. Deduplica por
/// `(file, command[:40])` preservando a ordem **reversa** (mais recentes
/// primeiro).
pub fn render_gotchas_md(gotchas: &[Value]) -> Option<String> {
    if gotchas.is_empty() {
        return None;
    }
    let mut seen = std::collections::HashSet::new();
    let mut unique: Vec<&Value> = Vec::new();
    for g in gotchas.iter().rev() {
        let file = g.get("file").and_then(Value::as_str).unwrap_or("");
        let cmd = g.get("command").and_then(Value::as_str).unwrap_or("");
        let key = format!("{}|{}", file, chars_take(cmd, 40));
        if seen.insert(key) {
            unique.push(g);
        }
    }
    if unique.is_empty() {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    out.push("# Armadilhas conhecidas".into());
    out.push(String::new());
    for g in unique {
        let date = g
            .get("ts")
            .and_then(Value::as_str)
            .map(|s| chars_take(s, 10))
            .unwrap_or_default();
        let file = g.get("file").and_then(Value::as_str).unwrap_or("");
        out.push(format!("## {date} — `{file}`"));
        out.push(String::new());
        let description = g.get("description").and_then(Value::as_str).unwrap_or("");
        out.push(description.to_string());
        if let Some(hint) = g.get("failure_hint").and_then(Value::as_str) {
            out.push(String::new());
            out.push(format!("_Sintoma:_ {hint}"));
        }
        out.push(String::new());
    }
    Some(out.join("\n"))
}

pub fn render_glossary_md(entries: &[Value]) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    out.push("# Glossário do projeto".into());
    out.push(String::new());
    for e in entries {
        let term = e.get("term").and_then(Value::as_str).unwrap_or("");
        let def = e.get("definition").and_then(Value::as_str).unwrap_or("");
        out.push(format!("## {term}"));
        out.push(String::new());
        out.push(def.to_string());
        out.push(String::new());
    }
    Some(out.join("\n"))
}

/// Filtra observações por `session_id`. Helper trivial — aqui porque
/// aparece em 2 callers e `session_end.cjs` faz a mesma operação.
pub fn filter_by_session<'a>(obs: &'a [Value], session_id: &str) -> Vec<Value> {
    obs.iter()
        .filter(|o| o.get("session_id").and_then(Value::as_str) == Some(session_id))
        .cloned()
        .collect()
}

/// Extrai o conjunto de arquivos editados numa sessão, preservando
/// ordem de primeira aparição e dedupe por path.
pub fn session_changed_files(session_obs: &[Value]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for o in session_obs {
        if o.get("type") != Some(&json!("file_edit")) {
            continue;
        }
        if let Some(f) = o.get("file").and_then(Value::as_str) {
            if !f.is_empty() && seen.insert(f.to_string()) {
                out.push(f.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obs_file(session: &str, file: &str) -> Value {
        json!({"type": "file_edit", "session_id": session, "file": file})
    }
    fn obs_bash(session: &str, cmd: &str) -> Value {
        json!({"type": "bash", "session_id": session, "cmd": cmd})
    }
    fn obs_bash_exit(session: &str, cmd: &str, exit: i64) -> Value {
        json!({"type": "bash", "session_id": session, "cmd": cmd, "exit": exit})
    }

    // ── build_summary ──────────────────────────────────────────────────

    #[test]
    fn summary_edited_files_only() {
        let obs = vec![
            obs_file("s", "crates/x/src/lib.rs"),
            obs_file("s", "crates/x/src/lib.rs"),
            obs_file("s", "crates/y/src/mod.rs"),
        ];
        assert_eq!(build_summary(&obs), "Editado: lib.rs, mod.rs");
    }

    #[test]
    fn summary_combines_all_parts() {
        let obs = vec![
            obs_file("s", "a.rs"),
            obs_bash("s", "cargo test -q"),
            json!({"type": "note", "content": "pensei x"}),
        ];
        let got = build_summary(&obs);
        assert!(got.contains("Editado: a.rs"));
        assert!(got.contains("Rodou: cargo test -q"));
        assert!(got.contains("Notas: pensei x"));
    }

    #[test]
    fn summary_falls_back_to_ops_count() {
        let obs = vec![json!({"type": "other"}), json!({"type": "other"})];
        assert_eq!(build_summary(&obs), "2 operações");
    }

    #[test]
    fn summary_limits_files_to_8() {
        let obs: Vec<Value> = (0..20)
            .map(|i| obs_file("s", &format!("f{i}.rs")))
            .collect();
        let s = build_summary(&obs);
        // 8 files, 0..7
        assert!(s.starts_with("Editado: f0.rs, f1.rs, f2.rs, f3.rs, f4.rs, f5.rs, f6.rs, f7.rs"));
        assert!(!s.contains("f8.rs"));
    }

    #[test]
    fn summary_limits_cmds_to_5_and_truncates_to_4_words() {
        let obs: Vec<Value> = (0..10)
            .map(|i| obs_bash("s", &format!("cargo run --bin x{i} --release --features a")))
            .collect();
        let s = build_summary(&obs);
        // cmds are dedup'd by first 4 words — all 10 distinct — we keep 5.
        assert!(s.contains("cargo run --bin x0"));
        assert!(s.contains("cargo run --bin x4"));
        assert!(!s.contains("x5"));
        // Words beyond 4 get dropped in summary display.
        assert!(!s.contains("--release"));
    }

    // ── classify_session ──────────────────────────────────────────────

    #[test]
    fn classify_docs_when_all_md() {
        let obs = vec![obs_file("s", "README.md"), obs_file("s", "docs/guide.md")];
        assert_eq!(classify_session(&obs), "docs");
    }

    #[test]
    fn classify_config_when_only_config_files() {
        let obs = vec![obs_file("s", "Dockerfile"), obs_file("s", "config.toml")];
        assert_eq!(classify_session(&obs), "config");
    }

    #[test]
    fn classify_bugfix_when_fix_in_cmd() {
        let obs = vec![
            obs_bash("s", "git commit -m 'fix crash'"),
            obs_file("s", "src/lib.rs"),
        ];
        assert_eq!(classify_session(&obs), "bugfix");
    }

    #[test]
    fn classify_refactor_when_cmd_has_rename() {
        let obs = vec![
            obs_bash("s", "git mv --rename a b"),
            obs_file("s", "src/lib.rs"),
        ];
        assert_eq!(classify_session(&obs), "refactor");
    }

    #[test]
    fn classify_feature_when_tests_and_src() {
        let obs = vec![
            obs_file("s", "src/lib.rs"),
            obs_file("s", "src/test_foo.rs"),
        ];
        assert_eq!(classify_session(&obs), "feature");
    }

    #[test]
    fn classify_feature_when_only_src() {
        let obs = vec![obs_file("s", "src/lib.rs")];
        assert_eq!(classify_session(&obs), "feature");
    }

    #[test]
    fn classify_exploration_when_only_bash() {
        let obs = vec![obs_bash("s", "grep foo .")];
        assert_eq!(classify_session(&obs), "exploration");
    }

    #[test]
    fn classify_unknown_when_nothing() {
        let obs: Vec<Value> = vec![];
        assert_eq!(classify_session(&obs), "unknown");
    }

    // ── detect_gotchas ─────────────────────────────────────────────────

    #[test]
    fn detect_triad_fail_edit_success() {
        let obs = vec![
            json!({"type": "bash", "cmd": "cargo test --bin x", "exit": 1, "output_summary": "FAILED"}),
            obs_file("s", "src/lib.rs"),
            obs_bash_exit("s", "cargo test --bin x", 0),
        ];
        let got = detect_gotchas(&obs, "s", "2026-04-19T10:00:00.000Z");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["file"], "src/lib.rs");
        assert_eq!(got[0]["failure_hint"], "FAILED");
        assert!(got[0]["description"]
            .as_str()
            .unwrap()
            .contains("cargo test"));
        assert_eq!(got[0]["ts"], "2026-04-19T10:00:00.000Z");
    }

    #[test]
    fn detect_none_when_cmd_prefix_differs() {
        let obs = vec![
            obs_bash_exit("s", "cargo build", 1),
            obs_file("s", "src/lib.rs"),
            obs_bash_exit("s", "cargo test", 0),
        ];
        assert!(detect_gotchas(&obs, "s", "t").is_empty());
    }

    #[test]
    fn detect_none_when_second_bash_also_failed() {
        let obs = vec![
            obs_bash_exit("s", "cargo test", 1),
            obs_file("s", "src/lib.rs"),
            obs_bash_exit("s", "cargo test", 1),
        ];
        assert!(detect_gotchas(&obs, "s", "t").is_empty());
    }

    #[test]
    fn detect_ignores_when_first_bash_exit_missing() {
        // a.exit ausente → JS default 0 → skip. (Tratado como sucesso.)
        let obs = vec![
            json!({"type": "bash", "cmd": "cargo test"}),
            obs_file("s", "x.rs"),
            obs_bash_exit("s", "cargo test", 0),
        ];
        assert!(detect_gotchas(&obs, "s", "t").is_empty());
    }

    #[test]
    fn detect_accepts_when_last_exit_missing() {
        // c.exit ausente → JS `c.exit ?? 1` → 1 → skip (precisa ser 0).
        let obs = vec![
            obs_bash_exit("s", "cargo test", 1),
            obs_file("s", "x.rs"),
            json!({"type": "bash", "cmd": "cargo test"}),
        ];
        assert!(detect_gotchas(&obs, "s", "t").is_empty());
    }

    // ── build_session_end_record ───────────────────────────────────────

    #[test]
    fn session_end_record_field_order() {
        let v = build_session_end_record(Some("sid"), "2026-04-19T10:00:00Z", 5, "feature", "x");
        let s = serde_json::to_string(&v).unwrap();
        assert_eq!(
            s,
            r#"{"type":"end","session_id":"sid","ended_at":"2026-04-19T10:00:00Z","obs_count":5,"kind":"feature","summary":"x"}"#
        );
    }

    #[test]
    fn session_end_record_null_session_id() {
        let v = build_session_end_record(None, "ts", 0, "unknown", "nada");
        assert_eq!(v["session_id"], Value::Null);
    }

    // ── render_decisions_md ────────────────────────────────────────────

    #[test]
    fn render_decisions_empty() {
        assert!(render_decisions_md(&[]).is_none());
    }

    #[test]
    fn render_decisions_reverses_order() {
        let decisions = vec![
            json!({"ts": "2026-04-18T10:00:00Z", "what": "Primeiro", "why": "motivo A"}),
            json!({"ts": "2026-04-19T10:00:00Z", "what": "Segundo", "why": "motivo B"}),
        ];
        let out = render_decisions_md(&decisions).unwrap();
        let p1 = out.find("Primeiro").unwrap();
        let p2 = out.find("Segundo").unwrap();
        assert!(p2 < p1, "segundo deve vir antes (reverse order)");
        assert!(out.contains("2026-04-19 — Segundo"));
        assert!(out.contains("motivo B"));
    }

    #[test]
    fn render_decisions_fallback_title() {
        let decisions = vec![json!({"ts": "2026-04-19T..."})];
        let out = render_decisions_md(&decisions).unwrap();
        assert!(out.contains("(sem título)"));
    }

    // ── render_gotchas_md ──────────────────────────────────────────────

    #[test]
    fn render_gotchas_dedupes_by_file_and_cmd_prefix() {
        // Os 2 primeiros têm mesmo file + mesmo cmd → mesmo key → segundo vence
        // (iteração reversa). O terceiro tem cmd diferente → entra também.
        let gotchas = vec![
            json!({"ts": "2026-04-18", "file": "a.rs", "command": "cargo test --bin x", "description": "primeiro"}),
            json!({"ts": "2026-04-19", "file": "a.rs", "command": "cargo test --bin x", "description": "segundo"}),
            json!({"ts": "2026-04-19", "file": "b.rs", "command": "cargo test", "description": "terceiro"}),
        ];
        let out = render_gotchas_md(&gotchas).unwrap();
        // Ordem reversa: b.rs (mais recente) primeiro, depois a.rs (dedup mantém o mais novo).
        let pb = out.find("`b.rs`").unwrap();
        let pa = out.find("`a.rs`").unwrap();
        assert!(pb < pa);
        assert!(!out.contains("primeiro"));
        assert!(out.contains("segundo"));
        assert!(out.contains("terceiro"));
    }

    #[test]
    fn render_gotchas_empty() {
        assert!(render_gotchas_md(&[]).is_none());
    }

    #[test]
    fn render_gotchas_with_failure_hint() {
        let g = vec![json!({
            "ts": "2026-04-19", "file": "a.rs", "command": "cargo test",
            "description": "x", "failure_hint": "FAIL 42"
        })];
        let out = render_gotchas_md(&g).unwrap();
        assert!(out.contains("_Sintoma:_ FAIL 42"));
    }

    // ── render_glossary_md ─────────────────────────────────────────────

    #[test]
    fn render_glossary_orders_as_input() {
        let entries = vec![
            json!({"term": "Seller", "definition": "vendedor"}),
            json!({"term": "Buyer", "definition": "comprador"}),
        ];
        let out = render_glossary_md(&entries).unwrap();
        let ps = out.find("## Seller").unwrap();
        let pb = out.find("## Buyer").unwrap();
        assert!(ps < pb);
    }

    #[test]
    fn render_glossary_empty() {
        assert!(render_glossary_md(&[]).is_none());
    }

    // ── filter_by_session ─────────────────────────────────────────────

    #[test]
    fn filter_matches_exact_session_id() {
        let obs = vec![obs_file("a", "x"), obs_file("b", "y"), obs_file("a", "z")];
        let filtered = filter_by_session(&obs, "a");
        assert_eq!(filtered.len(), 2);
    }

    // ── session_changed_files ─────────────────────────────────────────

    #[test]
    fn changed_files_dedupes_and_preserves_order() {
        let obs = vec![
            obs_file("s", "a.rs"),
            obs_file("s", "b.rs"),
            obs_file("s", "a.rs"),
            obs_bash("s", "cargo test"),
        ];
        let got = session_changed_files(&obs);
        assert_eq!(got, vec!["a.rs".to_string(), "b.rs".to_string()]);
    }
}
