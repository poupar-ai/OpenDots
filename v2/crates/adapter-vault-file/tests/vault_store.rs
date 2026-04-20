//! Testes unitários do `FileVaultStore`. Cobrem o contrato completo do
//! port `VaultStore` (lifecycle, secrets, envfiles, agent access,
//! bundles, destroy) + invariantes do adapter (perms 0600, gitignore,
//! cache de device-key, TTL).

use std::fs;
use std::path::Path;

use adapter_vault_file::{FileVaultStore, DEVICE_KEY_FILE, VAULT_FILE};
use core_ports::{VaultError, VaultStore};
use tempfile::TempDir;

fn store() -> (FileVaultStore, TempDir) {
    let t = TempDir::new().unwrap();
    (FileVaultStore::new(), t)
}

fn mem(t: &TempDir) -> &Path {
    t.path()
}

// ─── status / init ───────────────────────────────────────────────────

#[test]
fn status_reports_uninitialized_on_empty_dir() {
    let (v, t) = store();
    let s = v.status(mem(&t)).unwrap();
    assert!(!s.initialized);
    assert!(s.locked);
    assert!(!s.needs_migration);
    assert_eq!(s.expires_at, None);
    assert_eq!(s.ttl_ms, None);
}

#[test]
fn init_creates_vault_devicekey_and_gitignore() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    assert!(mem(&t).join(VAULT_FILE).exists());
    assert!(mem(&t).join(DEVICE_KEY_FILE).exists());
    assert!(mem(&t).join(".gitignore").exists());
    let ig = fs::read_to_string(mem(&t).join(".gitignore")).unwrap();
    assert!(ig.contains(VAULT_FILE));
    assert!(ig.contains(DEVICE_KEY_FILE));
}

#[test]
fn init_returns_unlocked_status() {
    let (v, t) = store();
    let s = v.init(mem(&t), "correct-horse").unwrap();
    assert!(s.initialized);
    assert!(!s.locked);
    assert!(s.expires_at.is_some());
    assert!(s.ttl_ms.is_some());
}

#[test]
fn init_rejects_second_init() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    let err = v.init(mem(&t), "correct-horse").unwrap_err();
    assert!(matches!(err, VaultError::AlreadyInitialized));
}

#[test]
fn init_rejects_short_password() {
    let (v, t) = store();
    let err = v.init(mem(&t), "short").unwrap_err();
    assert!(matches!(err, VaultError::Validation(_)));
}

#[cfg(unix)]
#[test]
fn init_sets_0600_on_sensitive_files() {
    use std::os::unix::fs::PermissionsExt;
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    for name in [VAULT_FILE, DEVICE_KEY_FILE] {
        let p = mem(&t).join(name);
        let mode = fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{} should be 0600, was {:o}", name, mode);
    }
}

// ─── unlock / lock ───────────────────────────────────────────────────

#[test]
fn unlock_wrong_password_fails() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    v.lock();
    let err = v.unlock(mem(&t), "wrong-horse", 60_000).unwrap_err();
    assert!(matches!(err, VaultError::WrongPassword));
}

#[test]
fn unlock_correct_password_succeeds() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    v.lock();
    let s = v.unlock(mem(&t), "correct-horse", 60_000).unwrap();
    assert!(!s.locked);
    assert_eq!(s.ttl_ms, Some(60_000));
}

#[test]
fn unlock_not_initialized_fails() {
    let (v, t) = store();
    let err = v.unlock(mem(&t), "any-password", 60_000).unwrap_err();
    assert!(matches!(err, VaultError::NotInitialized));
}

#[test]
fn lock_clears_session() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    assert!(!v.status(mem(&t)).unwrap().locked);
    v.lock();
    assert!(v.status(mem(&t)).unwrap().locked);
}

// ─── change_password ─────────────────────────────────────────────────

#[test]
fn change_password_requires_unlock() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    v.lock();
    let err = v
        .change_password(mem(&t), "correct-horse", "new-good-pass")
        .unwrap_err();
    assert!(matches!(err, VaultError::Locked));
}

#[test]
fn change_password_rejects_wrong_old() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    let err = v
        .change_password(mem(&t), "wrong-old-pass", "new-good-pass")
        .unwrap_err();
    assert!(matches!(err, VaultError::WrongPassword));
}

#[test]
fn change_password_rejects_short_new() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    let err = v
        .change_password(mem(&t), "correct-horse", "short")
        .unwrap_err();
    assert!(matches!(err, VaultError::Validation(_)));
}

#[test]
fn change_password_succeeds_and_new_unlock_works() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    v.change_password(mem(&t), "correct-horse", "brand-new-pass")
        .unwrap();
    v.lock();
    let err = v.unlock(mem(&t), "correct-horse", 60_000).unwrap_err();
    assert!(matches!(err, VaultError::WrongPassword));
    let s = v.unlock(mem(&t), "brand-new-pass", 60_000).unwrap();
    assert!(!s.locked);
}

// ─── secrets CRUD ────────────────────────────────────────────────────

#[test]
fn secrets_roundtrip() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    assert_eq!(v.list_keys(mem(&t)).unwrap(), Vec::<String>::new());

    v.set_secret(mem(&t), "API_KEY", "sk_live_abc").unwrap();
    v.set_secret(mem(&t), "DB_URL", "postgres://x").unwrap();
    let mut keys = v.list_keys(mem(&t)).unwrap();
    keys.sort();
    assert_eq!(keys, vec!["API_KEY".to_string(), "DB_URL".to_string()]);

    assert_eq!(
        v.get_secret(mem(&t), "API_KEY").unwrap(),
        Some("sk_live_abc".into())
    );
    assert_eq!(v.get_secret(mem(&t), "MISSING").unwrap(), None);

    assert!(v.delete_secret(mem(&t), "API_KEY").unwrap());
    assert!(!v.delete_secret(mem(&t), "API_KEY").unwrap());
    assert_eq!(v.list_keys(mem(&t)).unwrap(), vec!["DB_URL".to_string()]);
}

#[test]
fn secrets_require_unlock() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    v.lock();
    assert!(matches!(
        v.list_keys(mem(&t)).unwrap_err(),
        VaultError::Locked
    ));
    assert!(matches!(
        v.set_secret(mem(&t), "K", "v").unwrap_err(),
        VaultError::Locked
    ));
    assert!(matches!(
        v.get_secret(mem(&t), "K").unwrap_err(),
        VaultError::Locked
    ));
    assert!(matches!(
        v.delete_secret(mem(&t), "K").unwrap_err(),
        VaultError::Locked
    ));
}

#[test]
fn secrets_validate_name_and_value() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    assert!(matches!(
        v.set_secret(mem(&t), "bad name", "x").unwrap_err(),
        VaultError::Validation(_)
    ));
    // Empty value is allowed (domain-vault matches .cjs: only length
    // cap > 8192 is rejected).
    v.set_secret(mem(&t), "EMPTY_OK", "").unwrap();
    let oversized = "x".repeat(8193);
    assert!(matches!(
        v.set_secret(mem(&t), "TOO_BIG", &oversized).unwrap_err(),
        VaultError::Validation(_)
    ));
}

// ─── envfiles CRUD ───────────────────────────────────────────────────

#[test]
fn envfiles_roundtrip() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    assert!(v.list_envfiles(mem(&t)).unwrap().is_empty());

    v.set_envfile(mem(&t), "app.env", "A=1\nB=2\n").unwrap();
    let entries = v.list_envfiles(mem(&t)).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "app.env");
    assert_eq!(entries[0].size, "A=1\nB=2\n".len());
    assert!(entries[0].updated_at.is_some());

    assert_eq!(
        v.get_envfile(mem(&t), "app.env").unwrap(),
        Some("A=1\nB=2\n".into())
    );

    assert!(v.delete_envfile(mem(&t), "app.env").unwrap());
    assert!(!v.delete_envfile(mem(&t), "app.env").unwrap());
    assert!(v.list_envfiles(mem(&t)).unwrap().is_empty());
}

#[test]
fn envfiles_require_unlock() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    v.lock();
    assert!(matches!(
        v.list_envfiles(mem(&t)).unwrap_err(),
        VaultError::Locked
    ));
    assert!(matches!(
        v.set_envfile(mem(&t), "x.env", "K=V").unwrap_err(),
        VaultError::Locked
    ));
}

// ─── agent access ────────────────────────────────────────────────────

#[test]
fn get_secrets_for_agent_returns_empty_when_vault_missing() {
    let (v, t) = store();
    let got = v.get_secrets_for_agent(mem(&t), &["ANY".into()]).unwrap();
    assert!(got.is_empty());
}

#[test]
fn get_secrets_for_agent_returns_empty_on_empty_names() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    let got = v.get_secrets_for_agent(mem(&t), &[]).unwrap();
    assert!(got.is_empty());
}

#[test]
fn get_secrets_for_agent_works_without_unlock() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    v.set_secret(mem(&t), "STRIPE_KEY", "sk_live_xyz").unwrap();
    v.set_secret(mem(&t), "OTHER", "value").unwrap();
    v.lock();
    let got = v
        .get_secrets_for_agent(mem(&t), &["STRIPE_KEY".into(), "MISSING".into()])
        .unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got["STRIPE_KEY"], "sk_live_xyz");
}

// ─── bundles ─────────────────────────────────────────────────────────

#[test]
fn export_requires_unlock() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    v.lock();
    let err = v.export_bundle(mem(&t), "correct-horse", None).unwrap_err();
    assert!(matches!(err, VaultError::Locked));
}

#[test]
fn export_rejects_wrong_master() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    let err = v.export_bundle(mem(&t), "wrong-master", None).unwrap_err();
    assert!(matches!(err, VaultError::WrongPassword));
}

#[test]
fn export_import_roundtrip_to_empty_dir() {
    let (v1, t1) = store();
    v1.init(mem(&t1), "correct-horse").unwrap();
    v1.set_secret(mem(&t1), "API_KEY", "sk_roundtrip").unwrap();
    v1.set_envfile(mem(&t1), "app.env", "A=1\n").unwrap();
    let bundle = v1
        .export_bundle(mem(&t1), "correct-horse", Some("bundle-pass"))
        .unwrap();

    let (v2, t2) = store();
    v2.import_bundle(mem(&t2), &bundle, "bundle-pass", false, "new-master")
        .unwrap();
    // Import locks. Must unlock with the new master password.
    assert!(v2.status(mem(&t2)).unwrap().locked);
    v2.unlock(mem(&t2), "new-master", 60_000).unwrap();

    assert_eq!(
        v2.get_secret(mem(&t2), "API_KEY").unwrap(),
        Some("sk_roundtrip".into())
    );
    assert_eq!(
        v2.get_envfile(mem(&t2), "app.env").unwrap(),
        Some("A=1\n".into())
    );
}

#[test]
fn import_rejects_existing_without_replace() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    let bundle = v
        .export_bundle(mem(&t), "correct-horse", Some("bp"))
        .unwrap();
    let err = v
        .import_bundle(mem(&t), &bundle, "bp", false, "new-master")
        .unwrap_err();
    assert!(matches!(err, VaultError::AlreadyInitialized));
}

#[test]
fn import_requires_unlock_when_replacing() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    let bundle = v
        .export_bundle(mem(&t), "correct-horse", Some("bp"))
        .unwrap();
    v.lock();
    let err = v
        .import_bundle(mem(&t), &bundle, "bp", true, "new-master")
        .unwrap_err();
    assert!(matches!(err, VaultError::Locked));
}

#[test]
fn import_rejects_short_new_master() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    let bundle = v
        .export_bundle(mem(&t), "correct-horse", Some("bp"))
        .unwrap();
    let (v2, t2) = store();
    let err = v2
        .import_bundle(mem(&t2), &bundle, "bp", false, "short")
        .unwrap_err();
    assert!(matches!(err, VaultError::Validation(_)));
}

// ─── destroy ─────────────────────────────────────────────────────────

#[test]
fn destroy_removes_files_and_locks() {
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    assert!(mem(&t).join(VAULT_FILE).exists());
    assert!(mem(&t).join(DEVICE_KEY_FILE).exists());
    v.destroy(mem(&t)).unwrap();
    assert!(!mem(&t).join(VAULT_FILE).exists());
    assert!(!mem(&t).join(DEVICE_KEY_FILE).exists());
    assert!(v.status(mem(&t)).unwrap().locked);
}

#[test]
fn destroy_is_idempotent_on_empty_dir() {
    let (v, t) = store();
    v.destroy(mem(&t)).unwrap();
    v.destroy(mem(&t)).unwrap();
}

// ─── TTL expiry ──────────────────────────────────────────────────────

#[test]
fn session_expires_when_ttl_negative() {
    // TTL = -1ms so expires_at = now - 1ms, already in the past.
    let (v, t) = store();
    v.init(mem(&t), "correct-horse").unwrap();
    v.lock();
    v.unlock(mem(&t), "correct-horse", -1).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert!(v.status(mem(&t)).unwrap().locked);
    // Secrets access must require unlock again.
    assert!(matches!(
        v.list_keys(mem(&t)).unwrap_err(),
        VaultError::Locked
    ));
}

// ─── gitignore idempotence ───────────────────────────────────────────

#[test]
fn gitignore_does_not_duplicate_entries_on_reinit_like_flow() {
    let (v, t) = store();
    // Pre-existing .gitignore with project entries.
    fs::write(mem(&t).join(".gitignore"), "node_modules\n").unwrap();
    v.init(mem(&t), "correct-horse").unwrap();
    let ig = fs::read_to_string(mem(&t).join(".gitignore")).unwrap();
    assert_eq!(ig.matches(VAULT_FILE).count(), 1);
    assert_eq!(ig.matches(DEVICE_KEY_FILE).count(), 1);
    assert!(ig.contains("node_modules"));
}
