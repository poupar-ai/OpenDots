//! Paridade v1 (`plugin/scripts/lib.cjs`) vs v2 (core-paths):
//! - `findProjectRoot(cwd)`
//! - `getMemDir(cwd)`
//! - `getTmpDir(memDir)`
//!
//! Para cada função compara-se byte-a-byte o retorno do `.cjs` (invocado via
//! `node -e`) contra o da função Rust sobre o mesmo `cwd`.

use core_paths::{find_project_root, mem_dir, tmp_dir};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const lib = require(process.argv[1]);
const fn = process.argv[2];
const arg = process.argv[3];
let out;
switch (fn) {
  case 'findProjectRoot': out = lib.findProjectRoot(arg); break;
  case 'getMemDir':       out = lib.getMemDir(arg); break;
  case 'getTmpDir':       out = lib.getTmpDir(arg); break;
  default: throw new Error('unknown fn: ' + fn);
}
process.stdout.write(out);
"#;

fn lib_cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/lib.cjs")
        .canonicalize()
        .expect("lib.cjs missing — expected at plugin/scripts/lib.cjs")
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
    String::from_utf8(out.stdout).expect("stdout not utf-8")
}

#[test]
fn find_project_root_outside_repo_matches_v1() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let v1 = call_v1("findProjectRoot", tmp.path());
    let v2 = find_project_root(tmp.path()).to_string_lossy().to_string();
    assert_eq!(v1, v2, "findProjectRoot divergiu (fora-de-repo)");
}

#[test]
fn find_project_root_inside_repo_matches_v1() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let status = Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(tmp.path())
        .status();
    if !status.map(|s| s.success()).unwrap_or(false) {
        eprintln!("git indisponível — pulando");
        return;
    }
    let sub = tmp.path().join("x/y");
    std::fs::create_dir_all(&sub).unwrap();
    let v1 = call_v1("findProjectRoot", &sub);
    let v2 = find_project_root(&sub).to_string_lossy().to_string();
    assert_eq!(v1, v2, "findProjectRoot divergiu (dentro-de-repo)");
}

#[test]
fn get_mem_dir_matches_v1() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let v1 = call_v1("getMemDir", tmp.path());
    let v2 = mem_dir(tmp.path()).to_string_lossy().to_string();
    assert_eq!(v1, v2, "getMemDir divergiu");
}

#[test]
fn get_tmp_dir_matches_v1() {
    if !node_available() {
        return;
    }
    let input = PathBuf::from("/var/data/.claude-project-memory");
    let v1 = call_v1("getTmpDir", &input);
    let v2 = tmp_dir(&input).to_string_lossy().to_string();
    assert_eq!(v1, v2, "getTmpDir divergiu");
}

/// Bateria que reproduz a auditoria do Ponto 2: paths patológicos
/// (relativos, `./`, `//`, `..`, vazio) passados para `getTmpDir`.
/// Antes da implementação de `normalize_posix`, Rust divergia em
/// ./mem e /a//b. Manter esses casos bloqueados por paridade.
#[test]
fn get_tmp_dir_edge_cases_match_v1() {
    if !node_available() {
        return;
    }
    let inputs = [
        "a/b",
        "/a/b/",
        "./mem",
        "../mem",
        "/a//b",
        "",
        "/",
        "a/./b",
        "a/b/..",
        ".",
    ];
    for input in inputs {
        let v1 = call_v1("getTmpDir", Path::new(input));
        let v2 = tmp_dir(Path::new(input)).to_string_lossy().to_string();
        assert_eq!(v1, v2, "getTmpDir divergiu para input {:?}", input);
    }
}

#[test]
fn get_mem_dir_inside_repo_matches_v1() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let status = Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(tmp.path())
        .status();
    if !status.map(|s| s.success()).unwrap_or(false) {
        return;
    }
    let sub = tmp.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let v1 = call_v1("getMemDir", &sub);
    let v2 = mem_dir(&sub).to_string_lossy().to_string();
    assert_eq!(v1, v2);
}
