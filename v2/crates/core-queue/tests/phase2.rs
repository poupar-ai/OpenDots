//! Tests da Fase 2 (persistência + reconcile + priority + dead-letter).
//!
//! Estratégia: cria queue com `state_dir` (tmp), exercita o fluxo,
//! valida invariantes via API pública + leitura direta dos arquivos.
//!
//! Pra simular zumbi (turn rodando há > TTL), uso TTL super curto
//! (50ms) e `tokio::time::sleep(100ms)`. Não-ideal (flaky em CI
//! lento), mas alternativa seria injetar relógio fake e isso requer
//! refactor maior.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use core_queue::{AgentQueue, AgentQueueConfig, BoxFut, Priority, Runner};
use tempfile::TempDir;
use tokio::sync::mpsc;

/// Runner no-op (resolve future imediato sem fazer nada).
fn noop_runner() -> Runner {
    Arc::new(|_to, _from, _msg, _traces| -> BoxFut {
        Box::pin(async move {})
    })
}

/// Runner que SEGURA — fica em await infinito. Usado pra simular um
/// turn "in-flight" que não termina (simula crash do agente).
fn hanging_runner() -> Runner {
    Arc::new(|_to, _from, _msg, _traces| -> BoxFut {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_secs(3600)).await;
        })
    })
}

/// Runner que grava cada invocação — usado pra inspecionar ordem.
fn recording_runner(tx: mpsc::UnboundedSender<(String, String)>) -> Runner {
    Arc::new(move |to, _from, combined, _traces| -> BoxFut {
        let tx = tx.clone();
        Box::pin(async move {
            let _ = tx.send((to, combined));
        })
    })
}

fn cfg_with_state_dir(runner: Runner, dir: PathBuf) -> AgentQueueConfig {
    let mut c = AgentQueueConfig::new(runner);
    c.state_dir = Some(dir);
    c
}

// ─── reconcile: TTL pending ─────────────────────────────────────────────────

#[tokio::test]
async fn reconcile_expires_old_pending_to_dead_letter() {
    let tmp = TempDir::new().unwrap();
    let mut cfg = cfg_with_state_dir(noop_runner(), tmp.path().to_path_buf());
    cfg.message_max_age = Duration::from_millis(50);
    // Runner é no-op, mas o drain disparado pelo enqueue vai consumir.
    // Pra garantir que a msg fique em pending até reconcile, usamos
    // rate_limit pra travar drain... Mais simples: hanging runner pra
    // primeira msg "ocupar" o slot, e a segunda fica em pending.
    let q = AgentQueue::new(cfg);

    // Insere msg, sleep pra ela ser "velha".
    q.enqueue("agent_x", "from_x", "msg velha", None);
    // O drain spawn vai rodar IMEDIATAMENTE (noop_runner) — então
    // a msg sai do pending. Pra esse teste funcionar, preciso de
    // hanging runner. Recrio.
    drop(q);

    let mut cfg2 = cfg_with_state_dir(hanging_runner(), tmp.path().to_path_buf());
    cfg2.message_max_age = Duration::from_millis(50);
    let q = AgentQueue::new(cfg2);
    q.enqueue("agent_x", "from_x", "msg A (vai pro runner)", None);
    // Espera A sair do pending pro hanging runner (slot ocupado).
    // Sem isso, take_batch (greedy) leva A E B juntos no mesmo batch
    // e nada fica em pending pra expirar.
    tokio::time::sleep(Duration::from_millis(20)).await;
    q.enqueue("agent_x", "from_x", "msg B (fica em pending)", None);
    // Agora espera B virar "velha" em pending.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let res = q.reconcile();
    assert!(res.dead >= 1, "esperava ≥1 expirada, veio {}", res.dead);
    // Dead-letter file deve conter a msg expirada.
    let dead = std::fs::read_to_string(tmp.path().join("dead.jsonl")).unwrap();
    assert!(dead.contains("\"reason\":\"expired\""), "dead.jsonl: {dead}");
    assert!(dead.contains("msg B"), "esperava msg B no dead-letter: {dead}");
}

// ─── reconcile: zumbi recicla com retries+1 ─────────────────────────────────

#[tokio::test]
async fn reconcile_recycles_zombie_with_retries_increment() {
    let tmp = TempDir::new().unwrap();
    let mut cfg = cfg_with_state_dir(noop_runner(), tmp.path().to_path_buf());
    cfg.turn_ttl = Duration::from_millis(50);
    cfg.max_retries = 5; // suficiente pra não cair em dead-letter
    let q = AgentQueue::new(cfg);

    // acquire simula "B começou a processar" — vira running com
    // started_at_ms = now. Esperamos > TTL → vira zumbi.
    q.acquire("agent_z");
    assert!(q.is_running("agent_z"));
    tokio::time::sleep(Duration::from_millis(100)).await;

    let res = q.reconcile();
    assert_eq!(res.recycled, 1, "esperava 1 reciclado, veio {}", res.recycled);
    assert_eq!(res.dead, 0);
    assert!(!q.is_running("agent_z"), "running deveria ter sido limpo");
    // Entry voltou pra pending com retries=1 e priority=High (reciclados
    // ganham High pra processar antes dos outros).
    assert_eq!(q.pending_size("agent_z"), 1);
}

// ─── reconcile: dead-letter quando atinge max_retries ───────────────────────

#[tokio::test]
async fn reconcile_zombie_at_max_retries_goes_to_dead_letter() {
    let tmp = TempDir::new().unwrap();
    let mut cfg = cfg_with_state_dir(noop_runner(), tmp.path().to_path_buf());
    cfg.turn_ttl = Duration::from_millis(50);
    cfg.max_retries = 1; // qualquer retry já é >= max → dead
    let q = AgentQueue::new(cfg);

    q.acquire("agent_d");
    tokio::time::sleep(Duration::from_millis(100)).await;

    let res = q.reconcile();
    assert_eq!(res.dead, 1, "esperava 1 morto, veio {}", res.dead);
    assert_eq!(res.recycled, 0);
    assert_eq!(q.pending_size("agent_d"), 0);

    let dead = std::fs::read_to_string(tmp.path().join("dead.jsonl")).unwrap();
    assert!(
        dead.contains("\"reason\":\"max_retries_zombie\""),
        "dead.jsonl: {dead}"
    );
}

// ─── priority: High enfileirado depois de Normal vai pra frente ─────────────

#[tokio::test]
async fn enqueue_with_priority_orders_high_before_normal() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut cfg = AgentQueueConfig::new(recording_runner(tx));
    // Sem state_dir pra ficar rápido; testando só ordem.
    cfg.rate_max = 100;
    let q = AgentQueue::new(cfg);

    // Trava drain: marca como running, depois enfileira na ordem
    // [Normal, Low, High] (chegando), e libera.
    q.acquire("agent_p");
    q.enqueue_with("agent_p", "from", "msg-normal", None, Priority::Normal);
    q.enqueue_with("agent_p", "from", "msg-low", None, Priority::Low);
    q.enqueue_with("agent_p", "from", "msg-high", None, Priority::High);
    q.release("agent_p");

    // Espera drain rodar — runner combina TODAS as msgs do batch em
    // ordem da pending.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (_to, combined) = rx.try_recv().expect("runner não foi chamado");

    let idx_high = combined.find("msg-high").expect("missing high");
    let idx_normal = combined.find("msg-normal").expect("missing normal");
    let idx_low = combined.find("msg-low").expect("missing low");
    assert!(idx_high < idx_normal, "high deveria vir antes de normal:\n{combined}");
    assert!(idx_normal < idx_low, "normal deveria vir antes de low:\n{combined}");
}

// ─── persistência: state.json sobrevive recriação da queue ──────────────────

#[tokio::test]
async fn persist_then_restore_pending_roundtrips() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();

    {
        let cfg = cfg_with_state_dir(hanging_runner(), dir.clone());
        let q = AgentQueue::new(cfg);
        // 1 ocupa o slot (hanging) — espera antes de enqueue da 2ª
        // pra não cair no mesmo batch greedy do take_batch.
        q.enqueue("agent_r", "fA", "msg-1-running", None);
        tokio::time::sleep(Duration::from_millis(20)).await;
        q.enqueue_with("agent_r", "fB", "msg-2-pending", Some("trace-xyz"), Priority::High);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(q.is_running("agent_r"));
        assert!(q.pending_size("agent_r") >= 1);
        // Drop da queue não apaga arquivo — só o processo "morre".
    }

    let raw = std::fs::read_to_string(dir.join("state.json")).unwrap();
    // Sanity: state.json reflete o que enfileiramos.
    assert!(raw.contains("agent_r"));
    assert!(raw.contains("trace-xyz"), "trace_id deveria ter sido persistido: {raw}");

    // Recria com mesmo state_dir — restore deve trazer pending de volta.
    let cfg2 = cfg_with_state_dir(noop_runner(), dir);
    let q2 = AgentQueue::new(cfg2);
    // running do crash anterior ainda é refletido (será zombie em
    // reconcile próximo, mas o teste só verifica que persistiu).
    assert!(q2.is_running("agent_r"), "running deveria ter sido restaurado");
    assert!(q2.pending_size("agent_r") >= 1, "pending deveria ter sido restaurado");
}
