//! Unit tests do motor de fila.
//!
//! Estratégia: runner que grava cada invocação num `mpsc::UnboundedSender`.
//! Isso permite inspecionar a ordem, batching e conteúdo sem depender do
//! tempo do tokio. Com runtime single-threaded (`#[tokio::test]` default),
//! os `enqueue` sync executam sem ceder — então múltiplos enqueues em
//! rajada virão batched quando o drain spawnado finalmente rodar.

use std::sync::Arc;
use std::time::Duration;

use core_queue::{
    AgentQueue, AgentQueueConfig, BoxFut, DrainEndEvent, DrainStartEvent, EnqueueEvent,
    ManualClock, Runner, ThrottleEvent,
};
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunnerCall {
    to_agent: String,
    from_label: String,
    combined: String,
    trace_ids: Vec<String>,
}

/// Runner que grava cada invocação. Return future resolve imediato.
fn recording_runner(tx: mpsc::UnboundedSender<RunnerCall>) -> Runner {
    Arc::new(move |to, from, combined, trace_ids| -> BoxFut {
        let tx = tx.clone();
        Box::pin(async move {
            tx.send(RunnerCall {
                to_agent: to,
                from_label: from,
                combined,
                trace_ids,
            })
            .unwrap();
        })
    })
}

// ─── enqueue básico ──────────────────────────────────────────────────

#[tokio::test]
async fn enqueue_single_invokes_runner_once() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let q = AgentQueue::new(AgentQueueConfig::new(recording_runner(tx)));

    assert!(q.enqueue("alice", "bob", "oi", None));

    let call = rx.recv().await.unwrap();
    assert_eq!(call.to_agent, "alice");
    assert_eq!(call.from_label, "bob");
    assert_eq!(call.combined, "oi");
    assert!(call.trace_ids.is_empty());
}

#[tokio::test]
async fn enqueue_with_trace_id_propagates() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let q = AgentQueue::new(AgentQueueConfig::new(recording_runner(tx)));

    q.enqueue("a", "b", "msg", Some("trace-1"));
    let call = rx.recv().await.unwrap();
    assert_eq!(call.trace_ids, vec!["trace-1"]);
}

#[tokio::test]
async fn enqueue_rejects_empty_to_or_from() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let q = AgentQueue::new(AgentQueueConfig::new(recording_runner(tx)));

    assert!(!q.enqueue("", "b", "msg", None));
    assert!(!q.enqueue("a", "", "msg", None));
    // Nenhum runner disparado
    assert!(rx.try_recv().is_err());
}

// ─── batching ────────────────────────────────────────────────────────

#[tokio::test]
async fn burst_same_agent_batches_into_single_runner_call() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let q = AgentQueue::new(AgentQueueConfig::new(recording_runner(tx)));

    // Dois enqueues sync antes de qualquer await → ambos vão pra pending
    // antes do drain spawnado rodar.
    q.enqueue("alice", "bob", "msg1", Some("t1"));
    q.enqueue("alice", "carol", "msg2", Some("t2"));

    let call = rx.recv().await.unwrap();
    assert_eq!(call.to_agent, "alice");
    assert_eq!(call.from_label, "múltiplos agentes");
    assert_eq!(call.combined, "[bob]: msg1\n\n[carol]: msg2");
    assert_eq!(call.trace_ids, vec!["t1", "t2"]);

    // Segundo runner call não deve existir
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn burst_different_agents_run_independently() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let q = AgentQueue::new(AgentQueueConfig::new(recording_runner(tx)));

    q.enqueue("alice", "bob", "m1", None);
    q.enqueue("carol", "bob", "m2", None);

    let mut calls = Vec::new();
    calls.push(rx.recv().await.unwrap());
    calls.push(rx.recv().await.unwrap());
    calls.sort_by(|a, b| a.to_agent.cmp(&b.to_agent));

    assert_eq!(calls[0].to_agent, "alice");
    assert_eq!(calls[0].combined, "m1");
    assert_eq!(calls[1].to_agent, "carol");
    assert_eq!(calls[1].combined, "m2");
}

// ─── rate limit ──────────────────────────────────────────────────────

#[tokio::test]
async fn rate_limit_blocks_after_max() {
    let (runner_tx, _runner_rx) = mpsc::unbounded_channel();
    let (throttle_tx, mut throttle_rx) = mpsc::unbounded_channel::<ThrottleEvent>();
    let throttle_tx_c = throttle_tx.clone();

    let mut cfg = AgentQueueConfig::new(recording_runner(runner_tx));
    cfg.rate_max = 2;
    cfg.rate_window = Duration::from_secs(60);
    cfg.on_throttle = Some(Arc::new(move |e| {
        throttle_tx_c.send(e).unwrap();
    }));
    let q = AgentQueue::new(cfg);

    assert!(q.enqueue("a", "b", "1", None));
    assert!(q.enqueue("a", "b", "2", None));
    assert!(!q.enqueue("a", "b", "3", None)); // bloqueado
    assert!(!q.enqueue("a", "b", "4", None));

    let ev1 = throttle_rx.recv().await.unwrap();
    assert_eq!(ev1.reason, "rate_limit");
    assert_eq!(ev1.rate_max, 2);
    let _ev2 = throttle_rx.recv().await.unwrap();
    assert!(throttle_rx.try_recv().is_err());
}

#[tokio::test]
async fn rate_limit_expires_after_window() {
    let (runner_tx, _rx) = mpsc::unbounded_channel();
    let clock = Arc::new(ManualClock::new());

    let mut cfg = AgentQueueConfig::new(recording_runner(runner_tx));
    cfg.rate_max = 1;
    cfg.rate_window = Duration::from_millis(1000);
    cfg.clock = clock.clone();
    let q = AgentQueue::new(cfg);

    assert!(q.enqueue("a", "b", "1", None));
    assert!(!q.enqueue("a", "b", "2", None)); // rate=1, bloqueado

    clock.advance(1001); // passa a janela
    assert!(q.enqueue("a", "b", "3", None));
}

// ─── acquire / release ───────────────────────────────────────────────

#[tokio::test]
async fn acquire_blocks_drain_release_resumes_it() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let q = AgentQueue::new(AgentQueueConfig::new(recording_runner(tx)));

    q.acquire("alice");
    q.enqueue("alice", "bob", "blocked", None);

    // Drain foi spawnado mas encontra `running.contains("alice")` → retorna.
    // Pra dar chance dessa task rodar e terminar sem consumir nada:
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    assert!(rx.try_recv().is_err());
    assert_eq!(q.pending_size("alice"), 1);

    q.release("alice"); // spawna novo drain
    let call = rx.recv().await.unwrap();
    assert_eq!(call.combined, "blocked");
}

#[tokio::test]
async fn acquire_empty_agent_is_noop() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let q = AgentQueue::new(AgentQueueConfig::new(recording_runner(tx)));
    q.acquire("");
    q.release("");
    assert_eq!(q.stats().running.len(), 0);
}

// ─── hooks ───────────────────────────────────────────────────────────

#[tokio::test]
async fn enqueue_and_drain_hooks_fire_in_order() {
    let (runner_tx, mut runner_rx) = mpsc::unbounded_channel();
    let (eq_tx, mut eq_rx) = mpsc::unbounded_channel::<EnqueueEvent>();
    let (ds_tx, mut ds_rx) = mpsc::unbounded_channel::<DrainStartEvent>();
    let (de_tx, mut de_rx) = mpsc::unbounded_channel::<DrainEndEvent>();

    let mut cfg = AgentQueueConfig::new(recording_runner(runner_tx));
    cfg.on_enqueue = Some(Arc::new(move |e| eq_tx.send(e).unwrap()));
    cfg.on_drain_start = Some(Arc::new(move |e| ds_tx.send(e).unwrap()));
    cfg.on_drain_end = Some(Arc::new(move |e| de_tx.send(e).unwrap()));
    let q = AgentQueue::new(cfg);

    q.enqueue("alice", "bob", "hi", Some("t1"));

    let ev = eq_rx.recv().await.unwrap();
    assert_eq!(ev.to_agent, "alice");
    assert_eq!(ev.size, 1);

    runner_rx.recv().await.unwrap();
    let ds = ds_rx.recv().await.unwrap();
    assert_eq!(ds.batch_size, 1);
    assert_eq!(ds.trace_ids, vec!["t1"]);

    let de = de_rx.recv().await.unwrap();
    assert_eq!(de.to_agent, "alice");
}

// ─── stats ───────────────────────────────────────────────────────────

#[tokio::test]
async fn stats_reflect_running_and_pending() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let q = AgentQueue::new(AgentQueueConfig::new(recording_runner(tx)));

    q.acquire("alice");
    q.enqueue("alice", "bob", "m1", None);
    q.enqueue("alice", "carol", "m2", None);

    // Dá chance do drain spawnado descobrir que `alice` está running e sair.
    tokio::task::yield_now().await;

    let s = q.stats();
    assert_eq!(s.running, vec!["alice".to_string()]);
    assert_eq!(s.pending.get("alice").copied(), Some(2));
    assert_eq!(s.rate_max, 40);
    assert_eq!(s.rate_window_ms, 60_000);
    assert_eq!(s.rate_used, 2);
}

#[tokio::test]
async fn pending_size_and_is_running_match_stats() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let q = AgentQueue::new(AgentQueueConfig::new(recording_runner(tx)));

    q.acquire("alice");
    q.enqueue("alice", "bob", "m", None);
    tokio::task::yield_now().await;

    assert!(q.is_running("alice"));
    assert_eq!(q.pending_size("alice"), 1);
    assert!(!q.is_running("carol"));
    assert_eq!(q.pending_size("carol"), 0);
}

// ─── drain_one manual (uso avançado) ─────────────────────────────────

#[tokio::test]
async fn drain_one_can_be_called_manually() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let q = AgentQueue::new(AgentQueueConfig::new(recording_runner(tx)));

    // Injeta direto em pending via enqueue, mas segura com acquire antes
    // pra drain spawnado não consumir.
    q.acquire("alice");
    q.enqueue("alice", "bob", "m", None);

    // Solta e chama manualmente
    // (simula use-case onde caller tem controle explícito do drain)
    q.release("alice");
    let _ = rx.recv().await;

    // Segunda rodada manual — fila vazia, drain_one é no-op
    let q_clone = Arc::clone(&q);
    q_clone.drain_one("alice".to_string()).await;
    assert!(rx.try_recv().is_err());
}
