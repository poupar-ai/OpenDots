//! Paridade `parseCatalog` do .cjs (acessado via `listPractices` com
//! um catalog temporário) vs `parse_catalog` do Rust.
//!
//! O .cjs não exporta `parseCatalog` direto; testamos via `listPractices`
//! num `bp_dir` temp.

use domain_best_practices::parse_catalog;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
// Args: [cjsPath, bpDir]
const cjsPath = process.argv[1];
const bpDirOverride = process.argv[2];

// Monkey-patch __dirname override: recarregar o módulo com BP_DIR diferente
// via substituir require cache após load.
const path = require('path');
const origJoin = path.join;
const origDirname = path.dirname;
// Truque: o .cjs usa `path.join(__dirname, '..', 'seeds')` e
// `path.join(__dirname, '..', 'best-practices')`. Queremos que a última
// expressão resolva para bpDirOverride. Hack simples: monkey-patch
// require da fs.existsSync + fs.readFileSync pra substituir o arquivo
// esperado. Mais seguro: reescrever o módulo em runtime.

// Estratégia limpa: carregar o .cjs como string, injetar uma linha que
// redefine BP_DIR, e avaliar.
const fs = require('fs');
const src = fs.readFileSync(cjsPath, 'utf8');
const patched = src.replace(
  /const BP_DIR\s*=\s*[^;]+;/,
  `const BP_DIR = ${JSON.stringify(bpDirOverride)};`
);
const m = { exports: {} };
const wrapper = new Function('module', 'exports', 'require', '__dirname', '__filename', patched);
wrapper(m, m.exports, require, path.dirname(cjsPath), cjsPath);

process.stdout.write(JSON.stringify(m.exports.listPractices()));
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

fn call_v1_list(bp_dir: &std::path::Path) -> String {
    let out = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(lib_cjs_path())
        .arg(bp_dir.to_str().unwrap())
        .output()
        .expect("spawn node");
    assert!(
        out.status.success(),
        "node falhou: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8")
}

fn write_catalog(bp: &std::path::Path, body: &str) {
    fs::create_dir_all(bp).unwrap();
    fs::write(bp.join("_catalog.yaml"), body).unwrap();
}

fn assert_parity(bp: &std::path::Path) {
    if !node_available() {
        return;
    }
    let v1 = call_v1_list(bp);
    let v2 = domain_best_practices::list_practices(bp);
    let v2_s = serde_json::to_string(&Value::Array(
        v2.into_iter().map(Value::Object).collect(),
    ))
    .unwrap();
    assert_eq!(v1, v2_s);
}

#[test]
fn real_catalog_parity() {
    if !node_available() {
        return;
    }
    let bp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/best-practices")
        .canonicalize()
        .expect("plugin/best-practices missing");
    assert_parity(&bp);
}

#[test]
fn empty_catalog() {
    let tmp = TempDir::new().unwrap();
    write_catalog(tmp.path(), "");
    assert_parity(tmp.path());
}

#[test]
fn comments_only() {
    let tmp = TempDir::new().unwrap();
    write_catalog(
        tmp.path(),
        "# top comment\n# another\nversion: 1\n# trailing\n",
    );
    assert_parity(tmp.path());
}

#[test]
fn single_practice_no_multiline() {
    let tmp = TempDir::new().unwrap();
    write_catalog(
        tmp.path(),
        "practices:\n  - id: foo\n    name: Foo\n    scope: s\n    tags: [a, b]\n",
    );
    assert_parity(tmp.path());
}

#[test]
fn multi_line_block() {
    let tmp = TempDir::new().unwrap();
    write_catalog(
        tmp.path(),
        "practices:\n  - id: foo\n    when_to_use: |\n      line 1\n      line 2\n    scope: s\n",
    );
    assert_parity(tmp.path());
}

#[test]
fn quote_strip() {
    let tmp = TempDir::new().unwrap();
    write_catalog(
        tmp.path(),
        "practices:\n  - id: foo\n    name: \"Quoted\"\n    note: 'single'\n",
    );
    assert_parity(tmp.path());
}

#[test]
fn multiple_practices() {
    let tmp = TempDir::new().unwrap();
    write_catalog(
        tmp.path(),
        "practices:\n  - id: foo\n    name: Foo\n  - id: bar\n    name: Bar\n",
    );
    assert_parity(tmp.path());
}

#[test]
fn crlf_line_endings() {
    let tmp = TempDir::new().unwrap();
    write_catalog(
        tmp.path(),
        "practices:\r\n  - id: foo\r\n    name: Foo\r\n",
    );
    assert_parity(tmp.path());
}

#[test]
fn multi_line_then_field() {
    // Bloco termina quando a próxima linha tem indent menor que multiIndent.
    let tmp = TempDir::new().unwrap();
    write_catalog(
        tmp.path(),
        "practices:\n  - id: foo\n    when_to_use: |\n      a\n      b\n    tags: [x]\n",
    );
    assert_parity(tmp.path());
}

#[test]
fn unit_parse_catalog_rust_standalone() {
    // Sanity sem o Node: só para verificar que parse_catalog sobrevive
    // sem state compartilhado entre chamadas.
    let yaml = "practices:\n  - id: a\n  - id: b\n";
    let r = parse_catalog(yaml);
    assert_eq!(r.len(), 2);
    assert_eq!(r[0]["id"], "a");
    assert_eq!(r[1]["id"], "b");
}
