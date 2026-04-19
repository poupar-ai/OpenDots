//! Port do cofre (`vault.enc`) — secrets e env files criptografados.
//!
//! ## Formatos binários suportados
//!
//! | Magic      | Status    | Descrição                                              |
//! |------------|-----------|--------------------------------------------------------|
//! | `PMV1`     | legado    | AES-GCM com chave derivada de senha via PBKDF2.       |
//! | `PMV2`     | **atual** | AES-GCM + device-key 0600 + auth block com scrypt.    |
//! | `PMVEXP1`  | export    | Bundle de import/export entre máquinas.                |
//!
//! O trait expõe apenas operações de alto nível — **cripto, layout
//! binário, device-key e migração PMV1→PMV2 vivem no adapter**
//! (`adapter-vault-file`). Isso mantém regras de UI/autorização e
//! testes de domínio livres de detalhes de AES.
//!
//! ## Superfície grande = preocupação consciente
//!
//! Este port tem ~20 métodos. Normalmente quebrar em traits menores é
//! melhor — aqui não, porque todos operam sobre o **mesmo arquivo**
//! (`vault.enc`) sob o **mesmo lock de acesso**. Quebrar forçaria
//! coordenação entre traits, que é pior que um trait "gordo".

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("vault not initialized")]
    NotInitialized,
    #[error("already initialized")]
    AlreadyInitialized,
    #[error("locked — senha necessária")]
    Locked,
    #[error("wrong password")]
    WrongPassword,
    #[error("malformed vault file")]
    Malformed,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("validation: {0}")]
    Validation(String),
}

/// Shape idêntico ao retorno de `vault.status()` no `.cjs`:
/// `{ initialized, locked, expires_at, ttl_ms, needs_migration }`.
///
/// - `expires_at` / `ttl_ms` são `Option<i64>` em ms desde epoch
///   (`Date.now()` do Node).
/// - `needs_migration` é `true` quando o arquivo é PMV1 e precisa ser
///   reescrito em PMV2 no próximo unlock.
///
/// **Regra de paridade:** nomes de campos = chaves de JSON. Mudar aqui
/// quebra o teste `parity_vault_status.rs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultStatus {
    pub initialized: bool,
    pub locked: bool,
    pub expires_at: Option<i64>,
    pub ttl_ms: Option<i64>,
    pub needs_migration: bool,
}

/// Shape idêntico ao retorno de `listEnvFiles()` no `.cjs`:
/// `{ name, updated_at, size }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvFileEntry {
    pub name: String,
    pub updated_at: Option<String>,
    pub size: usize,
}

/// Contrato completo do cofre. Agrupado em blocos lógicos:
///
/// - **Lifecycle** — status/init/unlock/lock/change_password.
/// - **Secrets** (UI-gated) — operações que exigem interação humana.
/// - **Envfiles** (UI-gated) — arquivos `.env` inteiros, não só pares.
/// - **Agent access** — leitura sem UI via device-key.
/// - **Bundles** — export/import entre máquinas (PMVEXP1).
/// - **Destroy** — apaga o vault.enc e metadados.
pub trait VaultStore: Send + Sync {
    // ── Lifecycle ──────────────────────────────────────────────────────────
    fn status(&self, mem_dir: &Path) -> Result<VaultStatus, VaultError>;
    fn init(&self, mem_dir: &Path, password: &str) -> Result<VaultStatus, VaultError>;
    fn unlock(
        &self,
        mem_dir: &Path,
        password: &str,
        ttl_ms: i64,
    ) -> Result<VaultStatus, VaultError>;
    fn lock(&self);
    fn change_password(
        &self,
        mem_dir: &Path,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), VaultError>;

    // ── Secrets (UI-gated) ────────────────────────────────────────────────
    fn list_keys(&self, mem_dir: &Path) -> Result<Vec<String>, VaultError>;
    fn get_secret(&self, mem_dir: &Path, name: &str) -> Result<Option<String>, VaultError>;
    fn set_secret(&self, mem_dir: &Path, name: &str, value: &str) -> Result<(), VaultError>;
    fn delete_secret(&self, mem_dir: &Path, name: &str) -> Result<bool, VaultError>;

    // ── Envfiles (UI-gated) ───────────────────────────────────────────────
    fn list_envfiles(&self, mem_dir: &Path) -> Result<Vec<EnvFileEntry>, VaultError>;
    fn get_envfile(&self, mem_dir: &Path, name: &str) -> Result<Option<String>, VaultError>;
    fn set_envfile(&self, mem_dir: &Path, name: &str, content: &str) -> Result<(), VaultError>;
    fn delete_envfile(&self, mem_dir: &Path, name: &str) -> Result<bool, VaultError>;

    // ── Agent access (device-key, sem UI) ─────────────────────────────────
    /// Lê secrets em lote para um agente já autorizado. Não envolve
    /// prompt de senha — usa a device-key 0600.
    fn get_secrets_for_agent(
        &self,
        mem_dir: &Path,
        names: &[String],
    ) -> Result<std::collections::BTreeMap<String, String>, VaultError>;

    // ── Export / import bundles PMVEXP1 ───────────────────────────────────
    fn export_bundle(
        &self,
        mem_dir: &Path,
        master_password: &str,
        bundle_password: Option<&str>,
    ) -> Result<Vec<u8>, VaultError>;
    fn import_bundle(
        &self,
        mem_dir: &Path,
        bundle: &[u8],
        bundle_password: &str,
        replace: bool,
        new_master_password: &str,
    ) -> Result<(), VaultError>;

    fn destroy(&self, mem_dir: &Path) -> Result<(), VaultError>;
}
