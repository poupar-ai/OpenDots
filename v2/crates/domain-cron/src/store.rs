//! Persistência dos cron jobs em `<mem_dir>/cron_jobs.jsonl`.
//!
//! Modelo append-only com semântica de **última versão por id ganha**:
//! - `create_job` faz append da definição
//! - `deactivate_job` faz append da MESMA definição com `active=false`
//! - `load_state` lê tudo e deduplica por id, mantendo a última ocorrência
//!
//! Não suportamos "update" em geral (mudar schedule/message de um job
//! existente requer apagar + criar — mantém o modelo simples).

use crate::model::CronJob;
use crate::parser;
use core_storage::JsonlRepository;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("storage: {0}")]
    Storage(#[from] core_storage::StorageError),
    #[error("schedule inválido: {0}")]
    BadSchedule(#[from] parser::CronError),
    #[error("job não encontrado: {0}")]
    NotFound(String),
}

fn jsonl_path(mem_dir: &Path) -> std::path::PathBuf {
    mem_dir.join("cron_jobs.jsonl")
}

fn open_repo(mem_dir: &Path) -> Result<JsonlRepository<CronJob>, StoreError> {
    Ok(JsonlRepository::<CronJob>::open_default(jsonl_path(mem_dir))?)
}

/// Carrega o estado atual dos jobs (dedup por id, última versão ganha).
/// Computa `next_run_ms` pra cada job ativo baseado em
/// `created_at_ms` (jobs já carregados sem histórico — scheduler aplica
/// "skip catchup" se o next_run estiver no passado).
pub fn load_state(mem_dir: &Path) -> Result<Vec<CronJob>, StoreError> {
    let repo = open_repo(mem_dir)?;
    let mut latest: BTreeMap<String, CronJob> = BTreeMap::new();
    for job in repo.read_all() {
        latest.insert(job.id.clone(), job);
    }
    let mut out: Vec<CronJob> = latest.into_values().collect();
    out.sort_by(|a, b| a.created_at_ms.cmp(&b.created_at_ms));
    Ok(out)
}

/// Lista apenas jobs ativos, com `next_run_ms` calculado a partir de
/// `now_ms` (= próxima execução >= now).
pub fn list_active(mem_dir: &Path, now_ms: u64) -> Result<Vec<CronJob>, StoreError> {
    let mut jobs = load_state(mem_dir)?;
    jobs.retain(|j| j.active);
    for j in &mut jobs {
        if let Ok(cron) = parser::parse(&j.schedule) {
            j.next_run_ms = parser::next_run(&cron, now_ms.saturating_sub(1));
        }
    }
    Ok(jobs)
}

/// Lista TODOS os jobs (ativos e inativos). Útil pra audit/UI mostrar
/// soft-deletes.
pub fn list_all(mem_dir: &Path) -> Result<Vec<CronJob>, StoreError> {
    load_state(mem_dir)
}

/// Cria um novo cron job. Valida schedule antes de persistir.
pub fn create_job(mem_dir: &Path, mut job: CronJob) -> Result<CronJob, StoreError> {
    // Valida schedule
    parser::parse(&job.schedule)?;
    job.active = true;
    job.last_run_ms = None;
    job.next_run_ms = None;
    let repo = open_repo(mem_dir)?;
    repo.append(&job)?;
    Ok(job)
}

/// Soft-delete: append do mesmo job com `active=false`. Idempotente —
/// re-chamar é no-op se já estiver inativo.
pub fn deactivate_job(mem_dir: &Path, id: &str) -> Result<(), StoreError> {
    let mut jobs = load_state(mem_dir)?;
    let job = jobs
        .iter_mut()
        .find(|j| j.id == id)
        .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
    if !job.active {
        return Ok(());
    }
    job.active = false;
    let repo = open_repo(mem_dir)?;
    repo.append(job)?;
    Ok(())
}

/// Lista jobs ativos cujo `next_run_ms <= now_ms` (= devidos pra disparo).
///
/// `last_runs` é o mapa `id → last_run_ms` mantido em memória pelo
/// scheduler (sobrevive scheduler-loop mas não survive restart). Quando
/// um job acabou de subir e ainda não tem entrada aqui, assumimos
/// `last_run = now` (skip catchup) pra evitar fire imediato em restart.
pub fn due_jobs(
    mem_dir: &Path,
    now_ms: u64,
    last_runs: &BTreeMap<String, u64>,
) -> Result<Vec<CronJob>, StoreError> {
    let mut jobs = load_state(mem_dir)?;
    jobs.retain(|j| j.active);
    let mut out = Vec::new();
    for mut j in jobs {
        let baseline = last_runs.get(&j.id).copied().unwrap_or(now_ms);
        let cron = match parser::parse(&j.schedule) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let Some(next) = parser::next_run(&cron, baseline) else { continue };
        if next <= now_ms {
            j.last_run_ms = Some(now_ms);
            j.next_run_ms = parser::next_run(&cron, now_ms);
            out.push(j);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::JobAction;
    use tempfile::tempdir;

    fn mkjob(id: &str, owner: &str, schedule: &str) -> CronJob {
        CronJob {
            id: id.into(),
            owner: owner.into(),
            schedule: schedule.into(),
            action: JobAction::Chat {
                target: "oracle".into(),
                message: "tick".into(),
            },
            created_at_ms: 1_700_000_000_000,
            active: true,
            last_run_ms: None,
            next_run_ms: None,
        }
    }

    #[test]
    fn create_and_list() {
        let tmp = tempdir().unwrap();
        create_job(tmp.path(), mkjob("j1", "manager", "@hourly")).unwrap();
        create_job(tmp.path(), mkjob("j2", "manager", "@daily")).unwrap();
        let listed = list_active(tmp.path(), 1_700_000_001_000).unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().all(|j| j.next_run_ms.is_some()));
    }

    #[test]
    fn deactivate_hides_from_list_active() {
        let tmp = tempdir().unwrap();
        create_job(tmp.path(), mkjob("j1", "manager", "@hourly")).unwrap();
        deactivate_job(tmp.path(), "j1").unwrap();
        assert!(list_active(tmp.path(), 1_700_000_001_000).unwrap().is_empty());
        assert_eq!(list_all(tmp.path()).unwrap().len(), 1);
    }

    #[test]
    fn create_rejects_invalid_schedule() {
        let tmp = tempdir().unwrap();
        let err = create_job(tmp.path(), mkjob("j1", "manager", "lixo")).unwrap_err();
        matches!(err, StoreError::BadSchedule(_));
    }

    #[test]
    fn due_jobs_returns_overdue_only() {
        let tmp = tempdir().unwrap();
        // schedule a cada 5 segundos
        create_job(tmp.path(), mkjob("j1", "manager", "*/5 * * * * *")).unwrap();
        // last_runs vazio + now no segundo 10 → próximo é :15. Não devido ainda.
        let now = 1_700_000_010_000;
        let mut last_runs = BTreeMap::new();
        last_runs.insert("j1".to_string(), now);
        let due = due_jobs(tmp.path(), now, &last_runs).unwrap();
        assert!(due.is_empty(), "shouldn't fire when last_run = now");
        // Avança 6s — agora deve disparar (próximo a partir de 1_700_000_010 = :15
        // e estamos em :16)
        let later = 1_700_000_016_000;
        let due = due_jobs(tmp.path(), later, &last_runs).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, "j1");
    }
}
