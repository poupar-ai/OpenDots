//! Paridade v1 vs v2 para cada adapter. Seed mem + projectRoot em 2
//! tmp dirs, roda o `.cjs` contra um, chama o crate contra o outro,
//! compara conteúdo de cada arquivo escrito.

use domain_ide_adapters::apply;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const cjsPath = process.argv[1];
const { apply } = require(cjsPath);
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const { ide, memDir, projectRoot } = JSON.parse(input);
    const r = apply({ ide, memDir, projectRoot });
    process.stdout.write(JSON.stringify(r));
  } catch (e) {
    process.stderr.write('THROW: ' + e.message);
    process.exit(3);
  }
});
"#;

fn cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/ide-adapters.cjs")
        .canonicalize()
        .expect("ide-adapters.cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_node(ide: &str, mem_dir: &Path, project_root: &Path) -> Value {
    let input = json!({
        "ide": ide,
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
        panic!("node exited: {}", String::from_utf8_lossy(&out.stderr));
    }
    serde_json::from_slice(&out.stdout).unwrap()
}

fn seed_manifest(mem_dir: &Path, manifest: Value) {
    fs::write(mem_dir.join("agents.json"), manifest.to_string()).unwrap();
}

fn seed_context(mem_dir: &Path, name: &str, content: &str) {
    fs::write(mem_dir.join(name), content).unwrap();
}

/// Compara todos os arquivos em dois diretórios (relativos a root)
/// byte-a-byte. Falha se algum diferir ou se o set de arquivos diferir.
fn diff_trees(a_root: &Path, b_root: &Path, paths: &[String]) {
    let a_set: BTreeSet<&String> = paths.iter().collect();
    let b_set: BTreeSet<&String> = paths.iter().collect();
    assert_eq!(a_set, b_set);
    for p in paths {
        let a = fs::read(a_root.join(p)).unwrap_or_else(|_| panic!("a missing {p}"));
        let b = fs::read(b_root.join(p)).unwrap_or_else(|_| panic!("b missing {p}"));
        assert_eq!(
            a,
            b,
            "bytes differ for {p}\na:\n{}\n---\nb:\n{}",
            String::from_utf8_lossy(&a),
            String::from_utf8_lossy(&b)
        );
    }
}

fn setup_dirs() -> (TempDir, TempDir, TempDir, TempDir) {
    let mem_a = TempDir::new().unwrap();
    let mem_b = TempDir::new().unwrap();
    let proj_a = TempDir::new().unwrap();
    let proj_b = TempDir::new().unwrap();
    (mem_a, mem_b, proj_a, proj_b)
}

fn run_pair(ide: &str, manifest: &Value, extra: &[(&str, &str)]) {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let (mem_a, mem_b, proj_a, proj_b) = setup_dirs();
    seed_manifest(mem_a.path(), manifest.clone());
    seed_manifest(mem_b.path(), manifest.clone());
    for (n, c) in extra {
        seed_context(mem_a.path(), n, c);
        seed_context(mem_b.path(), n, c);
    }
    let r_js = run_node(ide, mem_a.path(), proj_a.path());
    let r_rs = apply(ide, mem_b.path(), proj_b.path()).unwrap();
    let written_js: Vec<String> = r_js["written"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        written_js, r_rs.written,
        "written arrays diverge for ide={ide}"
    );
    diff_trees(proj_a.path(), proj_b.path(), &r_rs.written);
}

fn basic_manifest() -> Value {
    json!({
        "agents": {
            "manager": {
                "label": "Manager",
                "description": "Orquestra agentes",
                "context_file": "ctx_manager.md"
            },
            "worker": {
                "label": "Worker",
                "description": "Faz o trabalho",
                "context_file": "ctx_worker.md"
            }
        }
    })
}

#[test]
fn parity_claude_code() {
    run_pair(
        "claude-code",
        &basic_manifest(),
        &[
            ("ctx_manager.md", "conteúdo manager\n\n"),
            ("ctx_worker.md", "   conteúdo worker   "),
        ],
    );
}

#[test]
fn parity_cursor() {
    run_pair(
        "cursor",
        &basic_manifest(),
        &[
            ("ctx_manager.md", "# runbook manager\n"),
            ("ctx_worker.md", "# runbook worker\n"),
        ],
    );
}

#[test]
fn parity_vscode_copilot() {
    run_pair(
        "vscode-copilot",
        &basic_manifest(),
        &[
            ("ctx_manager.md", "ctx1"),
            ("ctx_worker.md", "ctx2"),
        ],
    );
}

#[test]
fn parity_codex() {
    run_pair(
        "codex",
        &basic_manifest(),
        &[
            ("ctx_manager.md", "ctx1"),
            ("ctx_worker.md", "ctx2"),
        ],
    );
}

#[test]
fn parity_agent_without_context_file() {
    let m = json!({
        "agents": {
            "a": {"label": "A", "description": "desc A"},
            "b": {}
        }
    });
    run_pair("claude-code", &m, &[]);
    run_pair("cursor", &m, &[]);
    run_pair("vscode-copilot", &m, &[]);
    run_pair("codex", &m, &[]);
}

#[test]
fn parity_context_with_trailing_whitespace() {
    // trim() em JS remove \n, \t, spaces. Rust str::trim cobre o mesmo
    // conjunto (ECMAScript WhiteSpace + LineTerminator).
    let m = basic_manifest();
    run_pair(
        "claude-code",
        &m,
        &[
            ("ctx_manager.md", "\n\n\tcontent\t \n\n"),
            ("ctx_worker.md", "  \n  \n  "),
        ],
    );
}

#[test]
fn parity_empty_agents_object() {
    let m = json!({"agents": {}});
    run_pair("claude-code", &m, &[]);
    run_pair("cursor", &m, &[]);
    run_pair("vscode-copilot", &m, &[]);
    run_pair("codex", &m, &[]);
}

#[test]
fn parity_description_with_special_chars() {
    let m = json!({
        "agents": {
            "dev": {
                "label": "Dev",
                "description": "uses **markdown** and `code`",
                "context_file": "ctx.md"
            }
        }
    });
    run_pair("vscode-copilot", &m, &[("ctx.md", "runbook")]);
    run_pair("codex", &m, &[("ctx.md", "runbook")]);
}
