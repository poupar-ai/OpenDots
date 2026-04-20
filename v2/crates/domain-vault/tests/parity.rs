//! Cross-language parity tests: Rust domain-vault ↔ Node crypto.
//!
//! Strategy: rather than require()-ing vault.cjs (which has filesystem
//! side-effects and no exports for the pure crypto funcs), we reimplement
//! the same Node `crypto` calls inline with identical params. If the
//! byte output diverges from Rust, either Rust drifted from .cjs or
//! Node's primitives themselves drift — the former would be a bug in
//! the port.
//!
//! Every scrypt call burns ~300ms so the tests group operations to
//! minimize hash count. Tests that need PBKDF2 use small iteration
//! counts (not the production 600k) since the Rust / Node outputs
//! are independent of iteration count for parity purposes.

use std::io::Write;
use std::process::{Command, Stdio};

use domain_vault::{
    build_export_bundle_deterministic, build_header, decrypt_with_key, derive_export_key,
    encrypt_with_key_deterministic, hash_password_pbkdf2_sha512, hash_password_scrypt,
    unpack_export_bundle, verify_password, AuthBlock, EXPORT_MAGIC, KEY_LEN, MAGIC_V2, NONCE_LEN,
    PW_HASH_LEN, SALT_LEN,
};

// ─── Helpers to shell out to Node ─────────────────────────────────────

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
            "node failed: status={:?}\nstderr={}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    String::from_utf8(out.stdout).expect("utf8")
}

fn hex_to_bytes(s: &str) -> Vec<u8> {
    hex::decode(s.trim()).expect("hex")
}

fn bytes_to_hex(b: &[u8]) -> String {
    hex::encode(b)
}

// ─── Constants ──────────────────────────────────────────────────────

// Mirror the exact .cjs scrypt params — if these drift, vaults produced
// by one side become unreadable by the other.
const NODE_SCRYPT_OPTS: &str = "{ N: 1<<15, r: 8, p: 1, maxmem: 64*1024*1024 }";

// ─── parse / build header ────────────────────────────────────────────

#[test]
fn parity_build_header_matches_node() {
    // Node-side header construction, copied from vault.cjs#buildHeader.
    let salt = (0..SALT_LEN as u8).collect::<Vec<u8>>();
    let salt_hex = bytes_to_hex(&salt);
    let script = format!(
        r#"
        const crypto = require('crypto');
        const MAGIC_V2 = Buffer.from('PMV2', 'ascii');
        const salt = Buffer.from('{salt_hex}', 'hex');
        const saltLenBuf = Buffer.alloc(2);
        saltLenBuf.writeUInt16BE(salt.length, 0);
        const header = Buffer.concat([MAGIC_V2, saltLenBuf, salt]);
        process.stdout.write(header.toString('hex'));
        "#
    );
    let js = run_node(&script);
    let rust = build_header(MAGIC_V2, &salt);
    assert_eq!(bytes_to_hex(&rust), js);
}

// ─── AES-256-GCM ─────────────────────────────────────────────────────

#[test]
fn parity_rust_encrypts_node_decrypts() {
    // Fixed salt/nonce/key so the test is deterministic cross-language.
    let key = [7u8; KEY_LEN];
    let salt = [1u8; SALT_LEN];
    let nonce = [2u8; NONCE_LEN];
    let plaintext = b"parity plaintext payload";

    let file = encrypt_with_key_deterministic(&key, plaintext, &salt, &nonce).unwrap();
    let file_hex = bytes_to_hex(&file);

    let script = format!(
        r#"
        const crypto = require('crypto');
        const buf = Buffer.from('{file_hex}', 'hex');
        // Inline parseFile equivalent to vault.cjs#parseFile
        const saltLen = buf.readUInt16BE(4);
        const saltStart = 6;
        const saltEnd = saltStart + saltLen;
        const header = buf.slice(0, saltEnd);
        const nonce = buf.slice(saltEnd, saltEnd + 12);
        const tag = buf.slice(buf.length - 16);
        const ct = buf.slice(saltEnd + 12, buf.length - 16);
        const key = Buffer.alloc(32, 7);
        const decipher = crypto.createDecipheriv('aes-256-gcm', key, nonce);
        decipher.setAAD(header);
        decipher.setAuthTag(tag);
        const plain = Buffer.concat([decipher.update(ct), decipher.final()]);
        process.stdout.write(plain.toString('hex'));
        "#
    );
    let js_plain = hex_to_bytes(&run_node(&script));
    assert_eq!(js_plain, plaintext);
}

#[test]
fn parity_node_encrypts_rust_decrypts() {
    let key_hex = "07".repeat(KEY_LEN);
    let plaintext = b"payload from node";
    let plaintext_hex = bytes_to_hex(plaintext);
    let script = format!(
        r#"
        const crypto = require('crypto');
        const MAGIC_V2 = Buffer.from('PMV2', 'ascii');
        const key = Buffer.from('{key_hex}', 'hex');
        const plaintext = Buffer.from('{plaintext_hex}', 'hex');
        // Deterministic salt/nonce for test reproducibility.
        const salt = Buffer.alloc(32, 0x05);
        const nonce = Buffer.alloc(12, 0x06);
        const saltLenBuf = Buffer.alloc(2);
        saltLenBuf.writeUInt16BE(salt.length, 0);
        const header = Buffer.concat([MAGIC_V2, saltLenBuf, salt]);
        const cipher = crypto.createCipheriv('aes-256-gcm', key, nonce);
        cipher.setAAD(header);
        const ct = Buffer.concat([cipher.update(plaintext), cipher.final()]);
        const tag = cipher.getAuthTag();
        const file = Buffer.concat([header, nonce, ct, tag]);
        process.stdout.write(file.toString('hex'));
        "#
    );
    let file = hex_to_bytes(&run_node(&script));
    let key = vec![7u8; KEY_LEN];
    let decrypted = decrypt_with_key(&file, &key).unwrap();
    assert_eq!(decrypted, plaintext);
}

#[test]
fn parity_encrypt_byte_for_byte_identical() {
    // Ensures Rust and Node produce the *same* ciphertext for identical
    // inputs. AES-GCM is deterministic given (key, nonce, aad, plaintext),
    // so outputs must match byte-for-byte.
    let key = [0x42u8; KEY_LEN];
    let salt = [0x11u8; SALT_LEN];
    let nonce = [0x22u8; NONCE_LEN];
    let plaintext = b"ciphertext-must-match-between-rust-and-node";

    let rust_file = encrypt_with_key_deterministic(&key, plaintext, &salt, &nonce).unwrap();

    let plaintext_hex = bytes_to_hex(plaintext);
    let script = format!(
        r#"
        const crypto = require('crypto');
        const MAGIC_V2 = Buffer.from('PMV2', 'ascii');
        const key = Buffer.alloc(32, 0x42);
        const salt = Buffer.alloc(32, 0x11);
        const nonce = Buffer.alloc(12, 0x22);
        const plaintext = Buffer.from('{plaintext_hex}', 'hex');
        const saltLenBuf = Buffer.alloc(2);
        saltLenBuf.writeUInt16BE(salt.length, 0);
        const header = Buffer.concat([MAGIC_V2, saltLenBuf, salt]);
        const cipher = crypto.createCipheriv('aes-256-gcm', key, nonce);
        cipher.setAAD(header);
        const ct = Buffer.concat([cipher.update(plaintext), cipher.final()]);
        const tag = cipher.getAuthTag();
        const file = Buffer.concat([header, nonce, ct, tag]);
        process.stdout.write(file.toString('hex'));
        "#
    );
    let node_file = hex_to_bytes(&run_node(&script));
    assert_eq!(bytes_to_hex(&rust_file), bytes_to_hex(&node_file));
}

// ─── scrypt ──────────────────────────────────────────────────────────

#[test]
fn parity_scrypt_matches_node() {
    // Single scrypt call on both sides — expensive (~300ms each).
    let password = b"parity-scrypt-password";
    let salt = b"parity-scrypt-salt-16b";
    let script = format!(
        r#"
        const crypto = require('crypto');
        const pw = Buffer.from({password_json});
        const salt = Buffer.from({salt_json});
        const hash = crypto.scryptSync(pw.toString('utf8'), salt, {hash_len}, {opts});
        process.stdout.write(hash.toString('hex'));
        "#,
        password_json = serde_json::to_string(password).unwrap(),
        salt_json = serde_json::to_string(salt).unwrap(),
        hash_len = PW_HASH_LEN,
        opts = NODE_SCRYPT_OPTS,
    );
    let node_hash = hex_to_bytes(&run_node(&script));
    let rust_hash = hash_password_scrypt(password, salt).unwrap();
    assert_eq!(bytes_to_hex(&rust_hash), bytes_to_hex(&node_hash));
}

#[test]
fn parity_derive_export_key_matches_node() {
    // Export key derivation is also scrypt but with KEY_LEN output.
    let password = b"bundle-pass-xyz";
    let salt = (0..SALT_LEN as u8).map(|i| i ^ 0xA5).collect::<Vec<u8>>();
    let password_hex = bytes_to_hex(password);
    let salt_hex = bytes_to_hex(&salt);
    let script = format!(
        r#"
        const crypto = require('crypto');
        const pw = Buffer.from('{password_hex}', 'hex');
        const salt = Buffer.from('{salt_hex}', 'hex');
        const key = crypto.scryptSync(pw.toString('utf8'), salt, {key_len}, {opts});
        process.stdout.write(key.toString('hex'));
        "#,
        key_len = KEY_LEN,
        opts = NODE_SCRYPT_OPTS,
    );
    let node_key = hex_to_bytes(&run_node(&script));
    let rust_key = derive_export_key(password, &salt).unwrap();
    assert_eq!(bytes_to_hex(&rust_key), bytes_to_hex(&node_key));
}

// ─── PBKDF2-SHA512 ──────────────────────────────────────────────────

#[test]
fn parity_pbkdf2_sha512_matches_node() {
    let password = b"legacy-pbkdf2-password";
    let salt = b"legacy-salt-bytes";
    let iters = 1000u32;
    let password_hex = bytes_to_hex(password);
    let salt_hex = bytes_to_hex(salt);
    let script = format!(
        r#"
        const crypto = require('crypto');
        const pw = Buffer.from('{password_hex}', 'hex');
        const salt = Buffer.from('{salt_hex}', 'hex');
        const hash = crypto.pbkdf2Sync(pw.toString('utf8'), salt, {iters}, {hash_len}, 'sha512');
        process.stdout.write(hash.toString('hex'));
        "#,
        hash_len = PW_HASH_LEN,
    );
    let node_hash = hex_to_bytes(&run_node(&script));
    let rust_hash = hash_password_pbkdf2_sha512(password, salt, iters);
    assert_eq!(bytes_to_hex(&rust_hash), bytes_to_hex(&node_hash));
}

#[test]
fn parity_legacy_pbkdf2_derives_same_key_as_cjs() {
    // The legacy vault migration path derives a 32-byte key via
    // `pbkdf2Sync(password, salt, KDF_ITERS=600000, KEY_LEN=32, 'sha512')`.
    // That's production-size — use fewer iters for parity speed.
    let password = b"legacypw";
    let salt = vec![0x33u8; SALT_LEN];
    let iters = 5000u32;
    let password_hex = bytes_to_hex(password);
    let salt_hex = bytes_to_hex(&salt);
    let script = format!(
        r#"
        const crypto = require('crypto');
        const pw = Buffer.from('{password_hex}', 'hex');
        const salt = Buffer.from('{salt_hex}', 'hex');
        const key = crypto.pbkdf2Sync(pw.toString('utf8'), salt, {iters}, {key_len}, 'sha512');
        process.stdout.write(key.toString('hex'));
        "#,
        key_len = KEY_LEN,
    );
    let node_key = hex_to_bytes(&run_node(&script));
    let rust_key = hash_password_pbkdf2_sha512(password, &salt, iters);
    // `hash_password_pbkdf2_sha512` uses output len = PW_HASH_LEN(64); we need
    // to compare against a 32-byte key, so truncate the first 32 bytes of a
    // 32-byte-specific derivation. Recompute separately.
    let mut rust_key_32 = vec![0u8; KEY_LEN];
    pbkdf2::pbkdf2::<hmac::Hmac<sha2::Sha512>>(password, &salt, iters, &mut rust_key_32)
        .expect("pbkdf2");
    let _ = rust_key; // keep name readable above; avoid unused warn
    assert_eq!(bytes_to_hex(&rust_key_32), bytes_to_hex(&node_key));
}

// ─── AuthBlock verification ─────────────────────────────────────────

#[test]
fn parity_verify_password_scrypt_cross_lang() {
    // Node hashes with scrypt → we feed hash+salt into Rust verify_password.
    let password = b"human-master-pw";
    let salt = b"auth-salt-16byte";
    let password_hex = bytes_to_hex(password);
    let salt_hex = bytes_to_hex(salt);
    let script = format!(
        r#"
        const crypto = require('crypto');
        const pw = Buffer.from('{password_hex}', 'hex');
        const salt = Buffer.from('{salt_hex}', 'hex');
        const hash = crypto.scryptSync(pw.toString('utf8'), salt, {hash_len}, {opts});
        process.stdout.write(JSON.stringify({{
            password_hash: hash.toString('base64'),
            password_salt: salt.toString('base64'),
        }}));
        "#,
        hash_len = PW_HASH_LEN,
        opts = NODE_SCRYPT_OPTS,
    );
    let json_out = run_node(&script);
    let v: serde_json::Value = serde_json::from_str(&json_out).unwrap();
    let auth = AuthBlock {
        kdf: Some("scrypt".into()),
        password_hash: v["password_hash"].as_str().unwrap().to_string(),
        password_salt: v["password_salt"].as_str().unwrap().to_string(),
        iters: None,
        n: Some(1u32 << 15),
        r: Some(8),
        p: Some(1),
    };
    assert!(verify_password(password, &auth));
    assert!(!verify_password(b"wrong", &auth));
}

#[test]
fn parity_verify_password_legacy_pbkdf2_cross_lang() {
    let password = b"old-master-pw";
    let salt = b"pbkdf2-salt-bytes!";
    let iters = 1500u32;
    let password_hex = bytes_to_hex(password);
    let salt_hex = bytes_to_hex(salt);
    let script = format!(
        r#"
        const crypto = require('crypto');
        const pw = Buffer.from('{password_hex}', 'hex');
        const salt = Buffer.from('{salt_hex}', 'hex');
        const hash = crypto.pbkdf2Sync(pw.toString('utf8'), salt, {iters}, {hash_len}, 'sha512');
        process.stdout.write(JSON.stringify({{
            password_hash: hash.toString('base64'),
            password_salt: salt.toString('base64'),
        }}));
        "#,
        hash_len = PW_HASH_LEN,
    );
    let json_out = run_node(&script);
    let v: serde_json::Value = serde_json::from_str(&json_out).unwrap();
    let auth = AuthBlock {
        kdf: None, // legacy — no kdf field
        password_hash: v["password_hash"].as_str().unwrap().to_string(),
        password_salt: v["password_salt"].as_str().unwrap().to_string(),
        iters: Some(iters),
        n: None,
        r: None,
        p: None,
    };
    assert!(verify_password(password, &auth));
    assert!(!verify_password(b"incorrect", &auth));
}

// ─── Export bundle ───────────────────────────────────────────────────

#[test]
fn parity_export_bundle_rust_to_node() {
    // Rust creates bundle → Node decrypts.
    let vault_bytes = {
        // A minimal-but-valid PMV2 file, produced by Rust itself.
        let key = [9u8; KEY_LEN];
        let salt = [8u8; SALT_LEN];
        let nonce = [7u8; NONCE_LEN];
        encrypt_with_key_deterministic(&key, b"{}", &salt, &nonce).unwrap()
    };
    let device_key = vec![0xAAu8; KEY_LEN];
    let bundle_pw = b"bundle-password-x";
    let salt = vec![0xC1u8; SALT_LEN];
    let nonce = vec![0xC2u8; NONCE_LEN];
    let exported_at = "2026-04-19T00:00:00.000Z";

    let bundle = build_export_bundle_deterministic(
        &vault_bytes,
        &device_key,
        bundle_pw,
        exported_at,
        &salt,
        &nonce,
    )
    .unwrap();
    let bundle_hex = bytes_to_hex(&bundle);
    let pw_hex = bytes_to_hex(bundle_pw);

    let script = format!(
        r#"
        const crypto = require('crypto');
        const EXPORT_MAGIC = Buffer.from('PMVEXP1', 'ascii');
        const bundle = Buffer.from('{bundle_hex}', 'hex');
        const pw = Buffer.from('{pw_hex}', 'hex').toString('utf8');
        const salt = bundle.slice(EXPORT_MAGIC.length, EXPORT_MAGIC.length + 32);
        const nonce = bundle.slice(EXPORT_MAGIC.length + 32, EXPORT_MAGIC.length + 32 + 12);
        const tag = bundle.slice(bundle.length - 16);
        const ct = bundle.slice(EXPORT_MAGIC.length + 32 + 12, bundle.length - 16);
        const key = crypto.scryptSync(pw, salt, 32, {opts});
        const decipher = crypto.createDecipheriv('aes-256-gcm', key, nonce);
        decipher.setAAD(Buffer.concat([EXPORT_MAGIC, salt]));
        decipher.setAuthTag(tag);
        const plain = Buffer.concat([decipher.update(ct), decipher.final()]);
        const payload = JSON.parse(plain.toString('utf8'));
        process.stdout.write(JSON.stringify(payload));
        "#,
        opts = NODE_SCRYPT_OPTS,
    );
    let json_out = run_node(&script);
    let v: serde_json::Value = serde_json::from_str(&json_out).unwrap();
    assert_eq!(v["__version"], serde_json::json!(1));
    assert_eq!(v["exported_at"].as_str().unwrap(), exported_at);
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let decoded_vault = b64.decode(v["vault_b64"].as_str().unwrap()).unwrap();
    let decoded_dk = b64.decode(v["device_key_b64"].as_str().unwrap()).unwrap();
    assert_eq!(decoded_vault, vault_bytes);
    assert_eq!(decoded_dk, device_key);
}

#[test]
fn parity_export_bundle_node_to_rust() {
    // Node builds bundle → Rust unpacks.
    let vault_bytes = {
        let key = [1u8; KEY_LEN];
        let salt = [2u8; SALT_LEN];
        let nonce = [3u8; NONCE_LEN];
        encrypt_with_key_deterministic(&key, b"{}", &salt, &nonce).unwrap()
    };
    let device_key = vec![0xDDu8; KEY_LEN];
    let bundle_pw = b"from-node-password";
    let vault_hex = bytes_to_hex(&vault_bytes);
    let dk_hex = bytes_to_hex(&device_key);
    let pw_hex = bytes_to_hex(bundle_pw);
    let script = format!(
        r#"
        const crypto = require('crypto');
        const EXPORT_MAGIC = Buffer.from('PMVEXP1', 'ascii');
        const SALT_LEN = 32; const NONCE_LEN = 12; const KEY_LEN = 32;
        const vaultBuf = Buffer.from('{vault_hex}', 'hex');
        const dkBuf = Buffer.from('{dk_hex}', 'hex');
        const pw = Buffer.from('{pw_hex}', 'hex').toString('utf8');
        const payload = Buffer.from(JSON.stringify({{
            __version: 1,
            exported_at: '2026-04-19T12:00:00.000Z',
            vault_b64: vaultBuf.toString('base64'),
            device_key_b64: dkBuf.toString('base64'),
        }}), 'utf8');
        const salt = Buffer.alloc(SALT_LEN, 0x44);
        const nonce = Buffer.alloc(NONCE_LEN, 0x55);
        const key = crypto.scryptSync(pw, salt, KEY_LEN, {opts});
        const cipher = crypto.createCipheriv('aes-256-gcm', key, nonce);
        cipher.setAAD(Buffer.concat([EXPORT_MAGIC, salt]));
        const ct = Buffer.concat([cipher.update(payload), cipher.final()]);
        const tag = cipher.getAuthTag();
        const bundle = Buffer.concat([EXPORT_MAGIC, salt, nonce, ct, tag]);
        process.stdout.write(bundle.toString('hex'));
        "#,
        opts = NODE_SCRYPT_OPTS,
    );
    let bundle = hex_to_bytes(&run_node(&script));
    let unpacked = unpack_export_bundle(&bundle, bundle_pw).unwrap();
    assert_eq!(unpacked.vault_bytes, vault_bytes);
    assert_eq!(unpacked.device_key, device_key);
    assert_eq!(
        unpacked.exported_at.as_deref(),
        Some("2026-04-19T12:00:00.000Z")
    );
    // Sanity: bundle starts with the correct magic and length check matches EXPORT_MAGIC.
    assert_eq!(&bundle[..EXPORT_MAGIC.len()], EXPORT_MAGIC);
}

// ─── Tamper detection ───────────────────────────────────────────────

#[test]
fn parity_tampered_ciphertext_rejected_by_both() {
    let key = [0xFFu8; KEY_LEN];
    let salt = [0x01u8; SALT_LEN];
    let nonce = [0x02u8; NONCE_LEN];
    let mut file = encrypt_with_key_deterministic(&key, b"dont tamper", &salt, &nonce).unwrap();
    let last = file.len() - 1;
    file[last] ^= 0x01; // flip a tag bit

    // Rust rejects
    assert!(decrypt_with_key(&file, &key).is_err());

    // Node rejects with the same header/salt/nonce structure
    let file_hex = bytes_to_hex(&file);
    let script = format!(
        r#"
        const crypto = require('crypto');
        const buf = Buffer.from('{file_hex}', 'hex');
        const saltLen = buf.readUInt16BE(4);
        const saltEnd = 6 + saltLen;
        const header = buf.slice(0, saltEnd);
        const nonce = buf.slice(saltEnd, saltEnd + 12);
        const tag = buf.slice(buf.length - 16);
        const ct = buf.slice(saltEnd + 12, buf.length - 16);
        const key = Buffer.alloc(32, 0xFF);
        const decipher = crypto.createDecipheriv('aes-256-gcm', key, nonce);
        decipher.setAAD(header);
        decipher.setAuthTag(tag);
        try {{
            Buffer.concat([decipher.update(ct), decipher.final()]);
            process.stdout.write('UNEXPECTED_OK');
        }} catch (e) {{
            process.stdout.write('REJECTED');
        }}
        "#
    );
    assert_eq!(run_node(&script).trim(), "REJECTED");
}

// Avoid unused-import warnings when features change.
#[allow(dead_code)]
fn _silence_writer() {
    let mut v = Vec::new();
    v.write_all(b"x").unwrap();
}
