//! Paridade v1 vs v2 para `getContextLines`. Seed um grafo mockado,
//! chama nos dois lados, compara string final.

use domain_graph::get_context_lines;
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const FIXED_MS: i64 = 1_776_600_000_000;

const NODE_SCRIPT: &str = r#"
const cjsPath = process.argv[1];
const msFixed = Number(process.argv[3]);
const _Orig = Date;
function DateShim(...args) {
  if (!new.target) return _Orig();
  if (args.length === 0) return new _Orig(msFixed);
  return new _Orig(...args);
}
DateShim.prototype = _Orig.prototype;
DateShim.now = () => msFixed;
DateShim.UTC = _Orig.UTC.bind(_Orig);
DateShim.parse = _Orig.parse.bind(_Orig);
global.Date = DateShim;

const { getContextLines } = require(cjsPath);
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const { graph } = JSON.parse(input);
    const out = getContextLines(graph).join('\n');
    process.stdout.write(out);
  } catch (e) {
    process.stderr.write('THROW: ' + e.message + '\n' + e.stack);
    process.exit(3);
  }
});
"#;

fn cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/graph.cjs")
        .canonicalize()
        .expect("graph.cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_node(graph: &Value) -> String {
    let input = json!({"graph": graph});
    let mut child = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(cjs_path())
        .arg("unused")
        .arg(FIXED_MS.to_string())
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
    String::from_utf8(out.stdout).unwrap()
}

fn build_graph(nodes: Value, edges: Value) -> Value {
    json!({
        "version": 2,
        "rebuilt_at": null,
        "updated_at": null,
        "nodes": nodes,
        "edges": edges,
    })
}

#[test]
fn context_empty_graph() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let g = build_graph(json!({}), json!({}));
    let a = run_node(&g);
    let b = get_context_lines(&g, FIXED_MS).join("\n");
    assert_eq!(a, b);
}

#[test]
fn context_top_files() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let nodes = json!({
        "file:src/a.rs": {"kind":"file","label":"src/a.rs","count":5,"last_seen":"2026-04-19T11:55:00.000Z"},
        "file:src/b.rs": {"kind":"file","label":"src/b.rs","count":3,"last_seen":"2026-04-19T11:50:00.000Z"},
        "file:src/c.rs": {"kind":"file","label":"src/c.rs","count":1,"last_seen":"2026-04-19T11:50:00.000Z"},
    });
    let g = build_graph(nodes, json!({}));
    let a = run_node(&g);
    let b = get_context_lines(&g, FIXED_MS).join("\n");
    assert_eq!(a, b);
}

#[test]
fn context_with_co_edits_and_handoffs() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let nodes = json!({
        "file:src/a.rs": {"kind":"file","label":"src/a.rs","count":5,"last_seen":"2026-04-19T11:55:00.000Z"},
        "file:src/b.rs": {"kind":"file","label":"src/b.rs","count":5,"last_seen":"2026-04-19T11:55:00.000Z"},
        "agent:alpha": {"kind":"agent","label":"alpha","count":5,"last_seen":"2026-04-19T11:55:00.000Z"},
        "agent:beta":  {"kind":"agent","label":"beta","count":5,"last_seen":"2026-04-19T11:55:00.000Z"},
    });
    let edges = json!({
        "file:src/a.rs→file:src/b.rs→co_edited": {
            "from":"file:src/a.rs","to":"file:src/b.rs","rel":"co_edited",
            "count":3,"last_seen":"2026-04-19T11:55:00.000Z"
        },
        "file:src/b.rs→file:src/a.rs→co_edited": {
            "from":"file:src/b.rs","to":"file:src/a.rs","rel":"co_edited",
            "count":3,"last_seen":"2026-04-19T11:55:00.000Z"
        },
        "agent:alpha→agent:beta→handoff": {
            "from":"agent:alpha","to":"agent:beta","rel":"handoff",
            "count":4,"last_seen":"2026-04-19T11:55:00.000Z",
            "intents": {"ask":3,"ack":1}
        },
    });
    let g = build_graph(nodes, edges);
    let a = run_node(&g);
    let b = get_context_lines(&g, FIXED_MS).join("\n");
    assert_eq!(a, b);
}

#[test]
fn context_specialization_and_gotchas() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let nodes = json!({
        "agent:worker": {"kind":"agent","label":"worker","count":3,"last_seen":"2026-04-19T11:55:00.000Z"},
        "file:src/x.rs": {"kind":"file","label":"src/x.rs","count":4,"last_seen":"2026-04-19T11:55:00.000Z"},
        "file:src/y.rs": {"kind":"file","label":"src/y.rs","count":2,"last_seen":"2026-04-19T11:55:00.000Z"},
        "gotcha:bug-1": {"kind":"gotcha","label":"off-by-one no loop","count":1,"last_seen":"2026-04-19T11:55:00.000Z"},
    });
    let edges = json!({
        "agent:worker→file:src/x.rs→edits": {
            "from":"agent:worker","to":"file:src/x.rs","rel":"edits",
            "count":4,"last_seen":"2026-04-19T11:55:00.000Z"
        },
        "agent:worker→file:src/y.rs→edits": {
            "from":"agent:worker","to":"file:src/y.rs","rel":"edits",
            "count":2,"last_seen":"2026-04-19T11:55:00.000Z"
        },
        "gotcha:bug-1→file:src/x.rs→gotcha_for": {
            "from":"gotcha:bug-1","to":"file:src/x.rs","rel":"gotcha_for",
            "count":1,"last_seen":"2026-04-19T11:55:00.000Z"
        },
    });
    let g = build_graph(nodes, edges);
    let a = run_node(&g);
    let b = get_context_lines(&g, FIXED_MS).join("\n");
    assert_eq!(a, b);
}

#[test]
fn context_workflow_file_to_cmd() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let nodes = json!({
        "file:src/a.rs": {"kind":"file","label":"src/a.rs","count":3,"last_seen":"2026-04-19T11:55:00.000Z"},
        "command:cargo test --lib": {"kind":"command","label":"cargo test --lib","count":3,"last_seen":"2026-04-19T11:55:00.000Z"},
    });
    let edges = json!({
        "file:src/a.rs→command:cargo test --lib→followed_by": {
            "from":"file:src/a.rs","to":"command:cargo test --lib","rel":"followed_by",
            "count":3,"last_seen":"2026-04-19T11:55:00.000Z"
        },
    });
    let g = build_graph(nodes, edges);
    let a = run_node(&g);
    let b = get_context_lines(&g, FIXED_MS).join("\n");
    assert_eq!(a, b);
}
