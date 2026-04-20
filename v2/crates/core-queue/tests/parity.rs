//! Paridade `core-queue` (Rust) ↔ `plugin/scripts/queue.cjs`.
//!
//! Foco: dado o mesmo cenário (configuração + sequência de enqueues),
//! o motor dos dois lados deve produzir a MESMA lista de invocações
//! do runner (to, fromLabel, combined, traceIds) e o mesmo conjunto
//! de throttle events. Tempo não é byte-exato — batching é a invariante
//! interessante aqui.
//!
//! Estratégia: um script Node inline (via `node -e`) cria a fila com
//! o runner e hooks instrumentados, roda a sequência, aguarda idle,
//! e emite um único JSON no stdout com `{ calls, throttles }`. Rust
//! faz o mesmo e compara por igualdade.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use core_queue::{AgentQueue, AgentQueueConfig, BoxFut, Runner, ThrottleEvent};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::sync::Mutex;

// ─── Helpers ─────────────────────────────────────────────────────────

fn queue_cjs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("locate repo root")
        .join("plugin/scripts/queue.cjs")
}

/// Roda um script JS via `node -e`, valida status e faz parse do stdout
/// como JSON. Erros do Node aparecem no stderr da task (`inherit`).
fn run_node(script: &str) -> Value {
    let out = Command::new("node")
        .arg("-e")
        .arg(script)
        .output()
        .expect("spawn node");
    if !out.status.success() {
        panic!(
            "node failed (status {:?}):\n--- stdout ---\n{}\n--- stderr ---\n{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    serde_json::from_slice(&out.stdout).expect("parse node stdout as JSON")
}

/// Build Rust-side recording runner (captura chamadas num Vec).
type Calls = Arc<Mutex<Vec<Value>>>;

fn recording_runner(calls: Calls) -> Runner {
    Arc::new(move |to, from, combined, trace_ids| -> BoxFut {
        let calls = calls.clone();
        Box::pin(async move {
            let mut g = calls.lock().await;
            g.push(json!({
                "to": to,
                "from": from,
                "combined": combined,
                "trace_ids": trace_ids,
            }));
        })
    })
}

fn throttle_hook(tx: mpsc::UnboundedSender<Value>) -> Arc<dyn Fn(ThrottleEvent) + Send + Sync> {
    Arc::new(move |e| {
        tx.send(json!({
            "to": e.to_agent,
            "from": e.from_agent,
            "trace_id": e.trace_id,
            "reason": e.reason,
        }))
        .unwrap();
    })
}

/// Aguarda a fila ficar ociosa (sem running, sem pending em nenhum
/// agente listado). Usa polling por yield — single-threaded tokio
/// processa tasks na mesma ordem sem sleeps reais.
async fn wait_idle(q: &Arc<AgentQueue>, agents: &[&str]) {
    loop {
        let busy = agents
            .iter()
            .any(|a| q.is_running(a) || q.pending_size(a) > 0);
        if !busy {
            break;
        }
        tokio::task::yield_now().await;
    }
    // Um yield extra para garantir que on_drain_end foi processado.
    tokio::task::yield_now().await;
}

/// Normaliza throttles ordenando por (to, from, reason) — Node dispara
/// em ordem de chegada, Rust idem, mas em testes vamos ordenar só pra
/// tolerar reordenação caso surja.
fn sort_throttles(mut v: Vec<Value>) -> Vec<Value> {
    v.sort_by(|a, b| {
        a["to"]
            .as_str()
            .cmp(&b["to"].as_str())
            .then(a["from"].as_str().cmp(&b["from"].as_str()))
    });
    v
}

// ─── Testes ──────────────────────────────────────────────────────────

#[tokio::test]
async fn parity_single_enqueue() {
    // Node
    let script = format!(
        r#"
const {{ createAgentQueue }} = require({path});
const calls = [];
const q = createAgentQueue({{
    rateMax: 1000,
    rateWindowMs: 60000,
    runner: async (to, from, combined, traceIds) => {{
        calls.push({{ to, from, combined, trace_ids: traceIds }});
    }},
}});
q.enqueue('alice', 'bob', 'hi', 't1');
async function waitIdle() {{
    while (q.stats().running.length > 0 ||
           Object.values(q.stats().pending).reduce((a,b) => a+b, 0) > 0) {{
        await new Promise(r => setImmediate(r));
    }}
    await new Promise(r => setImmediate(r));
}}
waitIdle().then(() => {{
    process.stdout.write(JSON.stringify({{ calls }}));
}});
"#,
        path = json!(queue_cjs().to_string_lossy())
    );
    let node_out = run_node(&script);

    // Rust
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let q = AgentQueue::new(AgentQueueConfig::new(recording_runner(calls.clone())));
    q.enqueue("alice", "bob", "hi", Some("t1"));
    wait_idle(&q, &["alice"]).await;
    let rust_calls = calls.lock().await.clone();

    assert_eq!(node_out["calls"], Value::Array(rust_calls));
}

#[tokio::test]
async fn parity_burst_batches_same_agent() {
    let script = format!(
        r#"
const {{ createAgentQueue }} = require({path});
const calls = [];
const q = createAgentQueue({{
    rateMax: 1000,
    rateWindowMs: 60000,
    runner: async (to, from, combined, traceIds) => {{
        calls.push({{ to, from, combined, trace_ids: traceIds }});
    }},
}});
q.enqueue('alice', 'bob', 'm1', 't1');
q.enqueue('alice', 'carol', 'm2', 't2');
q.enqueue('alice', 'dave', 'm3', null);
async function waitIdle() {{
    while (q.stats().running.length > 0 ||
           Object.values(q.stats().pending).reduce((a,b) => a+b, 0) > 0) {{
        await new Promise(r => setImmediate(r));
    }}
    await new Promise(r => setImmediate(r));
}}
waitIdle().then(() => {{
    process.stdout.write(JSON.stringify({{ calls }}));
}});
"#,
        path = json!(queue_cjs().to_string_lossy())
    );
    let node_out = run_node(&script);

    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let q = AgentQueue::new(AgentQueueConfig::new(recording_runner(calls.clone())));
    q.enqueue("alice", "bob", "m1", Some("t1"));
    q.enqueue("alice", "carol", "m2", Some("t2"));
    q.enqueue("alice", "dave", "m3", None);
    wait_idle(&q, &["alice"]).await;
    let rust_calls = calls.lock().await.clone();

    assert_eq!(node_out["calls"], Value::Array(rust_calls));
}

#[tokio::test]
async fn parity_throttle_events() {
    let script = format!(
        r#"
const {{ createAgentQueue }} = require({path});
const throttles = [];
const calls = [];
const q = createAgentQueue({{
    rateMax: 2,
    rateWindowMs: 60000,
    runner: async (to, from, combined, traceIds) => {{
        calls.push({{ to, from, combined, trace_ids: traceIds }});
    }},
    onThrottle: (e) => {{
        throttles.push({{
            to: e.toAgent, from: e.fromAgent,
            trace_id: e.traceId, reason: e.reason,
        }});
    }},
}});
q.enqueue('alice', 'bob', 'm1', 't1');
q.enqueue('alice', 'bob', 'm2', 't2');
q.enqueue('alice', 'bob', 'm3', 't3');
q.enqueue('alice', 'bob', 'm4', 't4');
async function waitIdle() {{
    while (q.stats().running.length > 0 ||
           Object.values(q.stats().pending).reduce((a,b) => a+b, 0) > 0) {{
        await new Promise(r => setImmediate(r));
    }}
    await new Promise(r => setImmediate(r));
}}
waitIdle().then(() => {{
    process.stdout.write(JSON.stringify({{ calls, throttles }}));
}});
"#,
        path = json!(queue_cjs().to_string_lossy())
    );
    let node_out = run_node(&script);

    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let (throttle_tx, mut throttle_rx) = mpsc::unbounded_channel::<Value>();
    let mut cfg = AgentQueueConfig::new(recording_runner(calls.clone()));
    cfg.rate_max = 2;
    cfg.rate_window = Duration::from_secs(60);
    cfg.on_throttle = Some(throttle_hook(throttle_tx));
    let q = AgentQueue::new(cfg);

    q.enqueue("alice", "bob", "m1", Some("t1"));
    q.enqueue("alice", "bob", "m2", Some("t2"));
    q.enqueue("alice", "bob", "m3", Some("t3"));
    q.enqueue("alice", "bob", "m4", Some("t4"));
    wait_idle(&q, &["alice"]).await;
    let rust_calls = calls.lock().await.clone();
    let mut rust_throttles = Vec::new();
    while let Ok(t) = throttle_rx.try_recv() {
        rust_throttles.push(t);
    }

    assert_eq!(node_out["calls"], Value::Array(rust_calls));
    assert_eq!(
        sort_throttles(node_out["throttles"].as_array().unwrap().clone()),
        sort_throttles(rust_throttles)
    );
}

#[tokio::test]
async fn parity_acquire_blocks_release_resumes() {
    let script = format!(
        r#"
const {{ createAgentQueue }} = require({path});
const calls = [];
const q = createAgentQueue({{
    rateMax: 1000,
    rateWindowMs: 60000,
    runner: async (to, from, combined, traceIds) => {{
        calls.push({{ to, from, combined, trace_ids: traceIds }});
    }},
}});
q.acquire('alice');
q.enqueue('alice', 'bob', 'm1', 't1');
q.enqueue('alice', 'carol', 'm2', 't2');
(async () => {{
    // Dá chance do drain spawnado rodar e encontrar alice running → sai.
    await new Promise(r => setImmediate(r));
    await new Promise(r => setImmediate(r));
    q.release('alice');
    while (q.stats().running.length > 0 ||
           Object.values(q.stats().pending).reduce((a,b) => a+b, 0) > 0) {{
        await new Promise(r => setImmediate(r));
    }}
    await new Promise(r => setImmediate(r));
    process.stdout.write(JSON.stringify({{ calls }}));
}})();
"#,
        path = json!(queue_cjs().to_string_lossy())
    );
    let node_out = run_node(&script);

    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let q = AgentQueue::new(AgentQueueConfig::new(recording_runner(calls.clone())));
    q.acquire("alice");
    q.enqueue("alice", "bob", "m1", Some("t1"));
    q.enqueue("alice", "carol", "m2", Some("t2"));
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    q.release("alice");
    wait_idle(&q, &["alice"]).await;
    let rust_calls = calls.lock().await.clone();

    assert_eq!(node_out["calls"], Value::Array(rust_calls));
}
