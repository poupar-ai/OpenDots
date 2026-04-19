//! # core-ports
//!
//! Contratos (traits) da arquitetura hexagonal do project-mem v2.
//!
//! ## Regra do crate
//!
//! **Ninguém neste crate faz I/O.** Aqui só moram traits, tipos de
//! dados e variantes de erro. Toda implementação concreta vive em:
//!
//! - `adapter-*` — lado direito da hexágono (disco, rede, crypto,
//!   processos filhos). Ex.: `adapter-vault-file` implementa
//!   [`VaultStore`] lendo/gravando `vault.enc`.
//! - `domain-*` — lado esquerdo (regras de negócio que consomem os
//!   ports). Ex.: `domain-observations` recebe um `Arc<dyn
//!   ObservationLog>` e aplica filtros/agregações.
//!
//! Essa separação é o que permite testar o domínio com fakes triviais
//! (ver os `mod tests` abaixo de cada módulo) e trocar adapters sem
//! mexer em regra de negócio.
//!
//! ## Catálogo de ports
//!
//! | Trait                | Responsabilidade                                           |
//! |----------------------|------------------------------------------------------------|
//! | [`Clock`]            | Tempo. Permite `FakeClock` nos testes.                     |
//! | [`ObservationLog`]   | Append-only JSONL de observações (`observations.jsonl`).   |
//! | [`SessionStore`]     | Flag de sessão inicializada (`tmp/.init-<session_id>`).    |
//! | [`VaultStore`]       | Cofre `vault.enc` — PMV1 legado, PMV2 atual, bundles PMVEXP1. |
//! | [`GraphStore`]       | Grafo de projeto (`graph.json`): nodes + edges + stats.   |
//! | [`LicenseValidator`] | Validação Ed25519 de licença Firebase (Free/Basic/Pro/Business). |
//! | [`UpdateChannel`]    | Canal de update (checagem de versão + download de artefato). |
//! | [`TelemetrySink`]    | Envio de eventos de telemetria (opt-in).                  |
//! | [`Notifier`]         | Bus interno (SSE para UI, mensagens agente↔agente).        |
//!
//! ## Quando adicionar um novo port
//!
//! Só quando existir mais de uma implementação plausível **ou** quando
//! for preciso mockar o lado externo em testes. Port gratuito é
//! overhead: não criamos um `FileReader` trait só porque a função usa
//! `std::fs::read`.
//!
//! ## Regra de paridade
//!
//! Os nomes de campos em tipos serializados ([`VaultStatus`],
//! [`EnvFileEntry`], [`UpdateManifest`], etc.) precisam casar 1:1 com o
//! JSON emitido pelo `.cjs` equivalente. Testes de paridade
//! cross-language (ver `core-ports/tests/parity_*.rs`) spawn `node` e
//! comparam a shape. Renomear campo é quebrar contrato.

pub mod clock;
pub mod graph;
pub mod license;
pub mod notifier;
pub mod observation;
pub mod session;
pub mod telemetry;
pub mod update;
pub mod vault;

pub use clock::{Clock, SystemClock};
pub use graph::{GraphError, GraphStore};
pub use license::{LicenseError, LicenseStatus, LicenseValidator, Tier};
pub use notifier::{Notifier, NotifierError};
pub use observation::{ObservationError, ObservationLog};
pub use session::{SessionError, SessionStore};
pub use telemetry::{TelemetryError, TelemetryEvent, TelemetrySink};
pub use update::{UpdateChannel, UpdateError, UpdateManifest};
pub use vault::{EnvFileEntry, VaultError, VaultStatus, VaultStore};
