//! `domain-cron` — agendamento de jobs recorrentes (cron) sobre a fila de
//! agentes existente.
//!
//! Arquitetura hexagonal: módulo de domínio puro, sem dependência de HTTP,
//! tokio, ou qualquer adapter. Expõe:
//!
//! - [`parser`] — parse de cron expression 6-campo (seg/min/hora/dom/mon/dow)
//!   + atalhos (`@hourly`, `@daily`, etc.) + `next_run()`.
//! - [`model::CronJob`] — definição persistida (id, owner, target, message,
//!   schedule, active).
//! - [`store`] — carga e persistência idempotente em
//!   `<mem_dir>/cron_jobs.jsonl` via `core_storage::JsonlRepository`.
//! - Funções de alto nível: [`create_job`], [`list_active`], [`due_jobs`],
//!   [`deactivate_job`].
//!
//! A camada HTTP (`api-http::cron_routes`) consome esse domínio e a camada
//! de execução (`api-http::cron_scheduler`) chama [`due_jobs`] no tick e
//! delega a entrega pro `AgentQueue` já existente.

pub mod parser;
pub mod model;
pub mod store;

pub use model::{CronJob, JobAction};
pub use parser::{parse, next_run, Cron, CronError};
pub use store::{
    create_job, deactivate_job, list_active, list_all, load_state, due_jobs, StoreError,
};
