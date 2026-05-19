//! Loop de scheduling do cron — usa a `AgentQueue` existente como camada
//! de execução. O scheduler é só um "feeder" que olha pra `cron_jobs.jsonl`
//! a cada tick, identifica jobs vencidos e chama `auto_queue.enqueue(...)`
//! com o `owner` como `from` e o `target` como `to`. A queue cuida de
//! persistência, retry, rate-limit, e do JWT real via `agent_runner`.
//!
//! Catchup policy: jobs cuja janela passou enquanto o servidor estava
//! offline NÃO disparam retroativamente. Inicializamos `last_run` em
//! memória com `now` no startup — semantics "next fire é o próximo após
//! agora".

use crate::AppState;
use domain_cron::{model::JobAction, store};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

const TICK_INTERVAL: Duration = Duration::from_secs(5);

/// Spawn da background task. Idempotente — chame uma vez no startup.
/// `Arc<Mutex<BTreeMap<id, last_run_ms>>>` mantém histórico em memória;
/// volátil por design (recarga aplica skip-catchup).
pub fn spawn(state: AppState) {
    let last_runs: Arc<Mutex<BTreeMap<String, u64>>> = Arc::new(Mutex::new(BTreeMap::new()));
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(TICK_INTERVAL);
        // Tick imediato no boot — mas como last_runs começa vazio,
        // `due_jobs` vai assumir last_run = now pra cada job (skip catchup).
        loop {
            interval.tick().await;
            tick(&state, &last_runs).await;
        }
    });
}

async fn tick(state: &AppState, last_runs: &Arc<Mutex<BTreeMap<String, u64>>>) {
    let now = unix_now_ms();
    let due = {
        let guard = last_runs.lock().await;
        match store::due_jobs(&state.mem_dir, now, &guard) {
            Ok(v) => v,
            Err(_) => return,
        }
    };
    if due.is_empty() {
        return;
    }
    let mut guard = last_runs.lock().await;
    for job in due {
        let (target, message) = match &job.action {
            JobAction::Chat { target, message } => (target.clone(), message.clone()),
        };
        // Tenta enfileirar. enqueue valida rate-limit interno e devolve
        // bool — se falhar não é fatal, próximo tick tenta de novo.
        let _ = state.auto_queue.enqueue(&target, &job.owner, &message, None);
        guard.insert(job.id.clone(), now);
        // One-shot: auto-desativa após o fire pra não tentar rodar de novo
        // (e pra sumir da lista de jobs ativos na UI).
        if job.is_one_shot() {
            let _ = store::deactivate_job(&state.mem_dir, &job.id);
        }
    }
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
