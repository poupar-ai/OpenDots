//! Paridade v1 (`agents.cjs::initDefaults` + `DEFAULT_MANIFEST` +
//! `DEFAULT_MINI_CONTEXTS`) vs v2 (`domain_agents::init_defaults`).
//!
//! Dois dirs irmãos, cada lado inicializa, comparação byte-a-byte de:
//! - `agents.json`
//! - `agents/<nome>.md` para cada um dos 7 agentes default
//!
//! Também valida semântica de `overwrite`:
//! - `overwrite = false` não sobrescreve arquivos existentes
//! - `overwrite = true` sobrescreve

use domain_agents::{default_mini_contexts, init_defaults};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const lib = require(process.argv[1]);
const memDir = process.argv[2];
const overwrite = process.argv[3] === 'true';
lib.initDefaults(memDir, { overwrite });
process.stdout.write('');
"#;

fn lib_cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/agents.cjs")
        .canonicalize()
        .expect("agents.cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn call_v1_init(mem_dir: &Path, overwrite: bool) {
    let out = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(lib_cjs_path())
        .arg(mem_dir)
        .arg(if overwrite { "true" } else { "false" })
        .output()
        .expect("spawn node");
    assert!(
        out.status.success(),
        "node falhou: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn twin_dirs(tmp: &TempDir) -> (PathBuf, PathBuf) {
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    (a, b)
}

fn assert_dirs_match(a: &Path, b: &Path) {
    // agents.json bytes idênticos
    let ja = fs::read(a.join("agents.json")).unwrap();
    let jb = fs::read(b.join("agents.json")).unwrap();
    assert_eq!(ja, jb, "agents.json difere entre .cjs e Rust");

    // cada mini-context bytes idênticos
    for (name, _) in default_mini_contexts() {
        let fa = a.join("agents").join(format!("{name}.md"));
        let fb = b.join("agents").join(format!("{name}.md"));
        let ca = fs::read(&fa).unwrap_or_else(|_| panic!("missing v1 {name}"));
        let cb = fs::read(&fb).unwrap_or_else(|_| panic!("missing v2 {name}"));
        assert_eq!(ca, cb, "{name}.md difere entre .cjs e Rust");
    }
}

// ─── initDefaults em diretório limpo ──────────────────────────────────────

#[test]
fn parity_init_fresh_defaults_match() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);

    call_v1_init(&a, false);
    init_defaults(&b, false).unwrap();

    assert_dirs_match(&a, &b);
}

#[test]
fn parity_init_manifest_trailing_newline() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);

    call_v1_init(&a, false);
    init_defaults(&b, false).unwrap();

    let ja = fs::read(a.join("agents.json")).unwrap();
    let jb = fs::read(b.join("agents.json")).unwrap();
    assert_eq!(ja.last(), Some(&b'\n'));
    assert_eq!(jb.last(), Some(&b'\n'));
    assert_eq!(ja, jb);
}

#[test]
fn parity_init_mini_contexts_no_forced_trailing_newline() {
    // .cjs grava via fs.writeFileSync(p, content) sem adicionar \n;
    // nosso init_defaults também não deve adicionar. Comparamos bytes.
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);

    call_v1_init(&a, false);
    init_defaults(&b, false).unwrap();

    for (name, _) in default_mini_contexts() {
        let fa = a.join("agents").join(format!("{name}.md"));
        let fb = b.join("agents").join(format!("{name}.md"));
        assert_eq!(fs::read(&fa).unwrap(), fs::read(&fb).unwrap());
    }
}

// ─── initDefaults respeitando overwrite=false ─────────────────────────────

#[test]
fn parity_init_preserves_existing_manifest() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);

    // Pre-existente em ambos (idêntico)
    let custom = r#"{"custom":true}"#;
    fs::create_dir_all(a.join("agents")).unwrap();
    fs::create_dir_all(b.join("agents")).unwrap();
    fs::write(a.join("agents.json"), custom).unwrap();
    fs::write(b.join("agents.json"), custom).unwrap();

    call_v1_init(&a, false);
    init_defaults(&b, false).unwrap();

    let ja = fs::read_to_string(a.join("agents.json")).unwrap();
    let jb = fs::read_to_string(b.join("agents.json")).unwrap();
    assert_eq!(ja, custom);
    assert_eq!(jb, custom);
}

#[test]
fn parity_init_preserves_existing_mini_context() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);

    fs::create_dir_all(a.join("agents")).unwrap();
    fs::create_dir_all(b.join("agents")).unwrap();
    let existing = "# Customizado — não me toque";
    fs::write(a.join("agents/manager.md"), existing).unwrap();
    fs::write(b.join("agents/manager.md"), existing).unwrap();

    call_v1_init(&a, false);
    init_defaults(&b, false).unwrap();

    assert_eq!(
        fs::read_to_string(a.join("agents/manager.md")).unwrap(),
        existing
    );
    assert_eq!(
        fs::read_to_string(b.join("agents/manager.md")).unwrap(),
        existing
    );
}

// ─── initDefaults com overwrite=true ──────────────────────────────────────

#[test]
fn parity_init_overwrite_replaces_all() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);

    // Pre-existente diferente
    fs::create_dir_all(a.join("agents")).unwrap();
    fs::create_dir_all(b.join("agents")).unwrap();
    fs::write(a.join("agents.json"), r#"{"x":1}"#).unwrap();
    fs::write(b.join("agents.json"), r#"{"x":1}"#).unwrap();
    fs::write(a.join("agents/manager.md"), "old").unwrap();
    fs::write(b.join("agents/manager.md"), "old").unwrap();

    call_v1_init(&a, true);
    init_defaults(&b, true).unwrap();

    assert_dirs_match(&a, &b);
}

// ─── semântica de retorno ─────────────────────────────────────────────────

#[test]
fn parity_init_returns_manifest() {
    // .cjs retorna `loadManifest(memDir)`; Rust idem.
    // Comparamos via serializar ambos e diff.
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);

    let script = r#"
const lib = require(process.argv[1]);
const memDir = process.argv[2];
const r = lib.initDefaults(memDir, { overwrite: false });
process.stdout.write(JSON.stringify(r));
"#;
    let out = Command::new("node")
        .arg("-e")
        .arg(script)
        .arg(lib_cjs_path())
        .arg(&a)
        .output()
        .expect("spawn");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v1 = String::from_utf8(out.stdout).unwrap();

    let v2 = init_defaults(&b, false).unwrap().unwrap();
    let v2_str = serde_json::to_string(&v2).unwrap();
    assert_eq!(v1, v2_str);
}
