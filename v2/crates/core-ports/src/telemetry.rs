//! Port de telemetria opt-in.
//!
//! Eventos têm shape livre (`serde_json::Value` em `properties`) para
//! evitar coupling com o produto de destino — o adapter concreto pode
//! ser PostHog, Segment, endpoint custom, etc.
//!
//! ## Opt-in first
//!
//! open-poupar guarda dados sensíveis do usuário (secrets, decisões
//! arquiteturais). Telemetria **nunca** é ligada por default. O
//! adapter padrão é no-op até o usuário ativar explicitamente; a
//! variante de erro `Disabled` existe para o domínio saber quando a
//! ligação está desativada sem propagar `Option` em todo lugar.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Um evento de telemetria. `properties` é `Value` para não forçar
/// shapes por evento — cada callsite compõe o JSON que quiser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryEvent {
    /// Nome do evento (`"vault.unlock"`, `"agent.message.sent"`, etc.).
    pub name: String,
    /// Propriedades arbitrárias. **Não** colocar PII — review de código
    /// obriga justificativa para qualquer campo novo.
    pub properties: Value,
    /// Timestamp em ms desde epoch.
    pub ts_unix_ms: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("network: {0}")]
    Network(String),
    /// Usuário não deu opt-in. Não é falha — caller pode ignorar.
    #[error("disabled by user")]
    Disabled,
}

#[async_trait]
pub trait TelemetrySink: Send + Sync {
    /// Envia um evento. Implementações opt-out devem retornar
    /// `TelemetryError::Disabled` sem tocar rede.
    async fn send(&self, event: &TelemetryEvent) -> Result<(), TelemetryError>;
}
