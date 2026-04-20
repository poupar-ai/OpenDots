//! Paridade `applyPractices` + `loadPracticeContent` vs Rust.

use domain_best_practices::{apply_practices, load_practice_content};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const cjsPath = process.argv[1];
const bpDir = process.argv[2];
const op = process.argv[3];
const arg1 = process.argv[4] || '';
const arg2 = process.argv[5] || '';

const fs = require('fs');
const path = require('path');
const src = fs.readFileSync(cjsPath, 'utf8');
const patched = src.replace(
  /const BP_DIR\s*=\s*[^;]+;/,
  `const BP_DIR = ${JSON.stringify(bpDir)};`
);
const m = { exports: {} };
const wrapper = new Function('module', 'exports', 'require', '__dirname', '__filename', patched);
wrapper(m, m.exports, require, path.dirname(cjsPath), cjsPath);

if (op === 'apply') {
  const memDir = arg1;
  const ids = JSON.parse(arg2);
  const overwrite = process.argv[6] === 'true';
  const out = m.exports.applyPractices({ ids, memDir, overwrite });
  process.stdout.write(JSON.stringify(out));
} else if (op === 'load') {
  const id = arg1;
  const out = m.exports.loadPracticeContent(id);
  if (out === null) {
    process.stdout.write('__NULL__');
  } else {
    process.stdout.write(out);
  }
} else {
  process.stderr.write('op desconhecida: ' + op);
  process.exit(2);
}
"#;

fn lib_cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/best-practices.cjs")
        .canonicalize()
        .expect(".cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn call_apply(bp: &Path, mem: &Path, ids: &[&str], overwrite: bool) -> String {
    let ids_json = serde_json::to_string(ids).unwrap();
    let out = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(lib_cjs_path())
        .arg(bp.to_str().unwrap())
        .arg("apply")
        .arg(mem.to_str().unwrap())
        .arg(&ids_json)
        .arg(overwrite.to_string())
        .output()
        .expect("spawn node");
    assert!(
        out.status.success(),
        "node falhou: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8")
}

fn call_load(bp: &Path, id: &str) -> Option<String> {
    let out = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(lib_cjs_path())
        .arg(bp.to_str().unwrap())
        .arg("load")
        .arg(id)
        .output()
        .expect("spawn node");
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).expect("utf8");
    if s == "__NULL__" {
        None
    } else {
        Some(s)
    }
}

fn setup_catalog(bp: &Path) {
    fs::create_dir_all(bp.join("foo")).unwrap();
    fs::create_dir_all(bp.join("bar")).unwrap();
    fs::write(
        bp.join("_catalog.yaml"),
        "practices:\n  - id: foo\n    name: Foo\n    scope: s\n    tags: [a, b]\n    when_to_use: |\n      always\n    dir: foo\n  - id: bar\n    name: Bar\n    dir: bar\n",
    )
    .unwrap();
    fs::write(bp.join("foo").join("practice.md"), "foo body").unwrap();
    fs::write(bp.join("bar").join("practice.md"), "bar body").unwrap();
}

fn twin_mem(tmp: &TempDir) -> (PathBuf, PathBuf) {
    let a = tmp.path().join("mem-a");
    let b = tmp.path().join("mem-b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    (a, b)
}

fn compare_trees(a: &Path, b: &Path) {
    let la = walk(a);
    let lb = walk(b);
    assert_eq!(la, lb, "tree diff");
    for rel in &la {
        let ca = fs::read(a.join(rel)).unwrap();
        let cb = fs::read(b.join(rel)).unwrap();
        assert_eq!(
            ca,
            cb,
            "diff em {rel:?}\nA:\n{}\nB:\n{}",
            String::from_utf8_lossy(&ca),
            String::from_utf8_lossy(&cb)
        );
    }
}

fn walk(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !root.exists() {
        return out;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        for entry in fs::read_dir(&p).unwrap() {
            let e = entry.unwrap();
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path.strip_prefix(root).unwrap().to_path_buf());
            }
        }
    }
    out.sort();
    out
}

#[test]
fn apply_basic() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let bp = tmp.path().join("bp");
    setup_catalog(&bp);
    let (ma, mb) = twin_mem(&tmp);
    let v1 = call_apply(&bp, &ma, &["foo", "bar"], false);
    let r = apply_practices(&bp, &mb, &["foo", "bar"], false).unwrap();
    let v2 = serde_json::to_string(&serde_json::json!({
        "copied": r.copied,
        "preserved": r.preserved,
        "missing": r.missing,
    }))
    .unwrap();
    assert_eq!(v1, v2);
    compare_trees(&ma, &mb);
}

#[test]
fn apply_preserved() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let bp = tmp.path().join("bp");
    setup_catalog(&bp);
    let (ma, mb) = twin_mem(&tmp);
    for m in [&ma, &mb] {
        fs::create_dir_all(m.join("best-practices/foo")).unwrap();
        fs::write(m.join("best-practices/foo/practice.md"), "CUSTOM").unwrap();
    }
    let v1 = call_apply(&bp, &ma, &["foo"], false);
    let r = apply_practices(&bp, &mb, &["foo"], false).unwrap();
    let v2 = serde_json::to_string(&serde_json::json!({
        "copied": r.copied,
        "preserved": r.preserved,
        "missing": r.missing,
    }))
    .unwrap();
    assert_eq!(v1, v2);
    compare_trees(&ma, &mb);
}

#[test]
fn apply_overwrite() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let bp = tmp.path().join("bp");
    setup_catalog(&bp);
    let (ma, mb) = twin_mem(&tmp);
    for m in [&ma, &mb] {
        fs::create_dir_all(m.join("best-practices/foo")).unwrap();
        fs::write(m.join("best-practices/foo/practice.md"), "CUSTOM").unwrap();
    }
    let v1 = call_apply(&bp, &ma, &["foo"], true);
    let r = apply_practices(&bp, &mb, &["foo"], true).unwrap();
    let v2 = serde_json::to_string(&serde_json::json!({
        "copied": r.copied,
        "preserved": r.preserved,
        "missing": r.missing,
    }))
    .unwrap();
    assert_eq!(v1, v2);
    compare_trees(&ma, &mb);
}

#[test]
fn apply_missing_ids() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let bp = tmp.path().join("bp");
    setup_catalog(&bp);
    let (ma, mb) = twin_mem(&tmp);
    let v1 = call_apply(&bp, &ma, &["ghost", "foo"], false);
    let r = apply_practices(&bp, &mb, &["ghost", "foo"], false).unwrap();
    let v2 = serde_json::to_string(&serde_json::json!({
        "copied": r.copied,
        "preserved": r.preserved,
        "missing": r.missing,
    }))
    .unwrap();
    assert_eq!(v1, v2);
    compare_trees(&ma, &mb);
}

#[test]
fn load_content_existing() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let bp = tmp.path().join("bp");
    setup_catalog(&bp);
    let v1 = call_load(&bp, "foo");
    let v2 = load_practice_content(&bp, "foo");
    assert_eq!(v1, v2);
}

#[test]
fn load_content_missing() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let bp = tmp.path().join("bp");
    setup_catalog(&bp);
    let v1 = call_load(&bp, "ghost");
    let v2 = load_practice_content(&bp, "ghost");
    assert_eq!(v1, v2);
}

#[test]
fn load_content_no_catalog() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let bp = tmp.path().join("bp");
    fs::create_dir_all(&bp).unwrap();
    let v1 = call_load(&bp, "foo");
    let v2 = load_practice_content(&bp, "foo");
    assert_eq!(v1, v2);
}

#[test]
fn apply_real_catalog_accessibility() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let bp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/best-practices")
        .canonicalize()
        .unwrap();
    let (ma, mb) = twin_mem(&tmp);
    let v1 = call_apply(&bp, &ma, &["accessibility"], false);
    let r = apply_practices(&bp, &mb, &["accessibility"], false).unwrap();
    let v2 = serde_json::to_string(&serde_json::json!({
        "copied": r.copied,
        "preserved": r.preserved,
        "missing": r.missing,
    }))
    .unwrap();
    assert_eq!(v1, v2);
    compare_trees(&ma, &mb);
}
