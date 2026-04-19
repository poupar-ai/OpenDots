//! Paridade v1 (Node `.cjs`) vs v2 (Rust): shape de `vault.status()`.
//!
//! Invoca `node` executando `plugin/scripts/vault.cjs` contra um `memDir`
//! temporário não-inicializado. O output precisa deserializar exatamente
//! no struct `core_ports::VaultStatus` e os campos precisam bater com o
//! contrato do `.cjs`.

use core_ports::VaultStatus;
use std::process::Command;

const NODE_SCRIPT: &str = r#"
const os = require('os');
const fs = require('fs');
const path = require('path');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'vault-parity-'));
const v = require(process.argv[1]);
process.stdout.write(JSON.stringify(v.status(tmp)));
"#;

fn vault_cjs_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/vault.cjs")
        .canonicalize()
        .expect("vault.cjs missing — expected at plugin/scripts/vault.cjs")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn invoke_v1_status() -> String {
    let out = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(vault_cjs_path())
        .output()
        .expect("failed to spawn node");
    assert!(
        out.status.success(),
        "node exited {:?}, stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("stdout not utf-8")
}

#[test]
fn v1_status_deserializes_into_rust_struct() {
    if !node_available() {
        eprintln!("node indisponível — pulando teste de paridade");
        return;
    }
    let json = invoke_v1_status();
    let status: VaultStatus =
        serde_json::from_str(&json).expect("Rust struct não bate com shape do .cjs");

    assert!(!status.initialized, "vault não-inicializado vira initialized=false");
    assert!(status.locked, "vault não-inicializado vira locked=true");
    assert!(!status.needs_migration);
    assert!(status.expires_at.is_none());
    assert!(status.ttl_ms.is_none());
}

#[test]
fn v1_status_keys_match_exactly() {
    if !node_available() {
        return;
    }
    let json = invoke_v1_status();
    let v1: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&json).expect("parse v1 output");
    let rust_emitted = serde_json::to_value(VaultStatus {
        initialized: false,
        locked: true,
        expires_at: None,
        ttl_ms: None,
        needs_migration: false,
    })
    .unwrap();
    let v2 = rust_emitted.as_object().unwrap().clone();

    let v1_keys: std::collections::BTreeSet<_> = v1.keys().collect();
    let v2_keys: std::collections::BTreeSet<_> = v2.keys().collect();
    assert_eq!(
        v1_keys, v2_keys,
        "chaves divergem entre v1 ({:?}) e v2 ({:?})",
        v1_keys, v2_keys
    );
}
