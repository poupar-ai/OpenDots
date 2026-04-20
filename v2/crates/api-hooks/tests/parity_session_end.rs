//! Paridade `pmem-hook-session-end` (Rust) ↔ `plugin/scripts/session-end.cjs`.
//!
//! Estratégia: dois tempdirs com estado inicial idêntico, mesmo stdin
//! para os dois lados. Depois comparamos:
//! - Última linha de `sessions.jsonl` com `ended_at` removido.
//! - `gotchas.jsonl` completo com `ts` removido (gotchas detectados).
//! - `DECISIONS.md`, `GOTCHAS.md`, `GLOSSARY.md` byte-a-byte quando
//!   existem.
//! - Existência da flag `tmp/.init-<session_id>` após a run.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use tempfile::TempDir;

fn session_end_cjs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("locate repo root")
        .join("plugin/scripts/session-end.cjs")
}

fn rust_bin() -> &'static str {
    env!("CARGO_BIN_EXE_pmem-hook-session-end")
}

fn init_git_repo(tmp: &TempDir) -> PathBuf {
    let root = tmp.path().to_path_buf();
    let status = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .expect("spawn git init");
    assert!(status.success(), "git init falhou");
    for (k, v) in [("user.name", "Test"), ("user.email", "t@t.t")] {
        let _ = Command::new("git")
            .args(["config", k, v])
            .current_dir(&root)
            .status();
    }
    root.canonicalize().expect("canonicalize root")
}

fn run_with_stdin(cmd: &mut Command, stdin_json: &str) {
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
}

fn run_cjs(root: &Path, stdin_json: &str) {
    let mut cmd = Command::new("node");
    cmd.arg(session_end_cjs())
        .current_dir(root)
        .env("CLAUDE_CWD", root);
    run_with_stdin(&mut cmd, stdin_json);
}

fn run_rust(root: &Path, stdin_json: &str) {
    let mut cmd = Command::new(rust_bin());
    cmd.current_dir(root).env("CLAUDE_CWD", root);
    run_with_stdin(&mut cmd, stdin_json);
}

/// Última linha de sessions.jsonl sem `ended_at` (varia por run).
fn last_session_sans_ts(root: &Path) -> Option<Value> {
    let p = root.join(".claude-project-memory/sessions.jsonl");
    let content = fs::read_to_string(&p).ok()?;
    let last = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .last()?
        .to_string();
    let mut v: Value = serde_json::from_str(&last).ok()?;
    if let Some(m) = v.as_object_mut() {
        m.remove("ended_at");
    }
    Some(v)
}

fn gotchas_sans_ts(root: &Path) -> Vec<Value> {
    let p = root.join(".claude-project-memory/gotchas.jsonl");
    let Ok(content) = fs::read_to_string(&p) else {
        return Vec::new();
    };
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .map(|mut v| {
            if let Some(m) = v.as_object_mut() {
                m.remove("ts");
            }
            v
        })
        .collect()
}

/// Popula o mem_dir com `observations.jsonl` em ambos os tempdirs.
fn seed_mem_dir(root: &Path, observations: &[Value]) {
    let mem = root.join(".claude-project-memory");
    fs::create_dir_all(mem.join("tmp")).unwrap();
    let mut buf = String::new();
    for o in observations {
        buf.push_str(&serde_json::to_string(o).unwrap());
        buf.push('\n');
    }
    fs::write(mem.join("observations.jsonl"), buf).unwrap();
}

fn read_md(root: &Path, name: &str) -> Option<String> {
    fs::read_to_string(root.join(".claude-project-memory").join(name)).ok()
}

// ─── Testes ──────────────────────────────────────────────────────────

#[test]
fn parity_feature_session_with_edits_and_tests() {
    let tmp_c = TempDir::new().unwrap();
    let tmp_r = TempDir::new().unwrap();
    let root_c = init_git_repo(&tmp_c);
    let root_r = init_git_repo(&tmp_r);

    let obs = vec![
        json!({"type": "file_edit", "session_id": "s1", "file": "src/lib.rs", "ts": "2026-04-19T09:00:00.000Z"}),
        json!({"type": "file_edit", "session_id": "s1", "file": "src/test_lib.rs", "ts": "2026-04-19T09:05:00.000Z"}),
        json!({"type": "bash", "session_id": "s1", "cmd": "cargo test", "exit": 0, "ts": "2026-04-19T09:10:00.000Z"}),
    ];
    seed_mem_dir(&root_c, &obs);
    seed_mem_dir(&root_r, &obs);

    let input = json!({"session_id": "s1"}).to_string();
    run_cjs(&root_c, &input);
    run_rust(&root_r, &input);

    let c = last_session_sans_ts(&root_c).expect("cjs sess");
    let r = last_session_sans_ts(&root_r).expect("rust sess");
    assert_eq!(c, r);
    assert_eq!(c["type"], "end");
    assert_eq!(c["session_id"], "s1");
    assert_eq!(c["obs_count"], 3);
    assert_eq!(c["kind"], "feature");
    assert_eq!(
        c["summary"],
        "Editado: lib.rs, test_lib.rs. Rodou: cargo test"
    );
}

#[test]
fn parity_docs_session() {
    let tmp_c = TempDir::new().unwrap();
    let tmp_r = TempDir::new().unwrap();
    let root_c = init_git_repo(&tmp_c);
    let root_r = init_git_repo(&tmp_r);

    let obs = vec![json!({
        "type": "file_edit", "session_id": "s", "file": "README.md", "ts": "2026-04-19T09:00:00.000Z"
    })];
    seed_mem_dir(&root_c, &obs);
    seed_mem_dir(&root_r, &obs);

    let input = json!({"session_id": "s"}).to_string();
    run_cjs(&root_c, &input);
    run_rust(&root_r, &input);

    let c = last_session_sans_ts(&root_c).unwrap();
    let r = last_session_sans_ts(&root_r).unwrap();
    assert_eq!(c, r);
    assert_eq!(c["kind"], "docs");
}

#[test]
fn parity_detects_gotcha_triad() {
    let tmp_c = TempDir::new().unwrap();
    let tmp_r = TempDir::new().unwrap();
    let root_c = init_git_repo(&tmp_c);
    let root_r = init_git_repo(&tmp_r);

    let obs = vec![
        json!({
            "type": "bash", "session_id": "s", "cmd": "cargo test --bin x",
            "exit": 1, "output_summary": "FAILED", "ts": "2026-04-19T09:00:00.000Z"
        }),
        json!({
            "type": "file_edit", "session_id": "s", "file": "src/lib.rs",
            "ts": "2026-04-19T09:05:00.000Z"
        }),
        json!({
            "type": "bash", "session_id": "s", "cmd": "cargo test --bin x",
            "exit": 0, "ts": "2026-04-19T09:10:00.000Z"
        }),
    ];
    seed_mem_dir(&root_c, &obs);
    seed_mem_dir(&root_r, &obs);

    let input = json!({"session_id": "s"}).to_string();
    run_cjs(&root_c, &input);
    run_rust(&root_r, &input);

    let c = gotchas_sans_ts(&root_c);
    let r = gotchas_sans_ts(&root_r);
    assert_eq!(c.len(), 1);
    assert_eq!(c, r);
    assert_eq!(c[0]["file"], "src/lib.rs");
    assert_eq!(c[0]["failure_hint"], "FAILED");
    assert!(c[0]["description"].as_str().unwrap().contains("cargo test"));

    // GOTCHAS.md é regenerado — compare byte-a-byte.
    let md_c = read_md(&root_c, "GOTCHAS.md");
    let md_r = read_md(&root_r, "GOTCHAS.md");
    assert_eq!(md_c, md_r);
    let md = md_c.expect("md");
    assert!(md.contains("# Armadilhas conhecidas"));
    assert!(md.contains("src/lib.rs"));
    assert!(md.contains("_Sintoma:_ FAILED"));
}

#[test]
fn parity_regenerates_decisions_md() {
    let tmp_c = TempDir::new().unwrap();
    let tmp_r = TempDir::new().unwrap();
    let root_c = init_git_repo(&tmp_c);
    let root_r = init_git_repo(&tmp_r);

    let obs = vec![json!({
        "type": "bash", "session_id": "s", "cmd": "pwd", "exit": 0, "ts": "x"
    })];
    seed_mem_dir(&root_c, &obs);
    seed_mem_dir(&root_r, &obs);

    // decisions.jsonl pre-populado igual em ambos (regeneração lê disco).
    let decisions = [
        json!({"ts": "2026-04-18T09:00:00.000Z", "what": "Usar Asaas", "why": "Pix + cartão nativos"}),
        json!({"ts": "2026-04-19T09:00:00.000Z", "what": "0% comissão", "why": "diferencial central"}),
    ];
    let mut buf = String::new();
    for d in &decisions {
        buf.push_str(&serde_json::to_string(d).unwrap());
        buf.push('\n');
    }
    for root in [&root_c, &root_r] {
        fs::write(root.join(".claude-project-memory/decisions.jsonl"), &buf).unwrap();
    }

    let input = json!({"session_id": "s"}).to_string();
    run_cjs(&root_c, &input);
    run_rust(&root_r, &input);

    let md_c = read_md(&root_c, "DECISIONS.md").expect("cjs md");
    let md_r = read_md(&root_r, "DECISIONS.md").expect("rust md");
    assert_eq!(md_c, md_r);
    // Ordem reversa — '0% comissão' é mais recente, vem primeiro.
    let p1 = md_c.find("0% comissão").unwrap();
    let p2 = md_c.find("Usar Asaas").unwrap();
    assert!(p1 < p2);
}

#[test]
fn parity_regenerates_glossary_md() {
    let tmp_c = TempDir::new().unwrap();
    let tmp_r = TempDir::new().unwrap();
    let root_c = init_git_repo(&tmp_c);
    let root_r = init_git_repo(&tmp_r);

    // glossary.jsonl populado → esperado escrever GLOSSARY.md.
    let glossary_lines = "{\"term\":\"Seller\",\"definition\":\"vendedor\"}\n{\"term\":\"Buyer\",\"definition\":\"comprador\"}\n";
    for root in [&root_c, &root_r] {
        let mem = root.join(".claude-project-memory");
        fs::create_dir_all(&mem).unwrap();
        fs::write(mem.join("glossary.jsonl"), glossary_lines).unwrap();
        // Sem observações.jsonl → hook entra no early-return de regeneração.
    }

    let input = json!({"session_id": "s"}).to_string();
    run_cjs(&root_c, &input);
    run_rust(&root_r, &input);

    let md_c = read_md(&root_c, "GLOSSARY.md");
    let md_r = read_md(&root_r, "GLOSSARY.md");
    assert_eq!(md_c, md_r);
    let md = md_c.expect("md");
    assert!(md.contains("## Seller"));
    assert!(md.contains("## Buyer"));
}

#[test]
fn parity_session_without_observations_file() {
    // Sem observations.jsonl → ambos só regeneram MDs (nada aqui pra regenerar)
    // e exit. Nenhum sessions.jsonl deve existir.
    let tmp_c = TempDir::new().unwrap();
    let tmp_r = TempDir::new().unwrap();
    let root_c = init_git_repo(&tmp_c);
    let root_r = init_git_repo(&tmp_r);

    for root in [&root_c, &root_r] {
        fs::create_dir_all(root.join(".claude-project-memory")).unwrap();
    }

    let input = json!({"session_id": "s"}).to_string();
    run_cjs(&root_c, &input);
    run_rust(&root_r, &input);

    assert!(!root_c
        .join(".claude-project-memory/sessions.jsonl")
        .exists());
    assert!(!root_r
        .join(".claude-project-memory/sessions.jsonl")
        .exists());
}

#[test]
fn parity_session_no_matching_observations() {
    // observations.jsonl existe mas nenhuma obs bate o session_id → só
    // regenera MDs e exit. Nada em sessions.jsonl.
    let tmp_c = TempDir::new().unwrap();
    let tmp_r = TempDir::new().unwrap();
    let root_c = init_git_repo(&tmp_c);
    let root_r = init_git_repo(&tmp_r);

    let obs = vec![json!({
        "type": "bash", "session_id": "other", "cmd": "pwd", "exit": 0, "ts": "x"
    })];
    seed_mem_dir(&root_c, &obs);
    seed_mem_dir(&root_r, &obs);

    let input = json!({"session_id": "target"}).to_string();
    run_cjs(&root_c, &input);
    run_rust(&root_r, &input);

    assert!(!root_c
        .join(".claude-project-memory/sessions.jsonl")
        .exists());
    assert!(!root_r
        .join(".claude-project-memory/sessions.jsonl")
        .exists());
}

#[test]
fn parity_cleans_up_init_flag() {
    let tmp_c = TempDir::new().unwrap();
    let tmp_r = TempDir::new().unwrap();
    let root_c = init_git_repo(&tmp_c);
    let root_r = init_git_repo(&tmp_r);

    let obs = vec![json!({
        "type": "file_edit", "session_id": "s", "file": "a.rs", "ts": "x"
    })];
    seed_mem_dir(&root_c, &obs);
    seed_mem_dir(&root_r, &obs);

    for root in [&root_c, &root_r] {
        let flag = root.join(".claude-project-memory/tmp/.init-s");
        fs::write(&flag, "").unwrap();
        assert!(flag.exists());
    }

    let input = json!({"session_id": "s"}).to_string();
    run_cjs(&root_c, &input);
    run_rust(&root_r, &input);

    assert!(!root_c.join(".claude-project-memory/tmp/.init-s").exists());
    assert!(!root_r.join(".claude-project-memory/tmp/.init-s").exists());
}
