//! Paridade `pmem-hook-startup` (Rust) ↔ `plugin/scripts/startup.cjs`.
//!
//! Ambos são invocados no mesmo cwd controlado (repo git temporário),
//! com o mesmo conjunto de env vars; o stdout (1 linha JSON) deve bater
//! byte-a-byte.
//!
//! ### O que controlamos via env
//!
//! - `CLAUDE_CWD` — força o cwd lógico (bypassa `PWD`/`cwd()`).
//! - `CLAUDE_PLUGIN_ROOT` — aponta para o plugin real, pra que
//!   `seed.cjs` e `mcp/server.cjs` tenham paths determinísticos.
//! - `CLAUDE_CONFIG_DIR` — aponta para um dir temporário com
//!   `settings.json` fake, pra evitar ler o `~/.claude` real.
//! - `PROJECT_MEM_UI_AUTOSTART=0` — desliga o spawn da UI no `.cjs`.
//!   Essencial: sem isso, o `.cjs` tenta spawnar um detached child
//!   cada execução, poluindo o `/tmp` e o teste.
//! - `PROJECT_MEM_PORT` — fixado pra ter o link final igual.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn startup_cjs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("locate repo root")
        .join("plugin/scripts/startup.cjs")
}

fn plugin_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("locate repo root")
        .join("plugin")
}

fn rust_bin() -> &'static str {
    env!("CARGO_BIN_EXE_pmem-hook-startup")
}

/// Cria um repo git vazio dentro de `tmp`, retorna o path.
fn init_git_repo(tmp: &TempDir) -> PathBuf {
    let root = tmp.path().to_path_buf();
    let status = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .expect("spawn git init");
    assert!(status.success(), "git init falhou");
    // Paridade com o `.cjs`: `projectRoot !== cwd` é o teste de "inRepo";
    // o init faz o cwd virar toplevel. Canonicalize pra casar com o que
    // `git rev-parse --show-toplevel` devolve.
    root.canonicalize().expect("canonicalize root")
}

/// Seed do settings.json fake no `<cfg_dir>/settings.json`.
/// Se `mcp_registered=true`, registra o entry `mcpServers["project-mem"]`
/// (sem warning). Caso contrário, deixa outro entry (com warning esperado).
fn write_settings(cfg_dir: &Path, mcp_registered: bool) {
    fs::create_dir_all(cfg_dir).unwrap();
    let body = if mcp_registered {
        r#"{"mcpServers":{"project-mem":{"type":"stdio","command":"node","args":["x"]}}}"#
    } else {
        r#"{"mcpServers":{"other":{}}}"#
    };
    fs::write(cfg_dir.join("settings.json"), body).unwrap();
}

/// Roda o `.cjs` e devolve stdout limpo (sem `\n` final).
fn run_cjs(root: &Path, cfg_dir: &Path) -> String {
    let out = Command::new("node")
        .arg(startup_cjs())
        .current_dir(root)
        .env("CLAUDE_CWD", root)
        .env("CLAUDE_PLUGIN_ROOT", plugin_root())
        .env("CLAUDE_CONFIG_DIR", cfg_dir)
        .env("PROJECT_MEM_UI_AUTOSTART", "0")
        .env("PROJECT_MEM_PORT", "37778")
        .output()
        .expect("spawn node startup.cjs");
    if !out.status.success() {
        panic!(
            "node falhou (status {:?}): stderr={}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Roda o Rust binary e devolve stdout limpo.
fn run_rust(root: &Path, cfg_dir: &Path) -> String {
    let out = Command::new(rust_bin())
        .current_dir(root)
        .env("CLAUDE_CWD", root)
        .env("CLAUDE_PLUGIN_ROOT", plugin_root())
        .env("CLAUDE_CONFIG_DIR", cfg_dir)
        .env("PROJECT_MEM_UI_AUTOSTART", "0")
        .env("PROJECT_MEM_PORT", "37778")
        .output()
        .expect("spawn rust binary");
    if !out.status.success() {
        panic!(
            "rust falhou (status {:?}): stderr={}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

// ─── Testes ──────────────────────────────────────────────────────────

#[test]
fn parity_sem_memdir() {
    let tmp = TempDir::new().unwrap();
    let root = init_git_repo(&tmp);
    let cfg = TempDir::new().unwrap();
    write_settings(cfg.path(), true);

    let c = run_cjs(&root, cfg.path());
    let r = run_rust(&root, cfg.path());
    assert_eq!(c, r);
    assert!(c.contains("sem memória ainda"));
}

#[test]
fn parity_com_memdir_sem_stats() {
    let tmp = TempDir::new().unwrap();
    let root = init_git_repo(&tmp);
    let mem = root.join(".claude-project-memory");
    fs::create_dir_all(&mem).unwrap();
    fs::write(
        mem.join("sessions.jsonl"),
        "{\"type\":\"start\"}\n{\"type\":\"end\"}\n{\"type\":\"end\"}\n",
    )
    .unwrap();
    fs::write(mem.join("observations.jsonl"), "{}\n{}\n{}\n").unwrap();
    let cfg = TempDir::new().unwrap();
    write_settings(cfg.path(), true);

    let c = run_cjs(&root, cfg.path());
    let r = run_rust(&root, cfg.path());
    assert_eq!(c, r);
    assert!(c.contains("2 sessões · 3 obs"));
}

#[test]
fn parity_com_stats_cache() {
    let tmp = TempDir::new().unwrap();
    let root = init_git_repo(&tmp);
    let mem = root.join(".claude-project-memory");
    fs::create_dir_all(&mem).unwrap();
    fs::write(
        mem.join("stats.json"),
        r#"{
  "sessions": 42,
  "observations": 99,
  "last_injection_tokens": 1500,
  "last_injection_budget": 4000,
  "last_injection_skipped": []
}"#,
    )
    .unwrap();
    fs::write(mem.join("PROJECT.md"), "# proj").unwrap();
    let cfg = TempDir::new().unwrap();
    write_settings(cfg.path(), true);

    let c = run_cjs(&root, cfg.path());
    let r = run_rust(&root, cfg.path());
    assert_eq!(c, r);
    assert!(c.contains("42 sessões · 99 obs"));
    assert!(c.contains("PROJECT.md ✓"));
    assert!(c.contains("~1500/4000t"));
}

#[test]
fn parity_stats_com_skipped() {
    let tmp = TempDir::new().unwrap();
    let root = init_git_repo(&tmp);
    let mem = root.join(".claude-project-memory");
    fs::create_dir_all(&mem).unwrap();
    fs::write(
        mem.join("stats.json"),
        r#"{
  "sessions": 5,
  "observations": 10,
  "last_injection_tokens": 3800,
  "last_injection_budget": 4000,
  "last_injection_skipped": ["graph:120", "observations:430"]
}"#,
    )
    .unwrap();
    fs::write(mem.join("PROJECT.md"), "# proj").unwrap();
    let cfg = TempDir::new().unwrap();
    write_settings(cfg.path(), true);

    let c = run_cjs(&root, cfg.path());
    let r = run_rust(&root, cfg.path());
    assert_eq!(c, r);
    assert!(c.contains("(⚠2 omitidos)"));
}

#[test]
fn parity_agents_poucos() {
    let tmp = TempDir::new().unwrap();
    let root = init_git_repo(&tmp);
    let mem = root.join(".claude-project-memory");
    fs::create_dir_all(&mem).unwrap();
    fs::write(mem.join("agents.json"), r#"{"agents":{"a":{},"b":{}}}"#).unwrap();
    let cfg = TempDir::new().unwrap();
    write_settings(cfg.path(), true);

    let c = run_cjs(&root, cfg.path());
    let r = run_rust(&root, cfg.path());
    assert_eq!(c, r);
    assert!(c.contains("👥 sem time"));
    assert!(c.contains("seed.cjs setup"));
}

#[test]
fn parity_agents_suficientes() {
    let tmp = TempDir::new().unwrap();
    let root = init_git_repo(&tmp);
    let mem = root.join(".claude-project-memory");
    fs::create_dir_all(&mem).unwrap();
    fs::write(
        mem.join("agents.json"),
        r#"{"agents":{"a":{},"b":{},"c":{},"d":{}}}"#,
    )
    .unwrap();
    let cfg = TempDir::new().unwrap();
    write_settings(cfg.path(), true);

    let c = run_cjs(&root, cfg.path());
    let r = run_rust(&root, cfg.path());
    assert_eq!(c, r);
    assert!(!c.contains("sem time"));
}

#[test]
fn parity_warning_mcp_nao_registrado() {
    let tmp = TempDir::new().unwrap();
    let root = init_git_repo(&tmp);
    let cfg = TempDir::new().unwrap();
    write_settings(cfg.path(), false);

    let c = run_cjs(&root, cfg.path());
    let r = run_rust(&root, cfg.path());
    assert_eq!(c, r);
    assert!(c.contains("⚠ MCP não registrado"));
    assert!(c.contains("mcp/server.cjs"));
}

#[test]
fn parity_sem_settings_json() {
    // settings.json ausente → nenhum warning.
    let tmp = TempDir::new().unwrap();
    let root = init_git_repo(&tmp);
    let cfg = TempDir::new().unwrap();
    // NÃO cria settings.json.

    let c = run_cjs(&root, cfg.path());
    let r = run_rust(&root, cfg.path());
    assert_eq!(c, r);
    assert!(!c.contains("MCP não registrado"));
}
