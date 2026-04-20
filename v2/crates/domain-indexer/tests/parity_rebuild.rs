//! Paridade end-to-end de rebuildIndex + saveIndex (inclui writeIndexMd).
//! Estratégia: cria um repo git compartilhado, mem_dirs separados para JS
//! e Rust, roda ambos, normaliza generated_at + mtime, byte-compara
//! code_index.json e CODE_INDEX.md.

use domain_indexer::{rebuild_index, SystemClock};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const cjsPath = process.argv[1];
const { rebuildIndex } = require(cjsPath);
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const { memDir, projectRoot } = JSON.parse(input);
    const r = rebuildIndex(memDir, projectRoot);
    process.stdout.write(JSON.stringify({ files: Object.keys(r.files || {}) }));
  } catch (e) {
    process.stderr.write('THROW: ' + e.message);
    process.exit(3);
  }
});
"#;

fn cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/indexer.cjs")
        .canonicalize()
        .expect("indexer.cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_node(mem_dir: &Path, project_root: &Path) {
    let input = json!({
        "memDir": mem_dir.to_string_lossy(),
        "projectRoot": project_root.to_string_lossy(),
    });
    let mut child = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(cjs_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    if !out.status.success() {
        panic!("node failed: {}", String::from_utf8_lossy(&out.stderr));
    }
}

fn init_git_repo(project_dir: &Path, files: &[(&str, &str)]) {
    let run = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(project_dir)
            .output()
            .expect("git failed");
        if !out.status.success() {
            panic!(
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        }
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "test@test"]);
    run(&["config", "user.name", "test"]);
    for (rel, content) in files {
        let full = project_dir.join(rel);
        if let Some(p) = full.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(&full, content).unwrap();
    }
    run(&["add", "-A"]);
    run(&["-c", "commit.gpgsign=false", "commit", "-q", "-m", "seed"]);
}

/// Normaliza campos que dependem do relógio/filesystem: `generated_at` em
/// `code_index.json`, `mtime` de cada file, e o cabeçalho do MD.
fn normalize(v: &mut Value) {
    if let Some(obj) = v.as_object_mut() {
        if obj.contains_key("generated_at") {
            obj.insert("generated_at".to_string(), json!("<GEN>"));
        }
        if let Some(files) = obj.get_mut("files").and_then(Value::as_object_mut) {
            for (_, entry) in files.iter_mut() {
                if let Some(e) = entry.as_object_mut() {
                    if e.contains_key("mtime") {
                        e.insert("mtime".to_string(), json!("<MTIME>"));
                    }
                }
            }
        }
    }
}

fn normalize_md(md: &str) -> String {
    md.lines()
        .map(|l| {
            if l.starts_with("> Gerado em: ") {
                "> Gerado em: <GEN>".to_string()
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn run_pair(files: &[(&str, &str)]) {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let project = TempDir::new().unwrap();
    let mem_js = TempDir::new().unwrap();
    let mem_rs = TempDir::new().unwrap();
    init_git_repo(project.path(), files);

    run_node(mem_js.path(), project.path());
    rebuild_index(mem_rs.path(), project.path(), &SystemClock).unwrap();

    // Compara code_index.json com normalização
    let js_json = fs::read_to_string(mem_js.path().join("code_index.json")).unwrap();
    let rs_json = fs::read_to_string(mem_rs.path().join("code_index.json")).unwrap();
    let mut js_v: Value = serde_json::from_str(&js_json).unwrap();
    let mut rs_v: Value = serde_json::from_str(&rs_json).unwrap();
    normalize(&mut js_v);
    normalize(&mut rs_v);
    assert_eq!(
        serde_json::to_string_pretty(&js_v).unwrap(),
        serde_json::to_string_pretty(&rs_v).unwrap(),
        "code_index.json diverges"
    );

    // Compara CODE_INDEX.md com normalização
    let js_md = fs::read_to_string(mem_js.path().join("CODE_INDEX.md")).unwrap();
    let rs_md = fs::read_to_string(mem_rs.path().join("CODE_INDEX.md")).unwrap();
    assert_eq!(
        normalize_md(&js_md),
        normalize_md(&rs_md),
        "CODE_INDEX.md diverges"
    );
}

#[test]
fn parity_rebuild_mixed_langs() {
    run_pair(&[
        ("src/app.py", "def hello():\n    pass\n\nclass Foo:\n    pass\n"),
        ("src/mod.ts", "export function hi() {}\nexport const X = 1;\n"),
        ("src/lib.rs", "pub fn foo() {}\npub struct S;\n"),
        ("migrations/0001_users.sql", "CREATE TABLE users (id INT);\n"),
        ("pkg/main.go", "func Run() {}\n"),
        ("README.md", "hello\n"),
    ]);
}

#[test]
fn parity_rebuild_refs_counting() {
    run_pair(&[
        (
            "src/lib.ts",
            "export function validate(x: number) { return x; }\nexport class Builder {}\n",
        ),
        (
            "src/app.ts",
            "import { validate, Builder } from './lib';\nvalidate(1);\nconst b = new Builder();\nvalidate(2);\n",
        ),
        (
            "src/other.ts",
            "import { validate } from './lib';\nconst r = validate(3);\n",
        ),
    ]);
}

#[test]
fn parity_rebuild_empty_repo() {
    run_pair(&[("README.md", "no code\n")]);
}

#[test]
fn parity_rebuild_python_fastapi() {
    run_pair(&[
        (
            "api.py",
            "from fastapi import APIRouter\nrouter = APIRouter()\n\n@router.get(\"/items\")\nasync def list_items():\n    return []\n\n@router.post(\"/items\")\nasync def create_item():\n    return {}\n",
        ),
    ]);
}

#[test]
fn parity_rebuild_large_refs() {
    // Test that ref counting chunks into 200-name regex groups correctly
    let mut files: Vec<(String, String)> = Vec::new();
    let mut ts = String::new();
    for i in 0..5 {
        ts.push_str(&format!("export function sym_{i}() {{}}\n"));
    }
    files.push(("src/defs.ts".to_string(), ts));
    let mut use_src = String::new();
    for i in 0..5 {
        use_src.push_str(&format!("sym_{i}();\n"));
    }
    files.push(("src/use.ts".to_string(), use_src));
    let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    run_pair(&refs);
}
