//! Paridade de `getContextLines` e `rankSymbols` — exported do .cjs.

use domain_indexer::{get_context_lines, rank_symbols, SystemClock, Clock};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const cjsPath = process.argv[1];
const { getContextLines, rankSymbols } = require(cjsPath);
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const { op, memDir, maxSymbols, index, graph } = JSON.parse(input);
    let r;
    if (op === 'getContextLines') {
      r = getContextLines(memDir, maxSymbols);
    } else if (op === 'rankSymbols') {
      r = rankSymbols(index, graph);
    }
    process.stdout.write(JSON.stringify(r));
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

fn run_node_json(input: Value) -> Value {
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
    serde_json::from_slice(&out.stdout).unwrap()
}

fn seed(mem_dir: &Path, index: &Value, graph: Option<&Value>) {
    fs::write(
        mem_dir.join("code_index.json"),
        serde_json::to_string(index).unwrap(),
    )
    .unwrap();
    if let Some(g) = graph {
        fs::write(mem_dir.join("graph.json"), serde_json::to_string(g).unwrap()).unwrap();
    }
}

#[test]
fn parity_get_context_lines_with_hot_files() {
    if !node_available() {
        return;
    }
    let index = json!({
        "version": 1,
        "generated_at": "2026-01-01T00:00:00.000Z",
        "files": {
            "src/a.py": {
                "lang": "python",
                "mtime": 1_700_000_000_000_i64,
                "symbols": [
                    {"type": "fn", "name": "alpha", "sig": "def alpha()", "line": 1, "refs": 3},
                    {"type": "class", "name": "Beta", "sig": "class Beta", "line": 5, "refs": 0}
                ]
            },
            "src/b.ts": {
                "lang": "typescript",
                "mtime": 1_700_000_000_000_i64,
                "symbols": [
                    {"type": "fn", "name": "gamma", "sig": "function gamma()", "line": 2, "refs": 10, "exported": true}
                ]
            }
        }
    });
    let graph = json!({
        "nodes": {
            "file:src/a.py": {"kind": "file", "label": "src/a.py", "count": 5},
            "file:src/b.ts": {"kind": "file", "label": "src/b.ts", "count": 2}
        }
    });

    let mem_js = TempDir::new().unwrap();
    let mem_rs = TempDir::new().unwrap();
    seed(mem_js.path(), &index, Some(&graph));
    seed(mem_rs.path(), &index, Some(&graph));

    let js_out = run_node_json(json!({
        "op": "getContextLines",
        "memDir": mem_js.path().to_string_lossy(),
        "maxSymbols": 25
    }));
    let rs_out = get_context_lines(mem_rs.path(), 25);

    let js_lines: Vec<String> = js_out
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(js_lines, rs_out);
}

#[test]
fn parity_get_context_lines_no_graph() {
    if !node_available() {
        return;
    }
    let index = json!({
        "version": 1,
        "generated_at": "2026-01-01T00:00:00.000Z",
        "files": {
            "src/x.py": {
                "lang": "python",
                "mtime": 1_700_000_000_000_i64,
                "symbols": [
                    {"type": "fn", "name": "foo_bar", "sig": "def foo_bar()", "line": 1, "refs": 2}
                ]
            }
        }
    });

    let mem_js = TempDir::new().unwrap();
    let mem_rs = TempDir::new().unwrap();
    seed(mem_js.path(), &index, None);
    seed(mem_rs.path(), &index, None);

    let js_out = run_node_json(json!({
        "op": "getContextLines",
        "memDir": mem_js.path().to_string_lossy(),
        "maxSymbols": 10
    }));
    let rs_out = get_context_lines(mem_rs.path(), 10);

    let js_lines: Vec<String> = js_out
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(js_lines, rs_out);
}

#[test]
fn parity_get_context_lines_empty_index() {
    if !node_available() {
        return;
    }
    let index = json!({
        "version": 1,
        "generated_at": "2026-01-01T00:00:00.000Z",
        "files": {}
    });
    let mem_js = TempDir::new().unwrap();
    let mem_rs = TempDir::new().unwrap();
    seed(mem_js.path(), &index, None);
    seed(mem_rs.path(), &index, None);

    let js_out = run_node_json(json!({
        "op": "getContextLines",
        "memDir": mem_js.path().to_string_lossy(),
        "maxSymbols": 25
    }));
    let rs_out = get_context_lines(mem_rs.path(), 25);
    let js_lines: Vec<String> = js_out
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(js_lines, rs_out);
}

#[test]
fn parity_rank_symbols_basic() {
    if !node_available() {
        return;
    }
    let index = json!({
        "version": 1,
        "generated_at": "2026-01-01T00:00:00.000Z",
        "files": {
            "src/a.py": {
                "lang": "python",
                "mtime": 1_700_000_000_000_i64,
                "symbols": [
                    {"type": "fn", "name": "alpha", "sig": "def alpha()", "line": 1, "refs": 3},
                    {"type": "fn", "name": "delta", "sig": "def delta()", "line": 10, "refs": 1}
                ]
            },
            "src/b.ts": {
                "lang": "typescript",
                "mtime": 1_700_000_000_000_i64,
                "symbols": [
                    {"type": "fn", "name": "gamma", "sig": "function gamma()", "line": 2, "refs": 10, "exported": true}
                ]
            }
        }
    });
    let graph = json!({
        "nodes": {
            "file:src/a.py": {"kind": "file", "label": "src/a.py", "count": 5}
        }
    });

    let js_out = run_node_json(json!({
        "op": "rankSymbols",
        "index": index,
        "graph": graph
    }));

    // rank_symbols depends on `now_ms` — read it from SystemClock at the same
    // time both will use (they use the real clock within ms of each other).
    // We compare names/types/files sequences + scores (within epsilon).
    let now_ms = SystemClock.now_ms();
    let rs_out = rank_symbols(&index, Some(&graph), now_ms);

    assert_eq!(
        js_out.as_array().unwrap().len(),
        rs_out.len(),
        "rank_symbols length differs"
    );
    for (js_s, rs_s) in js_out.as_array().unwrap().iter().zip(rs_out.iter()) {
        assert_eq!(js_s["name"], rs_s["name"]);
        assert_eq!(js_s["file"], rs_s["file"]);
        assert_eq!(js_s["type"], rs_s["type"]);
        let js_score = js_s["score"].as_f64().unwrap();
        let rs_score = rs_s["score"].as_f64().unwrap();
        assert!(
            (js_score - rs_score).abs() < 1e-6,
            "score divergence for {}: js={} rs={}",
            js_s["name"],
            js_score,
            rs_score
        );
    }
}
