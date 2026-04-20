//! Paridade v1 (`architect.cjs::ensureInstalled`) vs v2.

use domain_architect::ensure_installed;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const lib = require(process.argv[1]);
const memDir = process.argv[2];
const out = lib.ensureInstalled({ memDir });
process.stdout.write(JSON.stringify(out));
"#;

fn lib_cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/architect.cjs")
        .canonicalize()
        .expect("architect.cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn call_v1(mem: &Path) -> String {
    let out = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(lib_cjs_path())
        .arg(mem.to_str().unwrap())
        .output()
        .expect("spawn node");
    assert!(
        out.status.success(),
        "node falhou: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8")
}

fn twin_dirs(tmp: &TempDir) -> (PathBuf, PathBuf) {
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    (a, b)
}

fn compare_disks(a: &Path, b: &Path) {
    let ja = fs::read(a.join("agents.json")).unwrap();
    let jb = fs::read(b.join("agents.json")).unwrap();
    assert_eq!(
        ja,
        jb,
        "agents.json diverge\nV1:\n{}\nV2:\n{}",
        String::from_utf8_lossy(&ja),
        String::from_utf8_lossy(&jb)
    );
    let ma = fs::read(a.join("agents").join("architect-meta.md")).unwrap();
    let mb = fs::read(b.join("agents").join("architect-meta.md")).unwrap();
    assert_eq!(ma, mb, "architect-meta.md diverge");
}

#[test]
fn ensure_creates_from_scratch() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    call_v1(&a);
    ensure_installed(&b).unwrap();
    compare_disks(&a, &b);
}

#[test]
fn ensure_adds_to_existing_manifest() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let raw = r#"{
  "version": 1,
  "manager": "manager",
  "oracle": "oracle",
  "agents": {
    "existing": {
      "category": "specialist"
    }
  }
}
"#;
    fs::write(a.join("agents.json"), raw).unwrap();
    fs::write(b.join("agents.json"), raw).unwrap();
    call_v1(&a);
    ensure_installed(&b).unwrap();
    compare_disks(&a, &b);
}

#[test]
fn ensure_is_idempotent_vs_v1() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    call_v1(&a);
    ensure_installed(&b).unwrap();
    call_v1(&a);
    ensure_installed(&b).unwrap();
    compare_disks(&a, &b);
}

#[test]
fn ensure_preserves_custom_context() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    fs::create_dir_all(a.join("agents")).unwrap();
    fs::create_dir_all(b.join("agents")).unwrap();
    fs::write(a.join("agents").join("architect-meta.md"), "CUSTOM").unwrap();
    fs::write(b.join("agents").join("architect-meta.md"), "CUSTOM").unwrap();
    call_v1(&a);
    ensure_installed(&b).unwrap();
    compare_disks(&a, &b);
}

#[test]
fn ensure_skips_overwrite_when_architect_already_meta() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    // architect já lá, mesma category. .cjs pula write.
    let raw = r#"{
  "version": 1,
  "agents": {
    "architect": {
      "category": "meta",
      "label": "Arquiteto customizado"
    }
  }
}
"#;
    fs::write(a.join("agents.json"), raw).unwrap();
    fs::write(b.join("agents.json"), raw).unwrap();
    call_v1(&a);
    ensure_installed(&b).unwrap();
    compare_disks(&a, &b);
}
