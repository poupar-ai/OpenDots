//! Paridade `pmem-hook-observe` (Rust) ↔ `plugin/scripts/observe.cjs`.
//!
//! Estratégia: para cada caso, cria dois tempdirs com estado inicial
//! idêntico (mesmos arquivos, mesma git state), alimenta o mesmo JSON
//! no stdin dos dois processos, depois compara o conteúdo de
//! `observations.jsonl` dos dois lados com o campo `ts` removido
//! (cada run tem um timestamp próprio).
//!
//! Os efeitos laterais em `graph.json` e `stats.json` são testados na
//! paridade do `domain-graph` e `core-paths::stats` respectivamente —
//! aqui interessa a **observação crua** que o hook grava.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use tempfile::TempDir;

fn observe_cjs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("locate repo root")
        .join("plugin/scripts/observe.cjs")
}

fn rust_bin() -> &'static str {
    env!("CARGO_BIN_EXE_pmem-hook-observe")
}

fn init_git_repo(tmp: &TempDir) -> PathBuf {
    let root = tmp.path().to_path_buf();
    let status = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .expect("spawn git init");
    assert!(status.success(), "git init falhou");
    // Config mínima pro repo aceitar commits determinísticos.
    for (k, v) in [("user.name", "Test"), ("user.email", "t@t.t")] {
        let _ = Command::new("git")
            .args(["config", k, v])
            .current_dir(&root)
            .status();
    }
    root.canonicalize().expect("canonicalize root")
}

/// Spawna um processo, escreve `stdin_json` no stdin, espera saída.
/// Retorna stdout (ignorado pelos testes — o efeito está em
/// observations.jsonl).
fn run_with_stdin(cmd: &mut Command, stdin_json: &str) -> (i32, String) {
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
    let code = out.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if code != 0 {
        panic!("process exit {code}: {stderr}");
    }
    (code, String::from_utf8_lossy(&out.stdout).to_string())
}

fn run_cjs(root: &Path, stdin_json: &str) {
    let mut cmd = Command::new("node");
    cmd.arg(observe_cjs())
        .current_dir(root)
        .env("CLAUDE_CWD", root)
        .env_remove("CLAUDE_PROJECT_MEM_AGENT");
    run_with_stdin(&mut cmd, stdin_json);
}

fn run_rust(root: &Path, stdin_json: &str) {
    let mut cmd = Command::new(rust_bin());
    cmd.current_dir(root)
        .env("CLAUDE_CWD", root)
        .env_remove("CLAUDE_PROJECT_MEM_AGENT");
    run_with_stdin(&mut cmd, stdin_json);
}

/// Lê a última observação gravada, com `ts` removido (varia por run).
/// Retorna `None` se o arquivo não existe ou está vazio.
fn last_obs_without_ts(root: &Path) -> Option<Value> {
    let p = root.join(".claude-project-memory/observations.jsonl");
    let content = fs::read_to_string(&p).ok()?;
    let last = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .last()?
        .to_string();
    let mut v: Value = serde_json::from_str(&last).ok()?;
    if let Some(m) = v.as_object_mut() {
        m.remove("ts");
    }
    Some(v)
}

fn observations_count(root: &Path) -> usize {
    let p = root.join(".claude-project-memory/observations.jsonl");
    fs::read_to_string(&p)
        .map(|c| c.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

// ─── Testes ──────────────────────────────────────────────────────────

#[test]
fn parity_bash_pytest_summary() {
    let tmp_c = TempDir::new().unwrap();
    let tmp_r = TempDir::new().unwrap();
    let root_c = init_git_repo(&tmp_c);
    let root_r = init_git_repo(&tmp_r);

    let input = json!({
        "session_id": "sess-42",
        "tool_name": "Bash",
        "tool_input": { "command": "pytest -q" },
        "tool_response": {
            "stdout": "collected 3 items\n3 passed in 0.01s",
            "exit_code": 0
        }
    })
    .to_string();

    run_cjs(&root_c, &input);
    run_rust(&root_r, &input);

    let c = last_obs_without_ts(&root_c).expect("cjs obs");
    let r = last_obs_without_ts(&root_r).expect("rust obs");
    assert_eq!(c, r);
    assert_eq!(c["type"], "bash");
    assert_eq!(c["output_summary"], "pytest: 3 passed");
}

#[test]
fn parity_bash_skipped_trivial_command() {
    let tmp_c = TempDir::new().unwrap();
    let tmp_r = TempDir::new().unwrap();
    let root_c = init_git_repo(&tmp_c);
    let root_r = init_git_repo(&tmp_r);

    let input = json!({
        "session_id": "s",
        "tool_name": "Bash",
        "tool_input": { "command": "ls -la" },
        "tool_response": { "stdout": "", "exit_code": 0 }
    })
    .to_string();

    run_cjs(&root_c, &input);
    run_rust(&root_r, &input);

    assert_eq!(observations_count(&root_c), 0);
    assert_eq!(observations_count(&root_r), 0);
}

#[test]
fn parity_irrelevant_tool_produces_nothing() {
    let tmp_c = TempDir::new().unwrap();
    let tmp_r = TempDir::new().unwrap();
    let root_c = init_git_repo(&tmp_c);
    let root_r = init_git_repo(&tmp_r);

    let input = json!({
        "session_id": "s",
        "tool_name": "Read",
        "tool_input": { "file_path": "x.rs" },
        "tool_response": { "content": "..." }
    })
    .to_string();

    run_cjs(&root_c, &input);
    run_rust(&root_r, &input);

    assert_eq!(observations_count(&root_c), 0);
    assert_eq!(observations_count(&root_r), 0);
}

#[test]
fn parity_tool_error_skipped() {
    let tmp_c = TempDir::new().unwrap();
    let tmp_r = TempDir::new().unwrap();
    let root_c = init_git_repo(&tmp_c);
    let root_r = init_git_repo(&tmp_r);

    let input = json!({
        "session_id": "s",
        "tool_name": "Bash",
        "tool_input": { "command": "cargo test" },
        "tool_response": { "error": "blew up", "exit_code": 1 }
    })
    .to_string();

    run_cjs(&root_c, &input);
    run_rust(&root_r, &input);

    assert_eq!(observations_count(&root_c), 0);
    assert_eq!(observations_count(&root_r), 0);
}

#[test]
fn parity_agent_call_task() {
    let tmp_c = TempDir::new().unwrap();
    let tmp_r = TempDir::new().unwrap();
    let root_c = init_git_repo(&tmp_c);
    let root_r = init_git_repo(&tmp_r);

    let input = json!({
        "session_id": "sess-1",
        "tool_name": "Task",
        "tool_input": {
            "subagent_type": "Explore",
            "description": "find X",
            "prompt": "Look at the codebase and find X."
        },
        "tool_response": { "content": "..." }
    })
    .to_string();

    run_cjs(&root_c, &input);
    run_rust(&root_r, &input);

    let c = last_obs_without_ts(&root_c).expect("cjs obs");
    let r = last_obs_without_ts(&root_r).expect("rust obs");
    assert_eq!(c, r);
    assert_eq!(c["type"], "agent_call");
    assert_eq!(c["agent"], "explore");
    assert_eq!(c["description"], "find X");
    assert_eq!(c["prompt"], "Look at the codebase and find X.");
}

#[test]
fn parity_file_edit_inside_repo_with_diff() {
    let tmp_c = TempDir::new().unwrap();
    let tmp_r = TempDir::new().unwrap();
    let root_c = init_git_repo(&tmp_c);
    let root_r = init_git_repo(&tmp_r);

    // Commit um arquivo base idêntico em ambos os repos pra ter HEAD
    // definido, depois modifica pra gerar diff tracked.
    for root in [&root_c, &root_r] {
        fs::write(root.join("lib.rs"), "fn a() {}\n").unwrap();
        Command::new("git")
            .args(["add", "lib.rs"])
            .current_dir(root)
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", "init"])
            .current_dir(root)
            .status()
            .unwrap();
        // Modifica: +1 linha.
        fs::write(root.join("lib.rs"), "fn a() {}\nfn b() {}\n").unwrap();
    }

    let input_for = |root: &Path| {
        json!({
            "session_id": "sess-edit",
            "tool_name": "Write",
            "tool_input": { "file_path": root.join("lib.rs").to_string_lossy() },
            "tool_response": { "content": "ok" }
        })
        .to_string()
    };

    run_cjs(&root_c, &input_for(&root_c));
    run_rust(&root_r, &input_for(&root_r));

    let c = last_obs_without_ts(&root_c).expect("cjs obs");
    let r = last_obs_without_ts(&root_r).expect("rust obs");
    assert_eq!(c, r);
    assert_eq!(c["type"], "file_edit");
    assert_eq!(c["file"], "lib.rs");
    assert_eq!(c["diff"]["tracked"], true);
    assert_eq!(c["diff"]["ins"], 1);
    assert_eq!(c["diff"]["del"], 0);
    assert!(c["diff"]["preview"].as_str().unwrap().contains("fn b"));
}

#[test]
fn parity_file_edit_untracked_file() {
    let tmp_c = TempDir::new().unwrap();
    let tmp_r = TempDir::new().unwrap();
    let root_c = init_git_repo(&tmp_c);
    let root_r = init_git_repo(&tmp_r);

    // Cria o arquivo mas não commita — HEAD não existe ainda no repo.
    for root in [&root_c, &root_r] {
        fs::write(root.join("new.txt"), "hello\nworld\n").unwrap();
    }

    let input_for = |root: &Path| {
        json!({
            "session_id": "sess-untracked",
            "tool_name": "Write",
            "tool_input": { "file_path": root.join("new.txt").to_string_lossy() },
            "tool_response": { "content": "ok" }
        })
        .to_string()
    };

    run_cjs(&root_c, &input_for(&root_c));
    run_rust(&root_r, &input_for(&root_r));

    let c = last_obs_without_ts(&root_c);
    let r = last_obs_without_ts(&root_r);
    // Ambos os lados devem escrever a observação (com ou sem diff — a
    // paridade é que eles batam entre si).
    assert_eq!(c, r);
    let c = c.expect("cjs obs");
    assert_eq!(c["type"], "file_edit");
    assert_eq!(c["file"], "new.txt");
}

#[test]
fn parity_bash_truncates_cmd_at_200_chars() {
    let tmp_c = TempDir::new().unwrap();
    let tmp_r = TempDir::new().unwrap();
    let root_c = init_git_repo(&tmp_c);
    let root_r = init_git_repo(&tmp_r);

    // Comando de 250 chars. Os primeiros 200 devem sobreviver.
    let long = format!("cargo {}", "x".repeat(250));
    let input = json!({
        "session_id": "s",
        "tool_name": "Bash",
        "tool_input": { "command": long },
        "tool_response": { "exit_code": 0 }
    })
    .to_string();

    run_cjs(&root_c, &input);
    run_rust(&root_r, &input);

    let c = last_obs_without_ts(&root_c).expect("cjs obs");
    let r = last_obs_without_ts(&root_r).expect("rust obs");
    assert_eq!(c, r);
    assert_eq!(c["cmd"].as_str().unwrap().chars().count(), 200);
}

#[test]
fn parity_agent_id_env_propagates() {
    let tmp_c = TempDir::new().unwrap();
    let tmp_r = TempDir::new().unwrap();
    let root_c = init_git_repo(&tmp_c);
    let root_r = init_git_repo(&tmp_r);

    let input = json!({
        "session_id": "s",
        "tool_name": "Bash",
        "tool_input": { "command": "cargo build" },
        "tool_response": { "exit_code": 0 }
    })
    .to_string();

    // Custom runners que setam CLAUDE_PROJECT_MEM_AGENT.
    let mut c_cmd = Command::new("node");
    c_cmd
        .arg(observe_cjs())
        .current_dir(&root_c)
        .env("CLAUDE_CWD", &root_c)
        .env("CLAUDE_PROJECT_MEM_AGENT", "agent-007");
    run_with_stdin(&mut c_cmd, &input);

    let mut r_cmd = Command::new(rust_bin());
    r_cmd
        .current_dir(&root_r)
        .env("CLAUDE_CWD", &root_r)
        .env("CLAUDE_PROJECT_MEM_AGENT", "agent-007");
    run_with_stdin(&mut r_cmd, &input);

    let c = last_obs_without_ts(&root_c).expect("cjs obs");
    let r = last_obs_without_ts(&root_r).expect("rust obs");
    assert_eq!(c, r);
    assert_eq!(c["agent_id"], "agent-007");
}
