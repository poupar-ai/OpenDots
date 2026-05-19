//! Modelo de dados pro cron job.

use serde::{Deserialize, Serialize};

/// Ação a ser executada quando o cron dispara. Inicialmente só suporta
/// `chat` (manda mensagem do `owner` pro `target` via fila). Forma de
/// enum aberta pra suportar `pipeline`, `subagent`, `webhook` no futuro
/// sem quebrar formato.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobAction {
    /// Manda `message` do `owner` pro `target` via `AgentQueue::enqueue`.
    Chat { target: String, message: String },
}

/// Entrada persistida no `cron_jobs.jsonl`. Append-only — para "deletar"
/// um job, append-amos uma versão com `active=false` e o load mantém
/// só a última versão por id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    /// ID estável (cuid-like ou uuid).
    pub id: String,
    /// Agente que possui o job. Mint do JWT usa essa identidade quando
    /// o scheduler dispara — no chat, vira `from`.
    pub owner: String,
    /// Cron expression (formato 6-campo ou atalho).
    pub schedule: String,
    /// O que executar.
    pub action: JobAction,
    /// Timestamp da criação (ms desde epoch).
    pub created_at_ms: u64,
    /// False = soft-deleted (preservado pra audit, mas ignorado em
    /// `list_active` e no scheduler).
    #[serde(default = "yes")]
    pub active: bool,
    /// Última vez que o scheduler disparou esse job (ms desde epoch).
    /// `None` = nunca disparou. Não persistido em todo update — fica
    /// em memória; após restart é zerado e o scheduler aplica
    /// "skip catchup" (sem refire de jobs perdidos durante downtime).
    #[serde(skip)]
    pub last_run_ms: Option<u64>,
    /// Próxima execução prevista, computada na hora a partir de
    /// `schedule + max(last_run, created_at)`. Não persistido.
    #[serde(skip)]
    pub next_run_ms: Option<u64>,
}

fn yes() -> bool {
    true
}

impl CronJob {
    pub fn target(&self) -> Option<&str> {
        match &self.action {
            JobAction::Chat { target, .. } => Some(target),
        }
    }
    pub fn message(&self) -> Option<&str> {
        match &self.action {
            JobAction::Chat { message, .. } => Some(message),
        }
    }
}
