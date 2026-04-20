//! Paridade `pmem-hook-session-init` (Rust) ↔ `plugin/scripts/session-init.cjs`.
//!
//! Estratégia: dois tempdirs com estado inicial idêntico, mesmo stdin
//! para os dois lados. Depois comparamos:
//! - Payload JSON impresso em stdout (todo o output do hook).
//! - Última linha de `sessions.jsonl` com `started_at` removido.
//! - `corrections.jsonl` com `ts` removido.
//! - `.claude/CLAUDE.md` byte-a-byte quando existir.
//! - `stats.json` com `last_injection_at` removido.
//!
//! Gotchas de cross-language:
//! - Git commit SHA depende de timestamp — seedamos `GIT_AUTHOR_DATE` e
//!   `GIT_COMMITTER_DATE` para SHAs determinísticas.
//! - `last_injection_at` varia por run → stripado antes de comparar.
//! - Timezone do timestamp em ISO não interfere pois ambos usam UTC.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use tempfile::TempDir;

// ── Localização dos binários ─────────────────────────────────────────

fn session_init_cjs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("locate repo root")
        .join("plugin/scripts/session-init.cjs")
}

fn rust_bin() -> &'static str {
    env!("CARGO_BIN_EXE_pmem-hook-session-init")
}

// ── Setup do tempdir ─────────────────────────────────────────────────

fn init_git_repo(tmp: &TempDir) -> PathBuf {
    let root = tmp.path().to_path_buf();
    let status = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .expect("spawn git init");
    assert!(status.success(), "git init falhou");
    for (k, v) in [
        ("user.name", "Test"),
        ("user.email", "t@t.t"),
        ("commit.gpgsign", "false"),
    ] {
        let _ = Command::new("git")
            .args(["config", k, v])
            .current_dir(&root)
            .status();
    }
    root.canonicalize().expect("canonicalize root")
}

fn mem_dir(root: &Path) -> PathBuf {
    let p = root.join(".claude-project-memory");
    fs::create_dir_all(&p).expect("create mem dir");
    p
}

// ── Execução cross-language ──────────────────────────────────────────

struct Output {
    stdout: String,
}

fn run_with_stdin(cmd: &mut Command, stdin_json: &str) -> Output {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin_json.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    if !out.status.success() {
        panic!(
            "process exit {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Output {
        stdout: String::from_utf8(out.stdout).expect("utf8 stdout"),
    }
}

fn run_cjs(root: &Path, stdin_json: &str) -> Output {
    let mut cmd = Command::new("node");
    cmd.arg(session_init_cjs())
        .current_dir(root)
        .env("CLAUDE_CWD", root)
        .env_remove("PROJECT_MEM_TOKEN_BUDGET");
    run_with_stdin(&mut cmd, stdin_json)
}

fn run_rust(root: &Path, stdin_json: &str) -> Output {
    let mut cmd = Command::new(rust_bin());
    cmd.current_dir(root)
        .env("CLAUDE_CWD", root)
        .env_remove("PROJECT_MEM_TOKEN_BUDGET");
    run_with_stdin(&mut cmd, stdin_json)
}

// ── Normalizadores ────────────────────────────────────────────────────

fn parse_stdout_strip_dynamic(raw: &str) -> Value {
    let line = raw
        .lines()
        .find(|l| l.trim().starts_with('{'))
        .unwrap_or("{}");
    let v: Value = serde_json::from_str(line).expect("parse stdout json");

    v
}

fn last_jsonl_line(path: &Path, strip_keys: &[&str]) -> Option<Value> {
    let content = fs::read_to_string(path).ok()?;
    let last = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .last()?
        .to_string();
    let mut v: Value = serde_json::from_str(&last).ok()?;
    if let Some(m) = v.as_object_mut() {
        for k in strip_keys {
            m.remove(*k);
        }
    }
    Some(v)
}

fn all_jsonl_lines_stripped(path: &Path, strip_keys: &[&str]) -> Vec<Value> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .map(|mut v| {
            if let Some(m) = v.as_object_mut() {
                for k in strip_keys {
                    m.remove(*k);
                }
            }
            v
        })
        .collect()
}

fn load_stats_stripped(root: &Path) -> Option<Value> {
    let p = root.join(".claude-project-memory/stats.json");
    let content = fs::read_to_string(&p).ok()?;
    let mut v: Value = serde_json::from_str(&content).ok()?;
    if let Some(m) = v.as_object_mut() {
        m.remove("last_injection_at");
    }
    Some(v)
}

// ── Tests ──────────────────────────────────────────────────────────────

#[test]
fn first_time_project_emits_onboarding() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    let root_js = init_git_repo(&tmp_js);
    let root_rs = init_git_repo(&tmp_rs);
    mem_dir(&root_js);
    mem_dir(&root_rs);

    let stdin = json!({
        "session_id": "sess-1",
        "prompt": "hello",
    })
    .to_string();

    let out_js = run_cjs(&root_js, &stdin);
    let out_rs = run_rust(&root_rs, &stdin);

    let v_js = parse_stdout_strip_dynamic(&out_js.stdout);
    let v_rs = parse_stdout_strip_dynamic(&out_rs.stdout);
    assert_eq!(v_js, v_rs, "onboarding payload difere");

    let sess_js = last_jsonl_line(
        &root_js.join(".claude-project-memory/sessions.jsonl"),
        &["started_at", "cwd"],
    );
    let sess_rs = last_jsonl_line(
        &root_rs.join(".claude-project-memory/sessions.jsonl"),
        &["started_at", "cwd"],
    );
    assert_eq!(sess_js, sess_rs, "sessions.jsonl difere no first-time");
}

#[test]
fn already_injected_skips_silently() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    let root_js = init_git_repo(&tmp_js);
    let root_rs = init_git_repo(&tmp_rs);
    let md_js = mem_dir(&root_js);
    let md_rs = mem_dir(&root_rs);
    fs::create_dir_all(md_js.join("tmp")).unwrap();
    fs::create_dir_all(md_rs.join("tmp")).unwrap();
    fs::write(md_js.join("tmp/.init-sess-2"), "").unwrap();
    fs::write(md_rs.join("tmp/.init-sess-2"), "").unwrap();

    let stdin = json!({
        "session_id": "sess-2",
        "prompt": "anything",
    })
    .to_string();

    let out_js = run_cjs(&root_js, &stdin);
    let out_rs = run_rust(&root_rs, &stdin);

    let v_js = parse_stdout_strip_dynamic(&out_js.stdout);
    let v_rs = parse_stdout_strip_dynamic(&out_rs.stdout);
    assert_eq!(v_js, v_rs, "already-injected payload difere");
    assert_eq!(v_js, json!({ "continue": true, "suppressOutput": true }));
}

#[test]
fn correction_marker_writes_to_corrections_file() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    let root_js = init_git_repo(&tmp_js);
    let root_rs = init_git_repo(&tmp_rs);
    let md_js = mem_dir(&root_js);
    let md_rs = mem_dir(&root_rs);

    // Preciso de PROJECT.md pra não disparar first-time
    fs::write(md_js.join("PROJECT.md"), "# Projeto\n\nconteúdo").unwrap();
    fs::write(md_rs.join("PROJECT.md"), "# Projeto\n\nconteúdo").unwrap();

    // Uma observação recente (mesma session_id da correção)
    let obs = json!({
        "type": "file_edit",
        "session_id": "sess-corr",
        "file": "src/main.rs",
        "ts": "2026-04-16T10:00:00.000Z"
    });
    fs::write(
        md_js.join("observations.jsonl"),
        format!("{}\n", serde_json::to_string(&obs).unwrap()),
    )
    .unwrap();
    fs::write(
        md_rs.join("observations.jsonl"),
        format!("{}\n", serde_json::to_string(&obs).unwrap()),
    )
    .unwrap();

    let stdin = json!({
        "session_id": "sess-corr",
        "prompt": "não, reverte isso",
    })
    .to_string();

    run_cjs(&root_js, &stdin);
    run_rust(&root_rs, &stdin);

    let corr_js = all_jsonl_lines_stripped(
        &root_js.join(".claude-project-memory/corrections.jsonl"),
        &["ts"],
    );
    let corr_rs = all_jsonl_lines_stripped(
        &root_rs.join(".claude-project-memory/corrections.jsonl"),
        &["ts"],
    );
    assert_eq!(corr_js, corr_rs, "corrections.jsonl difere");
    assert_eq!(corr_js.len(), 1, "deveria ter 1 correção");
}

#[test]
fn subsequent_session_with_project_md_includes_block() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    let root_js = init_git_repo(&tmp_js);
    let root_rs = init_git_repo(&tmp_rs);
    let md_js = mem_dir(&root_js);
    let md_rs = mem_dir(&root_rs);

    fs::write(md_js.join("PROJECT.md"), "# Projeto\n\nconteúdo do projeto").unwrap();
    fs::write(md_rs.join("PROJECT.md"), "# Projeto\n\nconteúdo do projeto").unwrap();
    // precisa existir sessions.jsonl para não cair no first-time
    fs::write(
        md_js.join("sessions.jsonl"),
        "{\"type\":\"start\",\"session_id\":\"prev\",\"started_at\":\"2026-04-15T00:00:00.000Z\",\"cwd\":\"/x\"}\n",
    )
    .unwrap();
    fs::write(
        md_rs.join("sessions.jsonl"),
        "{\"type\":\"start\",\"session_id\":\"prev\",\"started_at\":\"2026-04-15T00:00:00.000Z\",\"cwd\":\"/x\"}\n",
    )
    .unwrap();

    let stdin = json!({
        "session_id": "sess-new",
        "prompt": "faça algo",
    })
    .to_string();

    let out_js = run_cjs(&root_js, &stdin);
    let out_rs = run_rust(&root_rs, &stdin);

    let v_js = parse_stdout_strip_dynamic(&out_js.stdout);
    let v_rs = parse_stdout_strip_dynamic(&out_rs.stdout);
    assert_eq!(v_js, v_rs, "payload com PROJECT.md difere");

    // CLAUDE.md deve ter sido sincronizado com o conteúdo PROJECT.md
    let claude_js = fs::read_to_string(root_js.join(".claude/CLAUDE.md")).unwrap();
    let claude_rs = fs::read_to_string(root_rs.join(".claude/CLAUDE.md")).unwrap();
    assert_eq!(claude_js, claude_rs, ".claude/CLAUDE.md difere");
}

#[test]
fn full_context_with_decisions_gotchas_glossary() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    let root_js = init_git_repo(&tmp_js);
    let root_rs = init_git_repo(&tmp_rs);
    let md_js = mem_dir(&root_js);
    let md_rs = mem_dir(&root_rs);

    let project_md = "# Projeto\n\nApenas um teste de paridade.";
    let decisions_md = "# Decisões\n\n- Sempre use Rust.";
    let gotchas_md = "# Armadilhas\n\n- Não esqueça do ponto e vírgula.";
    let glossary_md = "# Glossário\n\n- **Foo**: bar.";
    let sessions_prev = "{\"type\":\"start\",\"session_id\":\"s0\",\"started_at\":\"2026-04-10T00:00:00.000Z\",\"cwd\":\"/x\"}\n\
        {\"type\":\"end\",\"session_id\":\"s0\",\"ended_at\":\"2026-04-10T01:00:00.000Z\",\"observations_count\":3,\"kind\":\"feature\",\"summary\":\"Fez X e Y\"}\n";

    for md in [&md_js, &md_rs] {
        fs::write(md.join("PROJECT.md"), project_md).unwrap();
        fs::write(md.join("DECISIONS.md"), decisions_md).unwrap();
        fs::write(md.join("GOTCHAS.md"), gotchas_md).unwrap();
        fs::write(md.join("GLOSSARY.md"), glossary_md).unwrap();
        fs::write(md.join("sessions.jsonl"), sessions_prev).unwrap();
    }

    let stdin = json!({
        "session_id": "sess-full",
        "prompt": "testando",
    })
    .to_string();

    let out_js = run_cjs(&root_js, &stdin);
    let out_rs = run_rust(&root_rs, &stdin);

    let v_js = parse_stdout_strip_dynamic(&out_js.stdout);
    let v_rs = parse_stdout_strip_dynamic(&out_rs.stdout);
    assert_eq!(v_js, v_rs, "payload completo difere");

    let stats_js = load_stats_stripped(&root_js);
    let stats_rs = load_stats_stripped(&root_rs);
    assert_eq!(stats_js, stats_rs, "stats difere");
}

#[test]
fn soul_and_project_both_present_separator() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    let root_js = init_git_repo(&tmp_js);
    let root_rs = init_git_repo(&tmp_rs);
    let md_js = mem_dir(&root_js);
    let md_rs = mem_dir(&root_rs);

    let soul = "# Alma\n\n- Valor A\n- Valor B";
    let project = "# Projeto\n\nContexto factual.";
    for md in [&md_js, &md_rs] {
        fs::write(md.join("SOUL.md"), soul).unwrap();
        fs::write(md.join("PROJECT.md"), project).unwrap();
        fs::write(
            md.join("sessions.jsonl"),
            "{\"type\":\"start\",\"session_id\":\"s0\",\"started_at\":\"2026-04-10T00:00:00.000Z\",\"cwd\":\"/x\"}\n",
        )
        .unwrap();
    }

    let stdin = json!({
        "session_id": "sess-soul",
        "prompt": "oi",
    })
    .to_string();

    run_cjs(&root_js, &stdin);
    run_rust(&root_rs, &stdin);

    let claude_js = fs::read_to_string(root_js.join(".claude/CLAUDE.md")).unwrap();
    let claude_rs = fs::read_to_string(root_rs.join(".claude/CLAUDE.md")).unwrap();
    assert_eq!(
        claude_js, claude_rs,
        ".claude/CLAUDE.md (soul+project) difere"
    );
    assert!(claude_js.contains("Valor A"));
    assert!(claude_js.contains("Contexto factual."));
    assert!(
        claude_js.contains("---"),
        "deveria ter separador entre soul e project"
    );
}

#[test]
fn correction_with_no_previous_obs_is_recorded() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    let root_js = init_git_repo(&tmp_js);
    let root_rs = init_git_repo(&tmp_rs);
    let md_js = mem_dir(&root_js);
    let md_rs = mem_dir(&root_rs);

    for md in [&md_js, &md_rs] {
        fs::write(md.join("PROJECT.md"), "# P\n\ntest").unwrap();
        fs::write(
            md.join("sessions.jsonl"),
            "{\"type\":\"start\",\"session_id\":\"x\",\"started_at\":\"2026-04-10T00:00:00.000Z\",\"cwd\":\"/x\"}\n",
        )
        .unwrap();
    }

    let stdin = json!({
        "session_id": "sess-c",
        "prompt": "pare, está errado",
    })
    .to_string();

    run_cjs(&root_js, &stdin);
    run_rust(&root_rs, &stdin);

    let corr_js = all_jsonl_lines_stripped(
        &root_js.join(".claude-project-memory/corrections.jsonl"),
        &["ts"],
    );
    let corr_rs = all_jsonl_lines_stripped(
        &root_rs.join(".claude-project-memory/corrections.jsonl"),
        &["ts"],
    );
    assert_eq!(corr_js, corr_rs);
    assert_eq!(corr_js.len(), 1);
    assert_eq!(corr_js[0].get("previous_action"), Some(&Value::Null));
}

#[test]
fn recent_observations_block_excludes_current_session() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    let root_js = init_git_repo(&tmp_js);
    let root_rs = init_git_repo(&tmp_rs);
    let md_js = mem_dir(&root_js);
    let md_rs = mem_dir(&root_rs);

    let obs_content = "{\"type\":\"file_edit\",\"session_id\":\"old\",\"file\":\"src/a.rs\",\"ts\":\"2026-04-15T10:00:00.000Z\",\"diff\":{\"ins\":3,\"del\":1,\"tracked\":true}}\n\
        {\"type\":\"bash\",\"session_id\":\"old\",\"cmd\":\"cargo test\",\"ts\":\"2026-04-15T10:05:00.000Z\",\"exit\":0,\"output_summary\":\"4 passed\"}\n\
        {\"type\":\"file_edit\",\"session_id\":\"current\",\"file\":\"src/b.rs\",\"ts\":\"2026-04-16T09:00:00.000Z\",\"diff\":{\"ins\":1,\"del\":0,\"tracked\":true}}\n";

    for md in [&md_js, &md_rs] {
        fs::write(md.join("PROJECT.md"), "# P\n\ntest").unwrap();
        fs::write(md.join("observations.jsonl"), obs_content).unwrap();
        fs::write(
            md.join("sessions.jsonl"),
            "{\"type\":\"start\",\"session_id\":\"old\",\"started_at\":\"2026-04-15T00:00:00.000Z\",\"cwd\":\"/x\"}\n",
        )
        .unwrap();
    }

    let stdin = json!({
        "session_id": "current",
        "prompt": "seguindo",
    })
    .to_string();

    let out_js = run_cjs(&root_js, &stdin);
    let out_rs = run_rust(&root_rs, &stdin);

    let v_js = parse_stdout_strip_dynamic(&out_js.stdout);
    let v_rs = parse_stdout_strip_dynamic(&out_rs.stdout);
    assert_eq!(v_js, v_rs, "payload com observations difere");

    let ctx = v_js
        .pointer("/hookSpecificOutput/additionalContext")
        .and_then(Value::as_str)
        .unwrap_or("");
    assert!(ctx.contains("EDIT `src/a.rs`"));
    assert!(ctx.contains("BASH `cargo test`"));
    assert!(
        !ctx.contains("src/b.rs"),
        "observação da sessão corrente não deveria aparecer"
    );
}
