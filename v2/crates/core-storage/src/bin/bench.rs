//! Benchmark sintético do JsonlRepository. Mede teto teórico do storage
//! layer puro (sem HTTP nem agentes). Resultados pra dar ordem de
//! grandeza do "limite por minuto" prometido.
//!
//! Uso:
//!   cargo run --release -p core-storage --bin bench
//!
//! Saída exemplo:
//!   append (fsync off): 1.234M ops/min
//!   batch×1000        : 12.5M ops/min
//!   read_all (100k)   : 250k items/s
//!
//! Disclaimer: SSD local, single-thread, no contention. Em multi-thread
//! com lock contention o número cai. Em rede / disco compartilhado
//! também. Use como upper bound, não promessa.
//!
//! Pra serem comparáveis, ambos modos rodam mesma quantidade de items
//! (mesmo working set) — o que muda é como são escritos.

use std::time::Instant;

use core_storage::{JsonlRepository, RotationPolicy};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

#[derive(Serialize, Deserialize, Clone)]
struct Item {
    ts: u64,
    role: String,
    content: String,
    trace_id: String,
}

fn sample_item(i: u64) -> Item {
    Item {
        ts: 1_700_000_000_000 + i,
        role: if i % 2 == 0 { "user".into() } else { "agent".into() },
        content: format!("benchmark message #{i} — algumas palavras pra simular tamanho típico de chat turn"),
        trace_id: format!("t_{i:08x}"),
    }
}

fn fmt_per_min(ops: u64, elapsed_s: f64) -> String {
    let per_s = ops as f64 / elapsed_s;
    let per_min = per_s * 60.0;
    if per_min >= 1_000_000.0 {
        format!("{:.2}M ops/min ({:.0} ops/s)", per_min / 1_000_000.0, per_s)
    } else if per_min >= 1_000.0 {
        format!("{:.1}k ops/min ({:.0} ops/s)", per_min / 1_000.0, per_s)
    } else {
        format!("{:.0} ops/min ({:.0} ops/s)", per_min, per_s)
    }
}

fn bench_append_single(n: u64, fsync: bool, rotate_lines: usize) {
    let tmp = TempDir::new().unwrap();
    let mut policy = RotationPolicy::default();
    policy.max_lines = rotate_lines;
    policy.fsync_on_append = fsync;
    let r: JsonlRepository<Item> =
        JsonlRepository::open(tmp.path().join("bench.jsonl"), policy).unwrap();

    let start = Instant::now();
    for i in 0..n {
        r.append(&sample_item(i)).unwrap();
    }
    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "  append × {} (fsync={}, rotate@{}): {}  [{:.2}s]",
        n,
        fsync,
        rotate_lines,
        fmt_per_min(n, elapsed),
        elapsed
    );
}

fn bench_append_batch(batches: u64, batch_size: u64, fsync: bool) {
    let tmp = TempDir::new().unwrap();
    let mut policy = RotationPolicy::default();
    policy.max_lines = usize::MAX; // pra isolar o custo do batch, sem rotação
    policy.fsync_on_append = fsync;
    let r: JsonlRepository<Item> =
        JsonlRepository::open(tmp.path().join("bench.jsonl"), policy).unwrap();
    let total = batches * batch_size;
    let items: Vec<Item> = (0..batch_size).map(sample_item).collect();

    let start = Instant::now();
    for _ in 0..batches {
        r.append_batch(&items).unwrap();
    }
    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "  batch × {} (size={}, fsync={}): {}  [{:.2}s]",
        batches,
        batch_size,
        fsync,
        fmt_per_min(total, elapsed),
        elapsed
    );
}

fn bench_read(n: u64) {
    let tmp = TempDir::new().unwrap();
    let mut policy = RotationPolicy::default();
    policy.max_lines = usize::MAX; // sem rotação no benchmark de read
    policy.max_bytes = u64::MAX;
    let r: JsonlRepository<Item> =
        JsonlRepository::open(tmp.path().join("bench.jsonl"), policy).unwrap();
    let items: Vec<Item> = (0..n).map(sample_item).collect();
    r.append_batch(&items).unwrap();

    let start = Instant::now();
    let all = r.read_all();
    let elapsed = start.elapsed().as_secs_f64();
    assert_eq!(all.len(), n as usize);
    let per_s = n as f64 / elapsed;
    println!(
        "  read_all (n={}): {:.0} items/s  [{:.2}s]",
        n, per_s, elapsed
    );

    let start = Instant::now();
    let t = r.tail(100);
    let elapsed = start.elapsed().as_secs_f64();
    assert_eq!(t.len(), 100);
    println!(
        "  tail(100) over {}: {:.0}µs",
        n,
        elapsed * 1_000_000.0
    );
}

fn main() {
    println!("== core-storage JsonlRepository bench ==\n");
    println!("Item size: ~150 bytes serialized (chat turn realista)\n");

    println!("[append] single, fsync OFF (default; durabilidade weak, throughput max):");
    bench_append_single(10_000, false, 50_000);
    bench_append_single(50_000, false, 50_000);

    println!("\n[append] single, fsync ON (durabilidade strong, throughput-bound by disk):");
    bench_append_single(1_000, true, 50_000);
    bench_append_single(10_000, true, 50_000);

    println!("\n[batch] N items por chamada, fsync OFF:");
    bench_append_batch(100, 10, false);
    bench_append_batch(100, 100, false);
    bench_append_batch(100, 1000, false);
    bench_append_batch(10, 10_000, false);

    println!("\n[batch] N items por chamada, fsync ON:");
    bench_append_batch(100, 100, true);
    bench_append_batch(100, 1000, true);

    println!("\n[rotation overhead] append single com rotação frequente:");
    bench_append_single(10_000, false, 1_000);  // rotaciona a cada 1k
    bench_append_single(10_000, false, 100);    // rotaciona a cada 100 (extremo)

    println!("\n[read]");
    bench_read(10_000);
    bench_read(100_000);

    println!("\n== fim ==");
}
