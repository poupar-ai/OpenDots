//! Port de validação de licença.
//!
//! project-mem é comercial closed-source com 4 tiers (Free/Basic/Pro/
//! Business). A licença é um JWT Ed25519 assinado pelo Firebase
//! Cloud Functions, com:
//!
//! - **Verificação offline de assinatura** — public key embutida no
//!   binário; não precisa rede para saber que o token é autêntico.
//! - **Check online** — Firebase valida revogação, install count,
//!   expiração. Se offline, cai em grace de 7 dias.
//!
//! ## Install limits por tier
//!
//! | Tier     | Limite de máquinas |
//! |----------|--------------------|
//! | Free     | 1                  |
//! | Basic    | 1                  |
//! | Pro      | 3                  |
//! | Business | 10                 |
//!
//! O `machine_id` é derivado de identificadores estáveis (MAC + disk
//! serial) — lógica fica no adapter, não aqui.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Tier da licença. Deserializa como lowercase no payload Firebase
/// (`{"tier":"pro"}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Free,
    Basic,
    Pro,
    Business,
}

impl Tier {
    /// Número máximo de máquinas onde esta licença pode estar ativa
    /// simultaneamente. Validação final é do lado servidor (Firebase),
    /// este valor é só para UI prévia.
    pub fn install_limit(self) -> u32 {
        match self {
            Tier::Free => 1,
            Tier::Basic => 1,
            Tier::Pro => 3,
            Tier::Business => 10,
        }
    }
}

/// Resultado possível de um check de licença. Três estados distintos
/// em vez de `Result<Valid, E>` porque **Grace é estado estável de
/// produto** (usuário offline por dias continua funcionando) — não é
/// erro a ser tratado pelo caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LicenseStatus {
    /// Check online OK, licença ativa.
    Valid {
        tier: Tier,
        expires_at: DateTime<Utc>,
        machine_id: String,
    },
    /// Offline ou Firebase indisponível, mas último check foi
    /// recente o suficiente para continuar funcionando.
    Grace {
        tier: Tier,
        last_online_check: DateTime<Utc>,
        grace_ends_at: DateTime<Utc>,
    },
    /// Token inválido, revogado, expirado sem grace, ou machine_id
    /// diferente da primeira ativação.
    Invalid { reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum LicenseError {
    #[error("network: {0}")]
    Network(String),
    #[error("signature verification failed")]
    BadSignature,
    #[error("malformed token: {0}")]
    Malformed(String),
    #[error("machine mismatch")]
    MachineMismatch,
    #[error("revoked")]
    Revoked,
}

/// Validador de licença. `async_trait` porque `check` faz HTTPS.
/// `verify_signature` é síncrono — crypto pura sobre bytes em
/// memória.
#[async_trait]
pub trait LicenseValidator: Send + Sync {
    /// Verifica assinatura Ed25519 usando a public key embutida no
    /// binário. Rápido e offline — é a primeira linha de defesa
    /// contra tokens forjados antes de gastar round-trip de rede.
    fn verify_signature(&self, token: &str) -> Result<(), LicenseError>;

    /// Check online completo contra Firebase. Valida revogação,
    /// install count e expiração. Se offline, deve cair em grace
    /// (nunca retornar `Invalid` por falha de rede).
    async fn check(&self, token: &str, machine_id: &str) -> Result<LicenseStatus, LicenseError>;
}
