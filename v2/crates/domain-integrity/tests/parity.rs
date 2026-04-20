//! Paridade com `plugin/scripts/integrity.cjs`. Testa:
//! - `snapshot` contra o plugin real + mem dir tmp
//! - `diff` com snapshots construídos manualmente
//! - `classify`

use domain_integrity::{classify, diff, snapshot, Change, ChangeKind, FileSnapshot};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const cjsPath = process.argv[1];
const mod = require(cjsPath);
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const req = JSON.parse(input);
    let r;
    if (req.op === 'snapshot') {
      const m = mod.snapshot(req.memDir);
      // Convert Map to plain object for JSON transport
      const out = {};
      for (const [k, v] of m) out[k] = v;
      r = out;
    } else if (req.op === 'diff') {
      const b = new Map(Object.entries(req.before || {}));
      const a = new Map(Object.entries(req.after || {}));
      r = mod.diff(b, a);
    } else if (req.op === 'classify') {
      r = mod.classify(req.path);
    } else if (req.op === 'pluginRoot') {
      r = mod.pluginRoot();
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
        .join("../../../plugin/scripts/integrity.cjs")
        .canonicalize()
        .expect("integrity.cjs missing")
}

fn real_plugin_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin")
        .canonicalize()
        .expect("plugin dir missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_node(input: Value) -> Value {
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

#[test]
fn parity_plugin_root_matches() {
    if !node_available() {
        return;
    }
    let js_root = run_node(json!({"op": "pluginRoot"}));
    let rs_root = real_plugin_root();
    assert_eq!(js_root.as_str().unwrap(), rs_root.to_string_lossy());
}

#[test]
fn parity_snapshot_real_plugin() {
    if !node_available() {
        return;
    }
    let mem = TempDir::new().unwrap();
    fs::write(mem.path().join("agents.json"), b"{\"agents\":{}}").unwrap();
    fs::write(mem.path().join("humans.json"), b"{}").unwrap();

    let js_map = run_node(json!({
        "op": "snapshot",
        "memDir": mem.path().to_string_lossy()
    }));
    let rs_map = snapshot(&real_plugin_root(), mem.path());

    // Compare set of keys
    let js_keys: std::collections::BTreeSet<String> = js_map
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    let rs_keys: std::collections::BTreeSet<String> =
        rs_map.keys().cloned().collect();
    assert_eq!(js_keys, rs_keys, "snapshot key set diverges");

    // Compare size + sha for each file (mtime float may diverge slightly)
    for k in &js_keys {
        let js = &js_map[k];
        let rs = rs_map.get(k).unwrap();
        assert_eq!(
            js["size"].as_u64().unwrap(),
            rs.size,
            "size diverges for {k}"
        );
        let js_sha = js["sha"].as_str();
        let rs_sha = rs.sha.as_deref();
        assert_eq!(js_sha, rs_sha, "sha diverges for {k}");
    }
}

#[test]
fn parity_diff_modified() {
    if !node_available() {
        return;
    }
    let before: BTreeMap<String, FileSnapshot> = [(
        "/tmp/foo.txt".to_string(),
        FileSnapshot {
            mtime: 1000.0,
            size: 10,
            sha: Some("aaa".into()),
        },
    )]
    .into_iter()
    .collect();
    let after: BTreeMap<String, FileSnapshot> = [(
        "/tmp/foo.txt".to_string(),
        FileSnapshot {
            mtime: 2000.0,
            size: 15,
            sha: Some("bbb".into()),
        },
    )]
    .into_iter()
    .collect();

    let js = run_node(json!({
        "op": "diff",
        "before": serde_json::to_value(&before).unwrap(),
        "after": serde_json::to_value(&after).unwrap()
    }));
    let rs = diff(&before, &after);

    assert_eq!(js.as_array().unwrap().len(), rs.len());
    assert_eq!(js[0]["path"].as_str().unwrap(), rs[0].path);
    assert_eq!(js[0]["change"].as_str().unwrap(), "modified");
    assert_eq!(rs[0].change, ChangeKind::Modified);
    assert_eq!(js[0]["size_delta"].as_i64().unwrap(), rs[0].size_delta.unwrap());
}

#[test]
fn parity_diff_deletion() {
    if !node_available() {
        return;
    }
    let before: BTreeMap<String, FileSnapshot> = [(
        "/tmp/x.txt".to_string(),
        FileSnapshot {
            mtime: 1000.0,
            size: 5,
            sha: Some("h".into()),
        },
    )]
    .into_iter()
    .collect();
    let after: BTreeMap<String, FileSnapshot> = BTreeMap::new();

    let js = run_node(json!({
        "op": "diff",
        "before": serde_json::to_value(&before).unwrap(),
        "after": serde_json::to_value(&after).unwrap()
    }));
    let rs = diff(&before, &after);
    assert_eq!(js[0]["change"].as_str().unwrap(), "deleted");
    assert_eq!(rs[0].change, ChangeKind::Deleted);
}

#[test]
fn parity_diff_creation() {
    if !node_available() {
        return;
    }
    let before: BTreeMap<String, FileSnapshot> = BTreeMap::new();
    let after: BTreeMap<String, FileSnapshot> = [(
        "/tmp/new.txt".to_string(),
        FileSnapshot {
            mtime: 1000.0,
            size: 5,
            sha: Some("h".into()),
        },
    )]
    .into_iter()
    .collect();

    let js = run_node(json!({
        "op": "diff",
        "before": serde_json::to_value(&before).unwrap(),
        "after": serde_json::to_value(&after).unwrap()
    }));
    let rs = diff(&before, &after);
    assert_eq!(js[0]["change"].as_str().unwrap(), "created");
    assert_eq!(rs[0].change, ChangeKind::Created);
}

#[test]
fn parity_diff_same_mtime_different_sha_not_modified() {
    // Edge case do .cjs: só confirma sha se mtime+size mudaram.
    // Se mtime igual mas sha diferente, NÃO reporta modificação.
    if !node_available() {
        return;
    }
    let before: BTreeMap<String, FileSnapshot> = [(
        "/a".to_string(),
        FileSnapshot {
            mtime: 100.0,
            size: 5,
            sha: Some("xxx".into()),
        },
    )]
    .into_iter()
    .collect();
    let after: BTreeMap<String, FileSnapshot> = [(
        "/a".to_string(),
        FileSnapshot {
            mtime: 100.0,
            size: 5,
            sha: Some("yyy".into()),
        },
    )]
    .into_iter()
    .collect();

    let js = run_node(json!({
        "op": "diff",
        "before": serde_json::to_value(&before).unwrap(),
        "after": serde_json::to_value(&after).unwrap()
    }));
    let rs = diff(&before, &after);
    assert_eq!(js.as_array().unwrap().len(), 0);
    assert_eq!(rs.len(), 0);
}

#[test]
fn parity_classify_all_categories() {
    if !node_available() {
        return;
    }
    let plugin_root = real_plugin_root();
    let cases = vec![
        (format!("{}/scripts/lib.cjs", plugin_root.to_string_lossy()), "plugin"),
        ("/some/where/agents.json".to_string(), "agents_manifest"),
        ("/some/where/bus-config.json".to_string(), "bus_config"),
        ("/some/where/humans.json".to_string(), "humans_manifest"),
        ("/x/something.txt".to_string(), "other"),
    ];
    for (path, expected) in cases {
        let js = run_node(json!({"op": "classify", "path": path}));
        assert_eq!(js.as_str().unwrap(), expected);
        let rs = classify(&path, &plugin_root);
        assert_eq!(rs, expected, "rust divergence for {path}");
    }
}

// Serde helper: FileSnapshot serializes to {mtime, size, sha} matching the
// .cjs Map value shape, so sending it across JSON via `serde_json::to_value`
// is already aligned. But we need to *suppress* `sha: null` -> Node's
// `sha: null` when `sha` is None. Let's sanity-check via a minimal test.
#[test]
fn filesnap_roundtrip_json() {
    let s = FileSnapshot {
        mtime: 123.0,
        size: 4,
        sha: Some("abc".into()),
    };
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["mtime"].as_f64().unwrap(), 123.0);
    assert_eq!(v["size"].as_u64().unwrap(), 4);
    assert_eq!(v["sha"].as_str().unwrap(), "abc");
    let back: FileSnapshot = serde_json::from_value(v).unwrap();
    assert_eq!(back, s);
}

// Silence unused imports warning (Path is transitively used via real_plugin_root)
#[allow(dead_code)]
fn _phantom(_: &Path, _: &Change) {}
