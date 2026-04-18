'use strict';

// Motor genérico de fila de turns por agente.
//
// Responsável por:
//  - manter estado de quem está rodando (running Set) e quem está pendente
//    (pending Map)
//  - rate-limit global (janela deslizante de N execuções em M ms)
//  - drenar a fila em lote: acumula múltiplos pedidos pro mesmo agente em uma
//    única chamada do runner (evita spawn N vezes quando N pedidos chegam em
//    rajada)
//
// Filosofia: o motor NÃO sabe como spawnar claude, NÃO sabe de bus/logging.
// Recebe tudo via callback/hooks. Isso permite reuso (auto-turns, shadow
// agents, futuros workers) e testabilidade.
//
// Uso:
//
//   const q = createAgentQueue({
//     runner: (toAgent, fromLabel, combinedMessage, traceIds) => Promise<void>,
//     rateMax: 40,
//     rateWindowMs: 60_000,
//     onThrottle: ({ toAgent, fromAgent, traceId }) => { ... },   // opcional
//     onEnqueue:  ({ toAgent, fromAgent, traceId, size }) => { }, // opcional
//     onDrainStart: ({ toAgent, batchSize }) => {},               // opcional
//     onDrainEnd:   ({ toAgent }) => {},                          // opcional
//   });
//
//   q.enqueue(toAgent, fromAgent, message, traceId);
//   q.stats() // { running: Set, pending: Map, rateUsed, rateMax }

function createAgentQueue(options = {}) {
  const runner        = options.runner;
  const rateMax       = Number.isFinite(options.rateMax) ? options.rateMax : 40;
  const rateWindowMs  = Number.isFinite(options.rateWindowMs) ? options.rateWindowMs : 60_000;
  const onThrottle    = typeof options.onThrottle === 'function'   ? options.onThrottle   : null;
  const onEnqueue     = typeof options.onEnqueue === 'function'    ? options.onEnqueue    : null;
  const onDrainStart  = typeof options.onDrainStart === 'function' ? options.onDrainStart : null;
  const onDrainEnd    = typeof options.onDrainEnd === 'function'   ? options.onDrainEnd   : null;

  if (typeof runner !== 'function') {
    throw new Error('createAgentQueue: options.runner é obrigatório');
  }

  const running   = new Set();
  const pending   = new Map();   // toAgent -> Array<{ fromAgent, message, traceId }>
  const rateStamps = [];

  function rateOk() {
    const now = Date.now();
    while (rateStamps.length && now - rateStamps[0] > rateWindowMs) rateStamps.shift();
    if (rateStamps.length >= rateMax) return false;
    rateStamps.push(now);
    return true;
  }

  function enqueue(toAgent, fromAgent, message, traceId) {
    if (!toAgent || !fromAgent) return false;
    if (!rateOk()) {
      if (onThrottle) {
        try { onThrottle({ toAgent, fromAgent, traceId: traceId || null, reason: 'rate_limit', rateMax, rateWindowMs }); } catch {}
      }
      return false;
    }
    if (!pending.has(toAgent)) pending.set(toAgent, []);
    const entry = { fromAgent, message, traceId: traceId || null };
    pending.get(toAgent).push(entry);
    if (onEnqueue) {
      try { onEnqueue({ toAgent, fromAgent, traceId: entry.traceId, size: pending.get(toAgent).length }); } catch {}
    }
    setImmediate(() => drain(toAgent));
    return true;
  }

  async function drain(toAgent) {
    if (running.has(toAgent)) return;
    const q = pending.get(toAgent);
    if (!q || !q.length) return;
    running.add(toAgent);
    try {
      while (q.length) {
        const batch = q.splice(0);
        const fromLabel = batch.length === 1 ? batch[0].fromAgent : 'múltiplos agentes';
        const combined = batch.length === 1
          ? batch[0].message
          : batch.map(b => `[${b.fromAgent}]: ${b.message}`).join('\n\n');
        const traceIds = batch.map(b => b.traceId).filter(Boolean);
        if (onDrainStart) {
          try { onDrainStart({ toAgent, batchSize: batch.length, traceIds }); } catch {}
        }
        try {
          await runner(toAgent, fromLabel, combined, traceIds);
        } catch (err) {
          // Runner engole erros via hooks próprios; aqui só evita quebrar o loop.
        }
      }
    } finally {
      running.delete(toAgent);
      if (onDrainEnd) {
        try { onDrainEnd({ toAgent }); } catch {}
      }
      // Se chegou algo novo durante o drain, re-entra.
      if ((pending.get(toAgent) || []).length) setImmediate(() => drain(toAgent));
    }
  }

  function stats() {
    return {
      running: Array.from(running),
      pending: Object.fromEntries(
        Array.from(pending.entries()).map(([k, v]) => [k, v.length])
      ),
      rate_used: rateStamps.length,
      rate_max: rateMax,
      rate_window_ms: rateWindowMs,
    };
  }

  function isRunning(toAgent) { return running.has(toAgent); }
  function pendingSize(toAgent) { return (pending.get(toAgent) || []).length; }

  // acquire/release: permite que um consumer externo (ex: chat humano com
  // streaming) reserve um agente enquanto processa, bloqueando o drain
  // automático. Ao liberar, drain é re-disparado se tiver pending.
  function acquire(toAgent) {
    if (!toAgent) return;
    running.add(toAgent);
  }
  function release(toAgent) {
    if (!toAgent) return;
    running.delete(toAgent);
    if ((pending.get(toAgent) || []).length) setImmediate(() => drain(toAgent));
  }

  return {
    enqueue,
    drain,
    stats,
    isRunning,
    pendingSize,
    acquire,
    release,
    // Expostos pra testes e introspecção controlada
    _running: running,
    _pending: pending,
  };
}

module.exports = { createAgentQueue };
