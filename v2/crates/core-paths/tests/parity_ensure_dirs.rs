//! Paridade v1 vs v2 para `ensureMemDir` / `ensureTmpDir`.
//!
//! Estratégia: criar dois diretórios irmãos temporários e chamar:
//!   - v1 via `node -e` carregando `lib.cjs` sobre o primeiro
//!   - v2 direto via Rust sobre o segundo
//! Depois comparar a listagem de entradas relativas entre os dois —
//! se `.cjs` e Rust criaram a mesma estrutura, `BTreeSet<PathBuf>` bate.

use core_paths::{ensure_mem_dir, ensure_tmp_dir, tmp_dir};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const lib = require(process.argv[1]);
const fn = process.argv[2];
const arg = process.argv[3];
let out;
switch (fn) {
  case 'ensureMemDir': lib.ensureMemDir(arg); out = ''; break;
  case 'ensureTmpDir': out = String(lib.ensureTmpDir(arg)); break;
  default: throw new Error('unknown fn: ' + fn);
}
process.stdout.write(out);
"#;

fn lib_cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/lib.cjs")
        .canonicalize()
        .expect("lib.cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn call_v1(fn_name: &str, arg: &Path) -> String {
    let out = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(lib_cjs_path())
        .arg(fn_name)
        .arg(arg)
        .output()
        .expect("failed to spawn node");
    assert!(
        out.status.success(),
        "node exited {:?}, stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// Walk recursivo de entradas relativas a `root`, incluindo diretórios.
/// Se `root` não existe retorna set vazio.
fn list_tree(root: &Path) -> BTreeSet<PathBuf> {
    fn walk(base: &Path, cur: &Path, acc: &mut BTreeSet<PathBuf>) {
        let rd = match std::fs::read_dir(cur) {
            Ok(r) => r,
            Err(_) => return,
        };
        for e in rd.flatten() {
            let p = e.path();
            let rel = p.strip_prefix(base).unwrap().to_path_buf();
            acc.insert(rel.clone());
            if p.is_dir() {
                walk(base, &p, acc);
            }
        }
    }
    let mut set = BTreeSet::new();
    if root.is_dir() {
        walk(root, root, &mut set);
    }
    set
}

#[test]
fn ensure_mem_dir_creates_same_tree() {
    if !node_available() {
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let mem_a = a.path().join("mem");
    let mem_b = b.path().join("mem");

    call_v1("ensureMemDir", &mem_a);
    ensure_mem_dir(&mem_b).unwrap();

    assert!(mem_a.is_dir(), "v1 não criou memDir");
    assert!(mem_b.is_dir(), "v2 não criou memDir");
    assert_eq!(list_tree(a.path()), list_tree(b.path()));
}

#[test]
fn ensure_mem_dir_idempotent_matches_v1() {
    if !node_available() {
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let mem_a = a.path().join("m");
    let mem_b = b.path().join("m");

    // pré-cria nos dois — deve ser no-op nos dois lados
    std::fs::create_dir_all(&mem_a).unwrap();
    std::fs::create_dir_all(&mem_b).unwrap();

    call_v1("ensureMemDir", &mem_a);
    ensure_mem_dir(&mem_b).unwrap();

    assert_eq!(list_tree(a.path()), list_tree(b.path()));
}

#[test]
fn ensure_tmp_dir_creates_same_tree_and_returns_same_path() {
    if !node_available() {
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let mem_a = a.path().join("mem");
    let mem_b = b.path().join("mem");
    std::fs::create_dir_all(&mem_a).unwrap();
    std::fs::create_dir_all(&mem_b).unwrap();

    let v1_ret = call_v1("ensureTmpDir", &mem_a);
    let v2_ret = ensure_tmp_dir(&mem_b).unwrap().to_string_lossy().to_string();

    // retornos: v1 devolve string do path, rust devolve PathBuf — comparar
    // via offset do memDir (ambos devem apontar para <mem>/tmp)
    assert_eq!(v1_ret, mem_a.join("tmp").to_string_lossy());
    assert_eq!(v2_ret, mem_b.join("tmp").to_string_lossy());

    assert_eq!(list_tree(a.path()), list_tree(b.path()));
}

#[test]
fn ensure_tmp_dir_creates_parent_if_missing_same_as_v1() {
    if !node_available() {
        return;
    }
    // cenário: memDir não existe antes — fs.mkdirSync recursive cria tudo.
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let mem_a = a.path().join("missing/m");
    let mem_b = b.path().join("missing/m");

    call_v1("ensureTmpDir", &mem_a);
    ensure_tmp_dir(&mem_b).unwrap();

    assert!(tmp_dir(&mem_a).is_dir());
    assert!(tmp_dir(&mem_b).is_dir());
    assert_eq!(list_tree(a.path()), list_tree(b.path()));
}
