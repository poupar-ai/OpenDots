//! Filesystem-backed implementation of [`core_ports::VaultStore`].
//!
//! Wires the pure crypto functions from `domain-vault` to disk I/O,
//! replicating the semantics of `plugin/scripts/vault.cjs`:
//!
//! - Atomic writes via `tmp-<rand>` + rename, with `O_NOFOLLOW` on Unix
//!   (protects against symlink attacks in a shared `mem_dir`).
//! - 0600 permission on `vault.enc` and `.vault-devicekey`.
//! - In-process device-key cache (`Mutex<Option<Vec<u8>>>`) — avoids a
//!   syscall per spawn; cleared on `destroy`.
//! - In-process UI session (`Mutex<Option<UiSession>>`) — unlock sets
//!   `expires_at = now + ttl`, `lock` clears. Expiry is *checked*
//!   lazily on every `require_ui_unlock`.
//! - Legacy PMV1 migration happens inside `unlock` automatically.
//! - Auth upgrade (PBKDF2 → scrypt) happens after a successful unlock
//!   and swallows errors silently — mirror of `.cjs` `try {} catch {}`.
//!
//! All crypto stays in `domain-vault`. This crate only adds bytes↔disk.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use core_ports::{EnvFileEntry, VaultError, VaultStatus, VaultStore};
use domain_vault as dv;
use rand::RngCore;
use serde_json::{Map, Value};

// ─── Constants ──────────────────────────────────────────────────────

pub const VAULT_FILE: &str = "vault.enc";
pub const DEVICE_KEY_FILE: &str = ".vault-devicekey";
pub const UNLOCK_STATE_FILE: &str = ".vault-unlock-state.json";
pub const DEFAULT_TTL_MS: i64 = 30 * 60 * 1000;

// ─── Helper error conversions ────────────────────────────────────────

fn map_vault_err(e: dv::Error) -> VaultError {
    use dv::Error as E;
    match e {
        E::WrongPassword | E::WrongLegacyPassword => VaultError::WrongPassword,
        E::BundleWrongPassword => VaultError::WrongPassword,
        E::AuthMissing => VaultError::Malformed,
        E::FileTooSmall
        | E::BadMagic
        | E::Truncated
        | E::Gcm
        | E::InvalidKeyLen
        | E::InvalidBase64
        | E::InvalidJson
        | E::BundleTruncated
        | E::BundleBadMagic
        | E::BundleUnreadable
        | E::BundleMissingFields
        | E::BundleBadDeviceKey
        | E::BundleBadVault => VaultError::Malformed,
        E::InvalidSecretName
        | E::SecretValueNotString
        | E::SecretValueTooLong
        | E::InvalidEnvfileName
        | E::EnvfileContentNotString
        | E::EnvfileTooLarge
        | E::PasswordTooShort => VaultError::Validation(e.to_string()),
        E::Scrypt(msg) => VaultError::Validation(format!("scrypt: {msg}")),
    }
}

// ─── Path helpers ───────────────────────────────────────────────────

fn vault_path(mem_dir: &Path) -> PathBuf {
    mem_dir.join(VAULT_FILE)
}

fn device_key_path(mem_dir: &Path) -> PathBuf {
    mem_dir.join(DEVICE_KEY_FILE)
}

fn gitignore_path(mem_dir: &Path) -> PathBuf {
    mem_dir.join(".gitignore")
}

// ─── Atomic write (O_NOFOLLOW | O_EXCL) ─────────────────────────────

/// Writes `data` atomically to `path`. Creates a sibling
/// `<path>.tmp-<hex>` with `O_EXCL | O_NOFOLLOW | O_CREAT | O_WRONLY`,
/// fsyncs, then renames over `path`. Symlink-attack safe: if the tmp
/// path is a symlink, `open` fails.
#[cfg(unix)]
fn write_file_safe(path: &Path, data: &[u8], mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut rand = [0u8; 6];
    rand::rng().fill_bytes(&mut rand);
    let suffix: String = rand.iter().fold(String::with_capacity(12), |mut acc, b| {
        use std::fmt::Write as _;
        let _ = write!(acc, "{:02x}", b);
        acc
    });
    let tmp = path.with_extension(format!(
        "{}.tmp-{}",
        path.extension().and_then(|s| s.to_str()).unwrap_or(""),
        suffix
    ));

    // Build flags manually — libc O_NOFOLLOW is essential here.
    let flags = libc_flags();

    let mut opts = OpenOptions::new();
    opts.write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(flags);
    let mut file = opts.open(&tmp)?;
    file.write_all(data)?;
    let _ = file.sync_all();
    drop(file);

    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(unix)]
fn libc_flags() -> i32 {
    // O_NOFOLLOW = 0x20000 on Linux, 0x0100 on macOS — use libc via
    // direct constant not ideal for portability; std::os::unix::fs
    // doesn't expose it, so pull from the public C bindings via
    // `libc` crate or inline. Avoid adding libc dep for one constant:
    // use the well-known POSIX value fallback via nix-style.
    #[cfg(target_os = "linux")]
    {
        0x20000 // O_NOFOLLOW
    }
    #[cfg(target_os = "macos")]
    {
        0x0100
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

#[cfg(not(unix))]
fn write_file_safe(path: &Path, data: &[u8], _mode: u32) -> std::io::Result<()> {
    // Windows: atomic rename semantics differ; best-effort write.
    let mut rand = [0u8; 6];
    rand::rng().fill_bytes(&mut rand);
    let suffix: String = rand.iter().fold(String::with_capacity(12), |mut acc, b| {
        use std::fmt::Write as _;
        let _ = write!(acc, "{:02x}", b);
        acc
    });
    let tmp = path.with_extension(format!(
        "{}.tmp-{}",
        path.extension().and_then(|s| s.to_str()).unwrap_or(""),
        suffix
    ));
    let mut file = File::create(&tmp)?;
    file.write_all(data)?;
    let _ = file.sync_all();
    drop(file);
    fs::rename(&tmp, path)
}

fn read_all(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

// ─── .gitignore management ──────────────────────────────────────────

fn ensure_gitignore(mem_dir: &Path) -> std::io::Result<()> {
    let p = gitignore_path(mem_dir);
    let current = if p.exists() {
        fs::read_to_string(&p).unwrap_or_default()
    } else {
        String::new()
    };
    if let Some(new) = dv::ensure_gitignore_text(&current) {
        write_file_safe(&p, new.as_bytes(), 0o644)?;
    }
    Ok(())
}

// ─── UI session state ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
struct UiSession {
    expires_at: i64,
    ttl_ms: i64,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ─── The adapter ────────────────────────────────────────────────────

#[derive(Default)]
pub struct FileVaultStore {
    cached_device_key: Mutex<Option<Vec<u8>>>,
    ui_session: Mutex<Option<UiSession>>,
}

impl FileVaultStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn has_device_key(&self, mem_dir: &Path) -> bool {
        device_key_path(mem_dir).exists()
    }

    fn load_device_key(&self, mem_dir: &Path) -> Result<Vec<u8>, VaultError> {
        if let Some(cached) = self.cached_device_key.lock().unwrap().as_ref() {
            return Ok(cached.clone());
        }
        let p = device_key_path(mem_dir);
        if !p.exists() {
            return Err(VaultError::Malformed);
        }
        let buf = read_all(&p)?;
        if buf.len() != dv::KEY_LEN {
            return Err(VaultError::Malformed);
        }
        *self.cached_device_key.lock().unwrap() = Some(buf.clone());
        Ok(buf)
    }

    fn write_device_key(&self, mem_dir: &Path, key: &[u8]) -> Result<(), VaultError> {
        write_file_safe(&device_key_path(mem_dir), key, 0o600)?;
        *self.cached_device_key.lock().unwrap() = Some(key.to_vec());
        Ok(())
    }

    fn clear_device_key_cache(&self) {
        if let Some(mut k) = self.cached_device_key.lock().unwrap().take() {
            k.iter_mut().for_each(|b| *b = 0);
        }
    }

    fn is_unlocked(&self) -> bool {
        let mut slot = self.ui_session.lock().unwrap();
        match *slot {
            Some(s) if now_ms() <= s.expires_at => true,
            Some(_) => {
                *slot = None;
                false
            }
            None => false,
        }
    }

    fn require_ui_unlock(&self) -> Result<(), VaultError> {
        if !self.is_unlocked() {
            return Err(VaultError::Locked);
        }
        Ok(())
    }

    fn current_expires_at(&self) -> Option<i64> {
        self.ui_session.lock().unwrap().map(|s| s.expires_at)
    }

    fn current_ttl_ms(&self) -> Option<i64> {
        self.ui_session.lock().unwrap().map(|s| s.ttl_ms)
    }

    fn set_session(&self, ttl_ms: i64) -> UiSession {
        let s = UiSession {
            expires_at: now_ms() + ttl_ms,
            ttl_ms,
        };
        *self.ui_session.lock().unwrap() = Some(s);
        s
    }

    /// Decrypts `vault.enc` with the device-key and normalizes the store.
    fn load_store(&self, mem_dir: &Path) -> Result<dv::Store, VaultError> {
        let file = read_all(&vault_path(mem_dir))?;
        let key = self.load_device_key(mem_dir)?;
        let plain = dv::decrypt_with_key(&file, &key).map_err(map_vault_err)?;
        let raw: Value = serde_json::from_slice(&plain).unwrap_or(Value::Object(Map::new()));
        Ok(dv::normalize_store(&raw))
    }

    /// Serializes + encrypts + writes `vault.enc` atomically.
    fn save_store(&self, mem_dir: &Path, store: &dv::Store) -> Result<(), VaultError> {
        let key = self.load_device_key(mem_dir)?;
        let plain = dv::serialize_store(store);
        let mut rng = rand::rng();
        let file = dv::encrypt_with_key(&key, &plain, &mut rng).map_err(map_vault_err)?;
        write_file_safe(&vault_path(mem_dir), &file, 0o600)?;
        Ok(())
    }

    fn status_struct(&self, mem_dir: &Path) -> VaultStatus {
        let initialized = vault_path(mem_dir).exists();
        let legacy = initialized && !self.has_device_key(mem_dir);
        let unlocked = self.is_unlocked();
        VaultStatus {
            initialized,
            locked: !unlocked || legacy,
            expires_at: if unlocked {
                self.current_expires_at()
            } else {
                None
            },
            ttl_ms: self.current_ttl_ms(),
            needs_migration: legacy,
        }
    }
}

impl VaultStore for FileVaultStore {
    // ── Lifecycle ──────────────────────────────────────────────────

    fn status(&self, mem_dir: &Path) -> Result<VaultStatus, VaultError> {
        Ok(self.status_struct(mem_dir))
    }

    fn init(&self, mem_dir: &Path, password: &str) -> Result<VaultStatus, VaultError> {
        if vault_path(mem_dir).exists() {
            return Err(VaultError::AlreadyInitialized);
        }
        if password.len() < 8 {
            return Err(VaultError::Validation(
                "password must be at least 8 chars".into(),
            ));
        }
        fs::create_dir_all(mem_dir)?;
        ensure_gitignore(mem_dir)?;
        let mut rng = rand::rng();
        let mut device_key = vec![0u8; dv::KEY_LEN];
        rng.fill_bytes(&mut device_key);
        self.write_device_key(mem_dir, &device_key)?;

        let auth = dv::build_auth_block(password.as_bytes(), &mut rng).map_err(map_vault_err)?;
        let store = dv::Store {
            secrets: Map::new(),
            envfiles: Map::new(),
            auth: Some(auth),
        };
        let plain = dv::serialize_store(&store);
        let file = dv::encrypt_with_key(&device_key, &plain, &mut rng).map_err(map_vault_err)?;
        write_file_safe(&vault_path(mem_dir), &file, 0o600)?;

        self.set_session(DEFAULT_TTL_MS);
        Ok(self.status_struct(mem_dir))
    }

    fn unlock(
        &self,
        mem_dir: &Path,
        password: &str,
        ttl_ms: i64,
    ) -> Result<VaultStatus, VaultError> {
        if !vault_path(mem_dir).exists() {
            return Err(VaultError::NotInitialized);
        }
        let mut file = read_all(&vault_path(mem_dir))?;
        let parsed = dv::parse_file(&file).map_err(map_vault_err)?;

        // Legacy migration PMV1 → PMV2
        if parsed.version == 1 {
            let mut rng = rand::rng();
            let result = dv::migrate_legacy_deterministic(&file, password.as_bytes(), &mut rng)
                .map_err(map_vault_err)?;
            // Write new device-key + new vault atomically.
            // NB: order matches .cjs — finding F19-01 is an upstream
            // concern (ordering bug on partial-failure), not fixed here
            // to preserve byte-level parity with .cjs during cutover.
            self.write_device_key(mem_dir, &result.device_key)?;
            write_file_safe(&vault_path(mem_dir), &result.new_file, 0o600)?;
            file = read_all(&vault_path(mem_dir))?;
        }

        let key = self.load_device_key(mem_dir)?;
        let plain = dv::decrypt_with_key(&file, &key).map_err(map_vault_err)?;
        let raw: Value = serde_json::from_slice(&plain).unwrap_or(Value::Object(Map::new()));
        let mut store = dv::normalize_store(&raw);
        let auth = store.auth.as_ref().ok_or(VaultError::Malformed)?;
        if !dv::verify_password(password.as_bytes(), auth) {
            return Err(VaultError::WrongPassword);
        }
        ensure_gitignore(mem_dir)?;

        // Session must be set before `save_store` (save_store doesn't
        // actually require UI unlock, but we mirror .cjs ordering).
        let session = self.set_session(ttl_ms);

        // Auth upgrade PBKDF2 → scrypt (silent failure per .cjs).
        if store.auth.as_ref().and_then(|a| a.kdf.as_deref()) != Some("scrypt") {
            let mut rng = rand::rng();
            if let Ok(new_auth) = dv::build_auth_block(password.as_bytes(), &mut rng) {
                store.auth = Some(new_auth);
                let _ = self.save_store(mem_dir, &store);
            }
        }

        Ok(VaultStatus {
            initialized: true,
            locked: false,
            expires_at: Some(session.expires_at),
            ttl_ms: Some(session.ttl_ms),
            needs_migration: false,
        })
    }

    fn lock(&self) {
        *self.ui_session.lock().unwrap() = None;
    }

    fn change_password(
        &self,
        mem_dir: &Path,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), VaultError> {
        self.require_ui_unlock()?;
        if new_password.len() < 8 {
            return Err(VaultError::Validation(
                "nova senha precisa de no mínimo 8 caracteres".into(),
            ));
        }
        let mut store = self.load_store(mem_dir)?;
        let auth = store.auth.as_ref().ok_or(VaultError::Malformed)?;
        if !dv::verify_password(old_password.as_bytes(), auth) {
            return Err(VaultError::WrongPassword);
        }
        let mut rng = rand::rng();
        store.auth =
            Some(dv::build_auth_block(new_password.as_bytes(), &mut rng).map_err(map_vault_err)?);
        self.save_store(mem_dir, &store)?;
        Ok(())
    }

    // ── Secrets ────────────────────────────────────────────────────

    fn list_keys(&self, mem_dir: &Path) -> Result<Vec<String>, VaultError> {
        self.require_ui_unlock()?;
        let store = self.load_store(mem_dir)?;
        let mut keys: Vec<String> = store.secrets.keys().cloned().collect();
        keys.sort();
        Ok(keys)
    }

    fn get_secret(&self, mem_dir: &Path, name: &str) -> Result<Option<String>, VaultError> {
        self.require_ui_unlock()?;
        let store = self.load_store(mem_dir)?;
        Ok(store
            .secrets
            .get(name)
            .and_then(|v| v.as_str().map(String::from)))
    }

    fn set_secret(&self, mem_dir: &Path, name: &str, value: &str) -> Result<(), VaultError> {
        self.require_ui_unlock()?;
        dv::validate_secret_name(name).map_err(map_vault_err)?;
        dv::validate_secret_value(value).map_err(map_vault_err)?;
        let mut store = self.load_store(mem_dir)?;
        store
            .secrets
            .insert(name.to_string(), Value::String(value.to_string()));
        self.save_store(mem_dir, &store)
    }

    fn delete_secret(&self, mem_dir: &Path, name: &str) -> Result<bool, VaultError> {
        self.require_ui_unlock()?;
        let mut store = self.load_store(mem_dir)?;
        if store.secrets.remove(name).is_none() {
            return Ok(false);
        }
        self.save_store(mem_dir, &store)?;
        Ok(true)
    }

    // ── Envfiles ──────────────────────────────────────────────────

    fn list_envfiles(&self, mem_dir: &Path) -> Result<Vec<EnvFileEntry>, VaultError> {
        self.require_ui_unlock()?;
        let store = self.load_store(mem_dir)?;
        let mut keys: Vec<String> = store.envfiles.keys().cloned().collect();
        keys.sort();
        Ok(keys
            .into_iter()
            .map(|name| {
                let entry = &store.envfiles[&name];
                let content_len = entry
                    .get("content")
                    .and_then(Value::as_str)
                    .map(|s| s.len())
                    .unwrap_or(0);
                let updated_at = entry
                    .get("updated_at")
                    .and_then(Value::as_str)
                    .map(String::from);
                EnvFileEntry {
                    name,
                    updated_at,
                    size: content_len,
                }
            })
            .collect())
    }

    fn get_envfile(&self, mem_dir: &Path, name: &str) -> Result<Option<String>, VaultError> {
        self.require_ui_unlock()?;
        let store = self.load_store(mem_dir)?;
        Ok(store
            .envfiles
            .get(name)
            .and_then(|v| v.get("content").and_then(Value::as_str).map(String::from)))
    }

    fn set_envfile(&self, mem_dir: &Path, name: &str, content: &str) -> Result<(), VaultError> {
        self.require_ui_unlock()?;
        dv::validate_envfile_name(name).map_err(map_vault_err)?;
        dv::validate_envfile_content(content).map_err(map_vault_err)?;
        let mut store = self.load_store(mem_dir)?;
        let mut entry = Map::new();
        entry.insert("content".into(), Value::String(content.to_string()));
        entry.insert("updated_at".into(), Value::String(iso_now()));
        store
            .envfiles
            .insert(name.to_string(), Value::Object(entry));
        self.save_store(mem_dir, &store)
    }

    fn delete_envfile(&self, mem_dir: &Path, name: &str) -> Result<bool, VaultError> {
        self.require_ui_unlock()?;
        let mut store = self.load_store(mem_dir)?;
        if store.envfiles.remove(name).is_none() {
            return Ok(false);
        }
        self.save_store(mem_dir, &store)?;
        Ok(true)
    }

    // ── Agent access ──────────────────────────────────────────────

    fn get_secrets_for_agent(
        &self,
        mem_dir: &Path,
        names: &[String],
    ) -> Result<BTreeMap<String, String>, VaultError> {
        if names.is_empty() {
            return Ok(BTreeMap::new());
        }
        if !vault_path(mem_dir).exists() {
            return Ok(BTreeMap::new());
        }
        if !self.has_device_key(mem_dir) {
            return Ok(BTreeMap::new());
        }
        // `.cjs` silently degrades to empty on any error (e.g. corrupt
        // vault). Replicate so callers don't see surprising failures
        // during agent spawn.
        let store = match self.load_store(mem_dir) {
            Ok(s) => s,
            Err(_) => return Ok(BTreeMap::new()),
        };
        let mut out = BTreeMap::new();
        for name in names {
            if let Some(v) = store.secrets.get(name).and_then(Value::as_str) {
                out.insert(name.clone(), v.to_string());
            }
        }
        Ok(out)
    }

    // ── Bundles ───────────────────────────────────────────────────

    fn export_bundle(
        &self,
        mem_dir: &Path,
        master_password: &str,
        bundle_password: Option<&str>,
    ) -> Result<Vec<u8>, VaultError> {
        self.require_ui_unlock()?;
        if !vault_path(mem_dir).exists() {
            return Err(VaultError::NotInitialized);
        }
        let vault_bytes = read_all(&vault_path(mem_dir))?;
        let device_key = read_all(&device_key_path(mem_dir))?;
        let store = self.load_store(mem_dir)?;
        let auth = store.auth.as_ref().ok_or(VaultError::Malformed)?;
        if !dv::verify_password(master_password.as_bytes(), auth) {
            return Err(VaultError::WrongPassword);
        }
        let enc_pw = bundle_password.unwrap_or(master_password);
        let mut rng = rand::rng();
        let mut salt = vec![0u8; dv::SALT_LEN];
        rng.fill_bytes(&mut salt);
        let mut nonce = vec![0u8; dv::NONCE_LEN];
        rng.fill_bytes(&mut nonce);
        let exported_at = iso_now();
        let bundle = dv::build_export_bundle_deterministic(
            &vault_bytes,
            &device_key,
            enc_pw.as_bytes(),
            &exported_at,
            &salt,
            &nonce,
        )
        .map_err(map_vault_err)?;
        Ok(bundle)
    }

    fn import_bundle(
        &self,
        mem_dir: &Path,
        bundle: &[u8],
        bundle_password: &str,
        replace: bool,
        new_master_password: &str,
    ) -> Result<(), VaultError> {
        if new_master_password.len() < 8 {
            return Err(VaultError::Validation(
                "nova senha mestra precisa de no mínimo 8 caracteres".into(),
            ));
        }
        let already = vault_path(mem_dir).exists();
        if already && !replace {
            return Err(VaultError::AlreadyInitialized);
        }
        if already && !self.is_unlocked() {
            return Err(VaultError::Locked);
        }
        let unpacked =
            dv::unpack_export_bundle(bundle, bundle_password.as_bytes()).map_err(map_vault_err)?;
        fs::create_dir_all(mem_dir)?;
        ensure_gitignore(mem_dir)?;
        self.write_device_key(mem_dir, &unpacked.device_key)?;
        write_file_safe(&vault_path(mem_dir), &unpacked.vault_bytes, 0o600)?;
        // Rewrite auth block with the NEW master password.
        let mut store = self.load_store(mem_dir)?;
        let mut rng = rand::rng();
        store.auth = Some(
            dv::build_auth_block(new_master_password.as_bytes(), &mut rng)
                .map_err(map_vault_err)?,
        );
        self.save_store(mem_dir, &store)?;
        self.lock();
        Ok(())
    }

    fn destroy(&self, mem_dir: &Path) -> Result<(), VaultError> {
        self.lock();
        self.clear_device_key_cache();
        let vp = vault_path(mem_dir);
        if vp.exists() {
            fs::remove_file(&vp)?;
        }
        let dp = device_key_path(mem_dir);
        if dp.exists() {
            fs::remove_file(&dp)?;
        }
        let up = mem_dir.join(UNLOCK_STATE_FILE);
        if up.exists() {
            fs::remove_file(&up)?;
        }
        Ok(())
    }
}

fn iso_now() -> String {
    // Minimal ISO-8601 UTC stamp: "YYYY-MM-DDTHH:MM:SS.mmmZ".
    // Replicates `new Date().toISOString()` format used by the .cjs.
    let ms = now_ms();
    let secs = ms / 1000;
    let sub_ms = (ms % 1000) as u32;
    let (y, mo, d, h, mi, s) = epoch_ms_to_ymd_hms(secs);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y, mo, d, h, mi, s, sub_ms
    )
}

// Minimal civil-calendar conversion (Howard Hinnant's algorithm).
fn epoch_ms_to_ymd_hms(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let mut days = secs.div_euclid(86_400);
    let time_of_day = secs.rem_euclid(86_400) as u32;
    // Hinnant's "days_from_civil" inverse.
    days += 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = (days - era * 146_097) as u32; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i32 + (era as i32) * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;
    (y, m, d, hour, minute, second)
}
