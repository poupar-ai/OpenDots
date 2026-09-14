//! Port de canal de updates.
//!
//! O binário open-poupar checa updates contra um manifest assinado
//! (mesma master key Ed25519 usada nas licenças). O manifest lista
//! artefatos por target (linux-x64, darwin-arm64, etc.) com SHA-256
//! para verificação pós-download.
//!
//! Separamos em dois métodos:
//! - `latest` — fetch HTTPS do manifest mais recente do canal pedido
//!   (stable / beta / nightly).
//! - `verify` — valida a assinatura localmente; barato, síncrono.
//!
//! Fluxo típico: `latest` → `verify` → baixar artifact → conferir SHA
//! → aplicar.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Manifest de uma release publicada no servidor de updates.
///
/// - `channel`: "stable" / "beta" / "nightly".
/// - `released_at`: ISO 8601. String, não `DateTime`, para preservar
///   exatamente o formato que o servidor emite (o manifest é assinado
///   como texto — reformatar quebraria a assinatura).
/// - `signature`: Ed25519 base64 sobre os outros campos serializados.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    pub channel: String,
    pub released_at: String,
    pub artifacts: Vec<Artifact>,
    pub signature: String,
}

/// Um binário publicado para um target específico.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// Target triple (`x86_64-unknown-linux-gnu` etc.).
    pub target: String,
    /// URL HTTPS para download direto.
    pub url: String,
    /// SHA-256 do arquivo, hex lowercase.
    pub sha256: String,
    /// Bytes do arquivo — valida o download antes de descompactar.
    pub size: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("network: {0}")]
    Network(String),
    #[error("bad signature")]
    BadSignature,
    #[error("malformed manifest: {0}")]
    Malformed(String),
}

#[async_trait]
pub trait UpdateChannel: Send + Sync {
    /// Busca o manifest mais recente do canal. Espera-se que retorne
    /// `UpdateError::Network` em qualquer falha de rede — caller trata
    /// offline como "sem update disponível", não como erro fatal.
    async fn latest(&self, channel: &str) -> Result<UpdateManifest, UpdateError>;

    /// Valida a assinatura Ed25519 do manifest. Síncrono porque é
    /// crypto em memória. Sempre chamar antes de confiar em qualquer
    /// URL ou SHA do manifest.
    fn verify(&self, manifest: &UpdateManifest) -> Result<(), UpdateError>;
}
