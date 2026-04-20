//! Parity tests — compara `domain-router` com `plugin/scripts/router.cjs`
//! invocado via `node` em subprocess. Pula se `node` ausente.

use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::tempdir;

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

const PLUGIN_SCRIPTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../plugin/scripts");

fn run_node_route(mem_dir: &std::path::Path, req: &Value) -> Value {
    let script = format!(
        r#"
const router = require('{PLUGIN_SCRIPTS}/router.cjs');
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => {{ input += d; }});
process.stdin.on('end', () => {{
  try {{
    const req = JSON.parse(input);
    let r;
    if (req.op === 'route') {{
      r = router.route({{ memDir: {:?}, from: req.from, file: req.file, topic: req.topic, intent: req.intent }});
    }} else if (req.op === 'suggestTargets') {{
      r = router.suggestTargets({{ memDir: {:?}, from: req.from, message: req.message }});
    }}
    process.stdout.write(JSON.stringify(r));
  }} catch (e) {{
    process.stderr.write(e.message);
    process.exit(1);
  }}
}});
"#,
        mem_dir.display().to_string(),
        mem_dir.display().to_string()
    );
    let mut child = Command::new("node")
        .arg("-e")
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn node");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(serde_json::to_string(req).unwrap().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "node failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("parse node stdout")
}

fn seed_manifest(mem_dir: &std::path::Path, manifest: &Value) {
    std::fs::create_dir_all(mem_dir).unwrap();
    std::fs::write(
        mem_dir.join("agents.json"),
        serde_json::to_string_pretty(manifest).unwrap(),
    )
    .unwrap();
}

fn seed_graph(mem_dir: &std::path::Path, g: &Value) {
    std::fs::write(
        mem_dir.join("graph.json"),
        serde_json::to_string_pretty(g).unwrap(),
    )
    .unwrap();
}

#[test]
fn route_unknown_agent() {
    if !node_available() {
        return;
    }
    let d = tempdir().unwrap();
    seed_manifest(
        d.path(),
        &json!({ "version": 1, "manager": "manager", "oracle": "oracle", "agents": {} }),
    );
    let rust = domain_router::route(
        d.path(),
        "ghost",
        None,
        None,
        None,
        &domain_router::SystemClock,
    );
    let node = run_node_route(d.path(), &json!({ "op": "route", "from": "ghost" }));
    assert_eq!(rust.error.as_deref(), node["error"].as_str());
    assert!(rust.candidates.is_empty());
}

#[test]
fn route_empty_can_talk() {
    if !node_available() {
        return;
    }
    let d = tempdir().unwrap();
    seed_manifest(
        d.path(),
        &json!({
            "version": 1, "manager": "m", "oracle": "o",
            "agents": { "a": { "type": "specialist" } }
        }),
    );
    let rust = domain_router::route(
        d.path(),
        "a",
        None,
        None,
        None,
        &domain_router::SystemClock,
    );
    let node = run_node_route(d.path(), &json!({ "op": "route", "from": "a" }));
    assert_eq!(rust.note.as_deref(), node["note"].as_str());
    assert!(rust.candidates.is_empty());
}

#[test]
fn route_ordered_by_can_talk_when_no_graph() {
    if !node_available() {
        return;
    }
    let d = tempdir().unwrap();
    seed_manifest(
        d.path(),
        &json!({
            "version": 1, "manager": "m", "oracle": "o",
            "agents": {
                "a": { "type": "specialist", "can_talk_to": ["b", "c", "d"] },
                "b": { "type": "specialist" },
                "c": { "type": "specialist" },
                "d": { "type": "specialist" },
            }
        }),
    );
    let rust = domain_router::route(
        d.path(),
        "a",
        None,
        None,
        None,
        &domain_router::SystemClock,
    );
    let node = run_node_route(d.path(), &json!({ "op": "route", "from": "a" }));
    let rust_agents: Vec<&str> = rust
        .candidates
        .iter()
        .filter_map(|c| c["agent"].as_str())
        .collect();
    let node_agents: Vec<&str> = node["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["agent"].as_str())
        .collect();
    assert_eq!(rust_agents, node_agents);
    assert_eq!(rust_agents, vec!["b", "c", "d"]);
}

#[test]
fn route_escalate_prioritizes_firefighter() {
    if !node_available() {
        return;
    }
    let d = tempdir().unwrap();
    seed_manifest(
        d.path(),
        &json!({
            "version": 1, "manager": "m", "oracle": "o",
            "agents": {
                "a": { "type": "specialist", "can_talk_to": ["b", "internal_firefighter", "c"] },
                "b": { "type": "specialist" },
                "c": { "type": "specialist" },
                "internal_firefighter": { "type": "firefighter" },
            }
        }),
    );
    let rust = domain_router::route(
        d.path(),
        "a",
        None,
        None,
        Some("escalate"),
        &domain_router::SystemClock,
    );
    let node = run_node_route(
        d.path(),
        &json!({ "op": "route", "from": "a", "intent": "escalate" }),
    );
    let rust_agents: Vec<&str> = rust
        .candidates
        .iter()
        .filter_map(|c| c["agent"].as_str())
        .collect();
    let node_agents: Vec<&str> = node["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["agent"].as_str())
        .collect();
    assert_eq!(rust_agents, node_agents);
    assert_eq!(rust_agents[0], "internal_firefighter");
}

#[test]
fn route_with_file_signal_boosts_editor() {
    if !node_available() {
        return;
    }
    let d = tempdir().unwrap();
    seed_manifest(
        d.path(),
        &json!({
            "version": 1, "manager": "m", "oracle": "o",
            "agents": {
                "a": { "type": "specialist", "can_talk_to": ["b", "c"] },
                "b": { "type": "specialist" },
                "c": { "type": "specialist" },
            }
        }),
    );
    // c editou x.rs várias vezes, b não — c deveria liderar
    seed_graph(
        d.path(),
        &json!({
            "version": 2,
            "nodes": {},
            "edges": {
                "e1": { "rel": "edits", "from": "agent:c", "to": "file:x.rs", "count": 10, "last_seen": "2026-04-19T00:00:00.000Z" }
            },
            "updated_at": "2026-04-19T00:00:00.000Z"
        }),
    );
    let rust = domain_router::route(
        d.path(),
        "a",
        Some("x.rs"),
        None,
        None,
        &domain_router::SystemClock,
    );
    let node = run_node_route(
        d.path(),
        &json!({ "op": "route", "from": "a", "file": "x.rs" }),
    );
    let rust_first = rust.candidates[0]["agent"].as_str().unwrap();
    let node_first = node["candidates"][0]["agent"].as_str().unwrap();
    assert_eq!(rust_first, node_first);
    assert_eq!(rust_first, "c");
}

#[test]
fn suggest_detects_backtick_file() {
    if !node_available() {
        return;
    }
    let d = tempdir().unwrap();
    seed_manifest(
        d.path(),
        &json!({
            "version": 1, "manager": "m", "oracle": "o",
            "agents": {
                "a": { "type": "specialist", "can_talk_to": ["b"] },
                "b": { "type": "specialist" },
            }
        }),
    );
    let (file, _) = domain_router::suggest_targets(
        d.path(),
        "a",
        "pode olhar `src/foo.rs` pra mim?",
        &domain_router::SystemClock,
    );
    let node = run_node_route(
        d.path(),
        &json!({ "op": "suggestTargets", "from": "a", "message": "pode olhar `src/foo.rs` pra mim?" }),
    );
    assert_eq!(file.as_deref(), node["file_detected"].as_str());
    assert_eq!(file.as_deref(), Some("src/foo.rs"));
}

#[test]
fn suggest_detects_extension_file() {
    if !node_available() {
        return;
    }
    let d = tempdir().unwrap();
    seed_manifest(
        d.path(),
        &json!({
            "version": 1, "manager": "m", "oracle": "o",
            "agents": {
                "a": { "type": "specialist", "can_talk_to": ["b"] },
                "b": { "type": "specialist" },
            }
        }),
    );
    let (file, _) = domain_router::suggest_targets(
        d.path(),
        "a",
        "arrumei meu/arquivo.py agora",
        &domain_router::SystemClock,
    );
    let node = run_node_route(
        d.path(),
        &json!({ "op": "suggestTargets", "from": "a", "message": "arrumei meu/arquivo.py agora" }),
    );
    assert_eq!(file.as_deref(), node["file_detected"].as_str());
}

#[test]
fn suggest_no_file_no_panic() {
    if !node_available() {
        return;
    }
    let d = tempdir().unwrap();
    seed_manifest(
        d.path(),
        &json!({
            "version": 1, "manager": "m", "oracle": "o",
            "agents": {
                "a": { "type": "specialist", "can_talk_to": ["b", "c"] },
                "b": { "type": "specialist" },
                "c": { "type": "specialist" },
            }
        }),
    );
    let (file, cands) = domain_router::suggest_targets(
        d.path(),
        "a",
        "oi, tudo bem?",
        &domain_router::SystemClock,
    );
    let node = run_node_route(
        d.path(),
        &json!({ "op": "suggestTargets", "from": "a", "message": "oi, tudo bem?" }),
    );
    assert_eq!(file.as_deref(), node["file_detected"].as_str());
    assert!(cands.len() <= 3);
    let node_cands: &Vec<Value> = node["candidates"].as_array().unwrap();
    assert_eq!(cands.len(), node_cands.len());
}
