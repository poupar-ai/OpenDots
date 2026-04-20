use adapter_fs::ObservationLogFs;
use core_ports::ObservationLog;
use serde_json::json;
use std::fs;
use tempfile::tempdir;

#[test]
fn append_creates_file_and_writes_line() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("obs.jsonl");
    let log = ObservationLogFs::new();
    log.append(&path, &json!({ "type": "a", "n": 1 })).unwrap();
    let raw = fs::read_to_string(&path).unwrap();
    assert_eq!(raw, "{\"type\":\"a\",\"n\":1}\n");
}

#[test]
fn append_multiple_preserves_order() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("obs.jsonl");
    let log = ObservationLogFs::new();
    log.append(&path, &json!({ "i": 1 })).unwrap();
    log.append(&path, &json!({ "i": 2 })).unwrap();
    log.append(&path, &json!({ "i": 3 })).unwrap();
    let all = log.read_all(&path).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0]["i"], 1);
    assert_eq!(all[2]["i"], 3);
}

#[test]
fn read_all_returns_empty_if_missing() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("absent.jsonl");
    let log = ObservationLogFs::new();
    assert!(log.read_all(&path).unwrap().is_empty());
}

#[test]
fn read_all_skips_blank_and_malformed() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mixed.jsonl");
    fs::write(&path, "\n   \n{\"ok\":1}\nnot-json\n{\"ok\":2}\n\t\n").unwrap();
    let log = ObservationLogFs::new();
    let all = log.read_all(&path).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0]["ok"], 1);
    assert_eq!(all[1]["ok"], 2);
}

#[test]
fn read_last_returns_none_if_missing() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("absent.jsonl");
    let log = ObservationLogFs::new();
    assert!(log.read_last(&path).unwrap().is_none());
}

#[test]
fn read_last_returns_none_if_empty() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("empty.jsonl");
    fs::write(&path, "").unwrap();
    let log = ObservationLogFs::new();
    assert!(log.read_last(&path).unwrap().is_none());
}

#[test]
fn read_last_returns_last_entry() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("obs.jsonl");
    let log = ObservationLogFs::new();
    log.append(&path, &json!({ "i": 1 })).unwrap();
    log.append(&path, &json!({ "i": 2 })).unwrap();
    log.append(&path, &json!({ "i": 3 })).unwrap();
    assert_eq!(log.read_last(&path).unwrap().unwrap()["i"], 3);
}

#[test]
fn read_last_skips_trailing_blank_lines() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("obs.jsonl");
    fs::write(&path, "{\"i\":1}\n{\"i\":2}\n\n   \n").unwrap();
    let log = ObservationLogFs::new();
    assert_eq!(log.read_last(&path).unwrap().unwrap()["i"], 2);
}

#[test]
fn read_last_malformed_tail_returns_none() {
    // Paridade com `readLastJsonlLine` do .cjs: em cada iteração do scan
    // backward, só a ÚLTIMA linha não-vazia do buffer é testada. Se ela é
    // malformada, o loop adiciona mais um chunk — mas continua olhando
    // apenas a última. Quando chega em pos=0 sem parse válido, retorna
    // null. Registrado como finding F20-01.
    let dir = tempdir().unwrap();
    let path = dir.path().join("obs.jsonl");
    fs::write(&path, "{\"i\":1}\n{\"i\":2}\nnot-json\n").unwrap();
    let log = ObservationLogFs::new();
    assert!(log.read_last(&path).unwrap().is_none());
}

#[test]
fn read_last_handles_file_larger_than_chunk() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("big.jsonl");
    let log = ObservationLogFs::new();
    for i in 0..2000 {
        log.append(&path, &json!({ "i": i, "pad": "x".repeat(20) }))
            .unwrap();
    }
    let meta = fs::metadata(&path).unwrap();
    assert!(meta.len() > adapter_fs::TAIL_CHUNK as u64);
    let last = log.read_last(&path).unwrap().unwrap();
    assert_eq!(last["i"], 1999);
}
