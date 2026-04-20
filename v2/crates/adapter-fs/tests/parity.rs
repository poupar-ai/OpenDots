//! Cross-language parity: `adapter-fs` (Rust) ↔ `plugin/scripts/lib.cjs`
//! + `plugin/scripts/graph.cjs` + `session-init.cjs`/`session-end.cjs` (Node).
//!
//! Cenários:
//! - `ObservationLog`: append format, read_all, read_last bidirecional
//! - `GraphStore`: save bytes idênticos, load/save bidirecional
//! - `SessionStore`: flag file criado/removido visível dos dois lados
//!
//! Cada teste gera seu próprio tempdir e spawna um processo Node curto.
//! Os scripts dependem só de `lib.cjs` / `graph.cjs` para isolar o que
//! está sendo testado.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use adapter_fs::{GraphStoreFs, ObservationLogFs, SessionStoreFs};
use core_ports::{GraphStore, ObservationLog, SessionStore};
use serde_json::{json, Value};
use tempfile::TempDir;

// ─── Helpers ─────────────────────────────────────────────────────────

fn scripts_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .join("../../../plugin/scripts")
        .canonicalize()
        .expect("locate plugin/scripts")
}

fn esc(p: &Path) -> String {
    p.to_str().expect("utf8 path").replace('\\', "\\\\")
}

fn run_node(script: &str) -> String {
    let child = Command::new("node")
        .arg("-e")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn node");
    let out = child.wait_with_output().expect("wait node");
    if !out.status.success() {
        panic!(
            "node failed: status={:?}\nscript:\n{}\nstderr:\n{}",
            out.status,
            script,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    String::from_utf8(out.stdout).expect("utf8 stdout")
}

fn tmp() -> TempDir {
    TempDir::new().expect("tempdir")
}

// ─── ObservationLog ──────────────────────────────────────────────────

#[test]
fn parity_append_byte_exact() {
    // Mesmo objeto escrito em Node via appendJsonl e em Rust via append
    // deve gerar bytes idênticos no arquivo.
    let dir_node = tmp();
    let dir_rust = tmp();
    let lib = esc(&scripts_dir().join("lib.cjs"));
    let f_node = esc(&dir_node.path().join("obs.jsonl"));

    let script = format!(
        r#"
        const {{ appendJsonl }} = require('{lib}');
        appendJsonl('{f_node}', {{ type: 'a', n: 1, tags: ['x', 'y'] }});
        appendJsonl('{f_node}', {{ type: 'b', n: 2 }});
        "#
    );
    run_node(&script);
    let node_bytes = std::fs::read(dir_node.path().join("obs.jsonl")).unwrap();

    let log = ObservationLogFs::new();
    let f_rust = dir_rust.path().join("obs.jsonl");
    log.append(&f_rust, &json!({ "type": "a", "n": 1, "tags": ["x", "y"] }))
        .unwrap();
    log.append(&f_rust, &json!({ "type": "b", "n": 2 }))
        .unwrap();
    let rust_bytes = std::fs::read(&f_rust).unwrap();

    assert_eq!(node_bytes, rust_bytes, "append bytes must be identical");
}

#[test]
fn parity_node_appends_rust_reads() {
    let dir = tmp();
    let lib = esc(&scripts_dir().join("lib.cjs"));
    let f = esc(&dir.path().join("obs.jsonl"));
    let script = format!(
        r#"
        const {{ appendJsonl }} = require('{lib}');
        appendJsonl('{f}', {{ i: 1 }});
        appendJsonl('{f}', {{ i: 2 }});
        appendJsonl('{f}', {{ i: 3, nested: {{ a: true }} }});
        "#
    );
    run_node(&script);

    let log = ObservationLogFs::new();
    let all = log.read_all(&dir.path().join("obs.jsonl")).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0]["i"], 1);
    assert_eq!(all[2]["nested"]["a"], true);
}

#[test]
fn parity_rust_appends_node_reads() {
    let dir = tmp();
    let path = dir.path().join("obs.jsonl");
    let log = ObservationLogFs::new();
    log.append(&path, &json!({ "i": 1 })).unwrap();
    log.append(&path, &json!({ "i": 2 })).unwrap();
    log.append(&path, &json!({ "i": 3 })).unwrap();

    let lib = esc(&scripts_dir().join("lib.cjs"));
    let f = esc(&path);
    let script = format!(
        r#"
        const {{ readJsonl }} = require('{lib}');
        process.stdout.write(JSON.stringify(readJsonl('{f}')));
        "#
    );
    let out = run_node(&script);
    let parsed: Vec<Value> = serde_json::from_str(&out).unwrap();
    assert_eq!(parsed.len(), 3);
    assert_eq!(parsed[2]["i"], 3);
}

#[test]
fn parity_read_last_matches_node() {
    let dir = tmp();
    let lib = esc(&scripts_dir().join("lib.cjs"));
    let f = esc(&dir.path().join("obs.jsonl"));
    let script = format!(
        r#"
        const {{ appendJsonl, readLastJsonlLine }} = require('{lib}');
        for (let i = 0; i < 200; i++) appendJsonl('{f}', {{ i, pad: 'x'.repeat(15) }});
        process.stdout.write(JSON.stringify(readLastJsonlLine('{f}')));
        "#
    );
    let node_out = run_node(&script);
    let node_last: Value = serde_json::from_str(&node_out).unwrap();

    let log = ObservationLogFs::new();
    let rust_last = log
        .read_last(&dir.path().join("obs.jsonl"))
        .unwrap()
        .unwrap();
    assert_eq!(node_last, rust_last);
    assert_eq!(rust_last["i"], 199);
}

#[test]
fn parity_read_last_malformed_tail_both_null() {
    // Confirma o finding F20-01: .cjs e Rust retornam null/None quando a
    // última linha não-vazia é malformada.
    let dir = tmp();
    let lib = esc(&scripts_dir().join("lib.cjs"));
    let path = dir.path().join("obs.jsonl");
    std::fs::write(&path, "{\"i\":1}\n{\"i\":2}\nnot-json\n").unwrap();
    let f = esc(&path);
    let script = format!(
        r#"
        const {{ readLastJsonlLine }} = require('{lib}');
        process.stdout.write(JSON.stringify(readLastJsonlLine('{f}')));
        "#
    );
    let node_out = run_node(&script);
    assert_eq!(node_out, "null");

    let log = ObservationLogFs::new();
    assert!(log.read_last(&path).unwrap().is_none());
}

#[test]
fn parity_read_all_skips_malformed_same_as_node() {
    let dir = tmp();
    let lib = esc(&scripts_dir().join("lib.cjs"));
    let path = dir.path().join("obs.jsonl");
    std::fs::write(&path, "{\"i\":1}\n   \nnot-json\n{\"i\":2}\n\n{\"i\":3}\n").unwrap();
    let f = esc(&path);
    let script = format!(
        r#"
        const {{ readJsonl }} = require('{lib}');
        process.stdout.write(JSON.stringify(readJsonl('{f}')));
        "#
    );
    let node_out: Vec<Value> = serde_json::from_str(&run_node(&script)).unwrap();

    let log = ObservationLogFs::new();
    let rust_out = log.read_all(&path).unwrap();

    assert_eq!(node_out, rust_out);
    assert_eq!(rust_out.len(), 3);
}

// ─── GraphStore ──────────────────────────────────────────────────────

#[test]
fn parity_save_bytes_identical() {
    // Mesmo Value → mesmos bytes em graph.json. Protege contra drift de
    // indent/ordem de chaves/escape de strings.
    let dir_node = tmp();
    let dir_rust = tmp();

    // Valor complexo: números, strings com escape, objetos aninhados,
    // array com objeto, null e booleanos.
    let graph_json = r#"{"version":2,"rebuilt_at":null,"updated_at":"2026-04-19T10:00:00Z","nodes":{"file:a.rs":{"kind":"file","label":"a.rs","count":3,"last_seen":"2026-04-19T10:00:00Z"},"cmd:cargo test":{"kind":"command","count":7,"tags":["build","test"]}},"edges":{}}"#;

    // Node save
    let graph_cjs = esc(&scripts_dir().join("graph.cjs"));
    let mem_node = esc(dir_node.path());
    let script = format!(
        r#"
        const fs = require('fs');
        const path = require('path');
        const g = {graph_json};
        // Bypass saveGraph (que muta updated_at). Grava direto como o
        // .cjs faria se updated_at já estivesse fixo no input.
        fs.writeFileSync(path.join('{mem_node}', 'graph.json'), JSON.stringify(g, null, 2));
        // Sanity: o caminho do módulo só garante que o arquivo existe.
        void require('{graph_cjs}');
        "#
    );
    run_node(&script);
    let node_bytes = std::fs::read(dir_node.path().join("graph.json")).unwrap();

    // Rust save
    let g: Value = serde_json::from_str(graph_json).unwrap();
    let store = GraphStoreFs::new();
    store.save(dir_rust.path(), &g).unwrap();
    let rust_bytes = std::fs::read(dir_rust.path().join("graph.json")).unwrap();

    assert_eq!(
        String::from_utf8_lossy(&node_bytes),
        String::from_utf8_lossy(&rust_bytes)
    );
}

#[test]
fn parity_node_saves_rust_loads() {
    let dir = tmp();
    let mem = esc(dir.path());
    let script = format!(
        r#"
        const fs = require('fs');
        const path = require('path');
        const g = {{ version: 2, rebuilt_at: null, updated_at: null, nodes: {{}}, edges: {{}} }};
        fs.writeFileSync(path.join('{mem}', 'graph.json'), JSON.stringify(g, null, 2));
        "#
    );
    run_node(&script);

    let store = GraphStoreFs::new();
    let loaded = store.load(dir.path()).unwrap().unwrap();
    assert_eq!(loaded["version"], 2);
    assert_eq!(loaded["nodes"], json!({}));
}

#[test]
fn parity_rust_saves_node_loads() {
    let dir = tmp();
    let store = GraphStoreFs::new();
    let g = json!({
        "version": 2,
        "rebuilt_at": null,
        "updated_at": null,
        "nodes": { "file:x.rs": { "count": 1, "kind": "file", "last_seen": "2026-01-01T00:00:00Z" } },
        "edges": {}
    });
    store.save(dir.path(), &g).unwrap();

    let mem = esc(dir.path());
    let script = format!(
        r#"
        const fs = require('fs');
        const path = require('path');
        const raw = fs.readFileSync(path.join('{mem}', 'graph.json'), 'utf8');
        const g = JSON.parse(raw);
        process.stdout.write(JSON.stringify([g.version, Object.keys(g.nodes).length, g.nodes['file:x.rs'].count]));
        "#
    );
    let out = run_node(&script);
    let parsed: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(parsed, json!([2, 1, 1]));
}

// ─── SessionStore ────────────────────────────────────────────────────

#[test]
fn parity_node_marks_rust_sees() {
    let dir = tmp();
    let flag = esc(&dir.path().join(".init-sidABC"));
    let script = format!(
        r#"
        const fs = require('fs');
        fs.writeFileSync('{flag}', '');
        "#
    );
    run_node(&script);

    let store = SessionStoreFs::new();
    assert!(store.is_initialized(dir.path(), "sidABC").unwrap());
}

#[test]
fn parity_rust_marks_node_sees() {
    let dir = tmp();
    let store = SessionStoreFs::new();
    store.mark_initialized(dir.path(), "sidXYZ").unwrap();

    let flag = esc(&dir.path().join(".init-sidXYZ"));
    let script = format!(
        r#"
        const fs = require('fs');
        const exists = fs.existsSync('{flag}');
        const size = exists ? fs.statSync('{flag}').size : -1;
        process.stdout.write(JSON.stringify({{ exists, size }}));
        "#
    );
    let out = run_node(&script);
    assert_eq!(out, r#"{"exists":true,"size":0}"#);
}

#[test]
fn parity_rust_clear_node_sees_absent() {
    let dir = tmp();
    let store = SessionStoreFs::new();
    store.mark_initialized(dir.path(), "sid1").unwrap();
    store.clear(dir.path(), "sid1").unwrap();

    let flag = esc(&dir.path().join(".init-sid1"));
    let script = format!(
        r#"
        const fs = require('fs');
        process.stdout.write(String(fs.existsSync('{flag}')));
        "#
    );
    assert_eq!(run_node(&script), "false");
}
