use adapter_fs::GraphStoreFs;
use core_ports::GraphStore;
use serde_json::json;
use std::fs;
use tempfile::tempdir;

#[test]
fn load_returns_none_if_missing() {
    let dir = tempdir().unwrap();
    let store = GraphStoreFs::new();
    assert!(store.load(dir.path()).unwrap().is_none());
}

#[test]
fn save_writes_two_space_pretty() {
    let dir = tempdir().unwrap();
    let store = GraphStoreFs::new();
    let graph = json!({ "version": 2, "nodes": {}, "edges": {} });
    store.save(dir.path(), &graph).unwrap();
    let raw = fs::read_to_string(dir.path().join("graph.json")).unwrap();
    // JSON.stringify(graph, null, 2) no .cjs gera esse shape exato.
    let expected = "{\n  \"version\": 2,\n  \"nodes\": {},\n  \"edges\": {}\n}";
    assert_eq!(raw, expected);
}

#[test]
fn save_preserves_key_order_of_caller() {
    let dir = tempdir().unwrap();
    let store = GraphStoreFs::new();
    // Passa as chaves na ordem rebuilt_at → nodes → version, que não é
    // alfabética nem canônica. Preserve_order tem que respeitar.
    let graph = json!({
        "rebuilt_at": "2026-04-19T00:00:00Z",
        "nodes": { "file:a.rs": { "count": 1 } },
        "version": 2
    });
    store.save(dir.path(), &graph).unwrap();
    let raw = fs::read_to_string(dir.path().join("graph.json")).unwrap();
    let rebuilt_idx = raw.find("rebuilt_at").unwrap();
    let nodes_idx = raw.find("nodes").unwrap();
    let version_idx = raw.find("version").unwrap();
    assert!(rebuilt_idx < nodes_idx);
    assert!(nodes_idx < version_idx);
}

#[test]
fn roundtrip() {
    let dir = tempdir().unwrap();
    let store = GraphStoreFs::new();
    let graph = json!({
        "version": 2,
        "rebuilt_at": null,
        "updated_at": "2026-04-19T10:00:00Z",
        "nodes": {
            "file:a.rs": { "kind": "file", "label": "a.rs", "count": 3, "last_seen": "2026-04-19T10:00:00Z" }
        },
        "edges": {}
    });
    store.save(dir.path(), &graph).unwrap();
    let loaded = store.load(dir.path()).unwrap().unwrap();
    assert_eq!(loaded, graph);
}

#[test]
fn load_errors_on_malformed_json() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("graph.json"), "{not json").unwrap();
    let store = GraphStoreFs::new();
    assert!(store.load(dir.path()).is_err());
}
