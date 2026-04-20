//! Cross-language parity: `FileVaultStore` (Rust) ↔ `vault.cjs` (Node).
//!
//! Cenários testados:
//! - Vault inicializado em Node é lido (unlock + secrets) pelo Rust
//! - Vault inicializado em Rust é lido pelo Node
//! - Bundle exportado em Node é importado pelo Rust
//! - Bundle exportado em Rust é importado pelo Node
//! - `status()` retorna mesmas chaves/tipos
//! - `listKeys` ordenação determinística
//!
//! Note: cada teste cria seu próprio tempdir. Scrypt custa ~300ms por
//! chamada — unlock/change_password é caro. Agrupo quando possível.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use adapter_vault_file::FileVaultStore;
use core_ports::VaultStore;
use serde_json::Value;
use tempfile::TempDir;

// ─── Helpers ─────────────────────────────────────────────────────────

fn vault_cjs_path() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    // crates/adapter-vault-file → ../../../plugin/scripts/vault.cjs
    PathBuf::from(manifest_dir)
        .join("../../../plugin/scripts/vault.cjs")
        .canonicalize()
        .expect("locate vault.cjs")
}

/// Runs a snippet of JS that has the `vault` module + `memDir` already
/// in scope. Stdout must be valid JSON (or empty string). Stderr on
/// failure is surfaced in the panic so test output explains *why*.
fn run_node_with_vault(mem_dir: &Path, body: &str) -> String {
    let vault_path = vault_cjs_path();
    let mem_dir_escaped = mem_dir.to_str().expect("utf8 path").replace('\\', "\\\\");
    let vault_path_escaped = vault_path
        .to_str()
        .expect("utf8 path")
        .replace('\\', "\\\\");
    let script = format!(
        r#"
        const vault = require('{vault_path_escaped}');
        const memDir = '{mem_dir_escaped}';
        (async () => {{
            try {{
                {body}
            }} catch (e) {{
                console.error('JS_ERR:', e.stack || e.message);
                process.exit(1);
            }}
        }})();
        "#
    );
    let child = Command::new("node")
        .arg("-e")
        .arg(&script)
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

// ─── Node → Rust ─────────────────────────────────────────────────────

#[test]
fn node_init_then_rust_unlocks_and_reads_secret() {
    let t = tmp();
    run_node_with_vault(
        t.path(),
        r#"
        vault.init(memDir, 'node-password-x');
        vault.setSecret(memDir, 'API_KEY', 'sk_from_node');
        vault.setEnvFile(memDir, 'app.env', 'A=1\nB=2');
        process.stdout.write('ok');
        "#,
    );
    // Rust adapter consumes what node wrote.
    let v = FileVaultStore::new();
    v.unlock(t.path(), "node-password-x", 60_000).unwrap();
    assert_eq!(
        v.get_secret(t.path(), "API_KEY").unwrap(),
        Some("sk_from_node".into())
    );
    assert_eq!(
        v.get_envfile(t.path(), "app.env").unwrap(),
        Some("A=1\nB=2".into())
    );
}

#[test]
fn node_init_rust_rejects_wrong_password() {
    let t = tmp();
    run_node_with_vault(
        t.path(),
        r#"
        vault.init(memDir, 'the-right-one');
        process.stdout.write('ok');
        "#,
    );
    let v = FileVaultStore::new();
    let err = v.unlock(t.path(), "the-wrong-one", 60_000).unwrap_err();
    assert!(matches!(err, core_ports::VaultError::WrongPassword));
}

#[test]
fn node_set_secret_then_rust_list_keys_match_sorted() {
    let t = tmp();
    run_node_with_vault(
        t.path(),
        r#"
        vault.init(memDir, 'shared-master-pass');
        vault.setSecret(memDir, 'ZEBRA', '1');
        vault.setSecret(memDir, 'ALPHA', '2');
        vault.setSecret(memDir, 'MIDDLE', '3');
        process.stdout.write('ok');
        "#,
    );
    let node_list_json = run_node_with_vault(
        t.path(),
        r#"
        vault.unlock(memDir, 'shared-master-pass');
        process.stdout.write(JSON.stringify(vault.listKeys(memDir)));
        "#,
    );
    let node_list: Vec<String> = serde_json::from_str(&node_list_json).unwrap();

    let v = FileVaultStore::new();
    v.unlock(t.path(), "shared-master-pass", 60_000).unwrap();
    let rust_list = v.list_keys(t.path()).unwrap();
    assert_eq!(rust_list, node_list);
}

// ─── Rust → Node ─────────────────────────────────────────────────────

#[test]
fn rust_init_then_node_unlocks_and_reads_secret() {
    let t = tmp();
    let v = FileVaultStore::new();
    v.init(t.path(), "rust-master-pass").unwrap();
    v.set_secret(t.path(), "STRIPE", "sk_rust_wrote_me")
        .unwrap();
    v.set_envfile(t.path(), "prod.env", "X=9").unwrap();

    let got = run_node_with_vault(
        t.path(),
        r#"
        vault.unlock(memDir, 'rust-master-pass');
        const r = {
            key: vault.getSecret(memDir, 'STRIPE'),
            env: vault.getEnvFile(memDir, 'prod.env'),
        };
        process.stdout.write(JSON.stringify(r));
        "#,
    );
    let parsed: Value = serde_json::from_str(&got).unwrap();
    assert_eq!(parsed["key"], "sk_rust_wrote_me");
    assert_eq!(parsed["env"], "X=9");
}

// ─── status shape parity ─────────────────────────────────────────────

#[test]
fn status_shape_matches_cjs() {
    let t = tmp();
    run_node_with_vault(
        t.path(),
        r#"
        vault.init(memDir, 'shape-check-pass');
        vault.lock();
        process.stdout.write('ok');
        "#,
    );
    // Node status right after lock.
    let node_json = run_node_with_vault(
        t.path(),
        r#"
        process.stdout.write(JSON.stringify(vault.status(memDir)));
        "#,
    );
    let node_status: Value = serde_json::from_str(&node_json).unwrap();

    // Rust status — must expose the same five keys with matching types.
    let v = FileVaultStore::new();
    let rust_status = v.status(t.path()).unwrap();
    assert_eq!(rust_status.initialized, node_status["initialized"]);
    assert_eq!(rust_status.locked, node_status["locked"]);
    assert_eq!(rust_status.needs_migration, node_status["needs_migration"]);
    // expires_at / ttl_ms are both null here (locked session).
    assert!(node_status["expires_at"].is_null());
    assert!(node_status["ttl_ms"].is_null());
    assert_eq!(rust_status.expires_at, None);
    assert_eq!(rust_status.ttl_ms, None);
}

// ─── bundle: Node export → Rust import ───────────────────────────────

#[test]
fn bundle_export_node_import_rust() {
    let t_node = tmp();
    // Node creates a vault and exports a bundle.
    let bundle_hex = run_node_with_vault(
        t_node.path(),
        r#"
        vault.init(memDir, 'node-master-aaaa');
        vault.setSecret(memDir, 'ROUND', 'trip-value');
        const buf = vault.exportBundle(memDir, 'node-master-aaaa', 'bundle-pw-123');
        process.stdout.write(buf.toString('hex'));
        "#,
    );
    let bundle = hex::decode(bundle_hex.trim()).expect("hex bundle");

    // Rust imports into a fresh directory.
    let t_rust = tmp();
    let v = FileVaultStore::new();
    v.import_bundle(
        t_rust.path(),
        &bundle,
        "bundle-pw-123",
        false,
        "new-master-rust",
    )
    .unwrap();
    v.unlock(t_rust.path(), "new-master-rust", 60_000).unwrap();
    assert_eq!(
        v.get_secret(t_rust.path(), "ROUND").unwrap(),
        Some("trip-value".into())
    );
}

// ─── bundle: Rust export → Node import ───────────────────────────────

#[test]
fn bundle_export_rust_import_node() {
    let t_rust = tmp();
    let v = FileVaultStore::new();
    v.init(t_rust.path(), "rust-src-master").unwrap();
    v.set_secret(t_rust.path(), "FOO", "bar-across").unwrap();
    let bundle = v
        .export_bundle(t_rust.path(), "rust-src-master", Some("bp-xyz"))
        .unwrap();
    let bundle_hex = hex::encode(&bundle);

    let t_node = tmp();
    let got = run_node_with_vault(
        t_node.path(),
        &format!(
            r#"
            const bundle = Buffer.from('{bundle_hex}', 'hex');
            vault.importBundle(memDir, bundle, 'bp-xyz', false, 'node-new-master');
            vault.unlock(memDir, 'node-new-master');
            process.stdout.write(vault.getSecret(memDir, 'FOO') || 'MISSING');
            "#
        ),
    );
    assert_eq!(got.trim(), "bar-across");
}

// ─── gitignore parity ────────────────────────────────────────────────

#[test]
fn gitignore_written_by_node_is_respected_by_rust_init() {
    // Node writes gitignore via init; Rust should not duplicate.
    let t = tmp();
    run_node_with_vault(
        t.path(),
        r#"
        vault.init(memDir, 'ignored-parity-pw');
        process.stdout.write('ok');
        "#,
    );
    let gi_path = t.path().join(".gitignore");
    let before = std::fs::read_to_string(&gi_path).unwrap();
    // Rust has no re-init op, but we can re-open with status (which
    // doesn't rewrite gitignore). Confirm file exists + is not empty.
    let v = FileVaultStore::new();
    let s = v.status(t.path()).unwrap();
    assert!(s.initialized);
    let after = std::fs::read_to_string(&gi_path).unwrap();
    assert_eq!(before, after);
}
