'use strict';

// Message bus entre agentes — camada de garantias sobre o handoff existente.
//
// Filosofia: mesmo padrão do grafo — tudo em JSONL append-only, estado derivado
// por replay. Zero infraestrutura.
//
// Storage: chunks diários em `messages/YYYY-MM-DD.jsonl`. Arquivo legacy
// `messages.jsonl` é migrado automaticamente na primeira leitura. Compactação
// apaga chunks mais antigos que `retention_days` (default 14, configurável via
// bus-config.json).
//
// Eventos (type):
//   created    — handoff aceito pelo endpoint, bus assume custódia
//   delivered  — destinatário foi spawnado (claude -p iniciou)
//   responded  — destinatário saiu exit=0 com output não-vazio
//   acked      — destinatário enviou handoff com reply_to + intent=ack
//   reported   — destinatário enviou handoff com reply_to + intent=report
//   retry      — bus re-enfileirou após `delivery_deadline` expirado
//   timed_out  — delivered mas sem responded até `response_deadline`
//   failed     — esgotou retries ou escalação explícita
//
// Ciclo feliz: created → delivered → responded → (opcional acked/reported)
// Ciclo com retry: created → retry → delivered → responded
// Ciclo de timeout: created → delivered → timed_out → [escalate pra firefighter]

const fs   = require('fs');
const path = require('path');

const MSG_DIR              = 'messages';
const LEGACY_FILE          = 'messages.jsonl';
const CONFIG_FILE          = 'bus-config.json';
const DEFAULT_RETENTION    = 14;   // dias
const MIN_RETENTION        = 1;
const MAX_RETENTION        = 365;
const DELIVERY_DEADLINE_MS = 90 * 1000;          // created → delivered
const RESPONSE_DEADLINE_MS = 5  * 60 * 1000;     // delivered → responded
const MAX_ATTEMPTS         = 3;

// ─── Paths ──────────────────────────────────────────────────────────────────

function msgDir(memDir)      { return path.join(memDir, MSG_DIR); }
function legacyFile(memDir)  { return path.join(memDir, LEGACY_FILE); }
function configFile(memDir)  { return path.join(memDir, CONFIG_FILE); }

function ensureMsgDir(memDir) {
  const d = msgDir(memDir);
  try { fs.mkdirSync(d, { recursive: true }); } catch {}
  return d;
}

function dayKey(tsIso) {
  // Extrai YYYY-MM-DD do timestamp ISO (UTC-based — consistente mesmo em
  // múltiplos fusos). Se ts for inválido, cai no dia atual.
  const m = typeof tsIso === 'string' && tsIso.match(/^(\d{4}-\d{2}-\d{2})/);
  if (m) return m[1];
  return new Date().toISOString().slice(0, 10);
}

function dayFile(memDir, day) {
  return path.join(msgDir(memDir), `${day}.jsonl`);
}

function listDayFiles(memDir) {
  ensureMsgDir(memDir);
  let entries = [];
  try {
    entries = fs.readdirSync(msgDir(memDir))
      .filter(n => /^\d{4}-\d{2}-\d{2}\.jsonl$/.test(n))
      .sort();
  } catch {}
  return entries;
}

// ─── Config ─────────────────────────────────────────────────────────────────

function getConfig(memDir) {
  const defaults = { retention_days: DEFAULT_RETENTION };
  try {
    const raw = fs.readFileSync(configFile(memDir), 'utf8');
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === 'object') {
      return { ...defaults, ...parsed };
    }
  } catch {}
  return defaults;
}

function setConfig(memDir, patch) {
  const current = getConfig(memDir);
  const next = { ...current };
  if (patch && typeof patch === 'object') {
    if ('retention_days' in patch) {
      const n = Number(patch.retention_days);
      if (!Number.isFinite(n) || n < MIN_RETENTION || n > MAX_RETENTION) {
        throw new Error(`retention_days deve ser número entre ${MIN_RETENTION} e ${MAX_RETENTION}`);
      }
      next.retention_days = Math.floor(n);
    }
  }
  try { fs.writeFileSync(configFile(memDir), JSON.stringify(next, null, 2)); } catch {}
  return next;
}

// ─── Migração do legacy messages.jsonl ──────────────────────────────────────

// Lê o arquivo monolítico antigo e splita por dia. Chamado preguiçosamente
// na primeira leitura e na primeira escrita pós-deploy.
function migrateLegacy(memDir) {
  const legacy = legacyFile(memDir);
  if (!fs.existsSync(legacy)) return { migrated: 0 };
  ensureMsgDir(memDir);

  let raw;
  try { raw = fs.readFileSync(legacy, 'utf8'); } catch { return { migrated: 0 }; }
  const lines = raw.split('\n').filter(Boolean);
  const byDay = new Map();
  for (const line of lines) {
    let ev;
    try { ev = JSON.parse(line); } catch { continue; }
    if (!ev || !ev.ts) continue;
    const day = dayKey(ev.ts);
    if (!byDay.has(day)) byDay.set(day, []);
    byDay.get(day).push(line);
  }
  for (const [day, dayLines] of byDay) {
    const f = dayFile(memDir, day);
    try {
      const existing = fs.existsSync(f) ? fs.readFileSync(f, 'utf8') : '';
      const prefix = existing && !existing.endsWith('\n') ? existing + '\n' : existing;
      fs.writeFileSync(f, prefix + dayLines.join('\n') + '\n');
    } catch {}
  }
  try { fs.renameSync(legacy, legacy + '.migrated'); } catch {}
  return { migrated: lines.length, days: byDay.size };
}

// ─── Leitura ────────────────────────────────────────────────────────────────

function parseFile(filePath, out) {
  let raw;
  try { raw = fs.readFileSync(filePath, 'utf8'); } catch { return; }
  for (const line of raw.split('\n')) {
    if (!line) continue;
    try { out.push(JSON.parse(line)); } catch {}
  }
}

/**
 * readEvents(memDir, { sinceDays, sinceDate }) → Event[]
 *
 * - sinceDays: limita a leitura aos últimos N dias (default: todos os arquivos
 *   ainda presentes no disco, já respeita retenção após compactação).
 * - sinceDate: 'YYYY-MM-DD' — só lê arquivos desse dia em diante.
 *
 * Garante migração do legacy na primeira chamada.
 */
function readEvents(memDir, { sinceDays, sinceDate } = {}) {
  migrateLegacy(memDir);
  const days = listDayFiles(memDir);
  if (days.length === 0) return [];

  let cutoff = null;
  if (typeof sinceDays === 'number' && sinceDays > 0) {
    const d = new Date();
    d.setUTCDate(d.getUTCDate() - (sinceDays - 1));
    cutoff = d.toISOString().slice(0, 10);
  }
  if (sinceDate && (!cutoff || sinceDate > cutoff)) cutoff = sinceDate;

  const out = [];
  for (const name of days) {
    const day = name.replace(/\.jsonl$/, '');
    if (cutoff && day < cutoff) continue;
    parseFile(path.join(msgDir(memDir), name), out);
  }
  return out;
}

// ─── Escrita ────────────────────────────────────────────────────────────────

function emit(memDir, ev) {
  if (!ev || !ev.type || !ev.trace_id) return;
  migrateLegacy(memDir);
  ensureMsgDir(memDir);
  const entry = { ts: new Date().toISOString(), ...ev };
  const day = dayKey(entry.ts);
  try {
    fs.appendFileSync(dayFile(memDir, day), JSON.stringify(entry) + '\n');
  } catch {}
  return entry;
}

// ─── Compactação ────────────────────────────────────────────────────────────

/**
 * compactOldFiles(memDir, { retentionDays, now }) → { deleted, kept }
 *
 * Apaga chunks diários mais antigos que retentionDays. Default usa config.
 */
function compactOldFiles(memDir, { retentionDays, now } = {}) {
  const cfg = getConfig(memDir);
  const days = Number.isFinite(retentionDays) && retentionDays > 0
    ? Math.floor(retentionDays) : cfg.retention_days;
  const ref = now ? new Date(now) : new Date();
  ref.setUTCDate(ref.getUTCDate() - days);
  const cutoff = ref.toISOString().slice(0, 10);

  const deleted = [];
  const kept = [];
  for (const name of listDayFiles(memDir)) {
    const day = name.replace(/\.jsonl$/, '');
    const full = path.join(msgDir(memDir), name);
    if (day < cutoff) {
      try { fs.unlinkSync(full); deleted.push(day); } catch {}
    } else {
      kept.push(day);
    }
  }
  return { deleted, kept, retention_days: days, cutoff };
}

// ─── State derivation ────────────────────────────────────────────────────────

// Replay-based: dada toda a sequência de eventos, o estado atual de um trace
// é o último evento terminal (responded/acked/reported/failed/timed_out) ou o
// mais avançado intermediário (delivered/retry) ou 'created' se nada mais.

const STAGE_ORDER = {
  created: 1, retry: 1,
  delivered: 2,
  responded: 3,
  acked: 4, reported: 4,
  timed_out: 5, failed: 5,
};

function deriveState(events) {
  const byTrace = new Map();
  for (const ev of events) {
    const cur = byTrace.get(ev.trace_id);
    if (!cur || (STAGE_ORDER[ev.type] || 0) >= (STAGE_ORDER[cur.type] || 0)) {
      byTrace.set(ev.trace_id, ev);
    }
  }
  return byTrace;
}

// Retorna metadados completos de um trace: evento original `created` + estado
// atual. Útil pra decidir retry/resume.
function traceSummary(events, trace_id) {
  const related = events.filter(e => e.trace_id === trace_id);
  if (!related.length) return null;
  const created = related.find(e => e.type === 'created') || null;
  const latest  = related[related.length - 1];
  const attempts = related.filter(e => e.type === 'delivered' || e.type === 'retry').length;
  return { created, latest, attempts, events: related };
}

// ─── Queries públicas ────────────────────────────────────────────────────────

function getMessage(memDir, trace_id) {
  // Trace pode estar em qualquer dia; lê janela ampla pra não perder traces
  // antigos ainda dentro da retenção.
  const cfg = getConfig(memDir);
  return traceSummary(readEvents(memDir, { sinceDays: cfg.retention_days }), trace_id);
}

// Retorna todos os traces que ainda precisam de ação (não terminal).
function getPending(memDir) {
  // Traces não-terminais são quase sempre recentes (deadlines curtos). Ler
  // últimos 2 dias é suficiente pra cobrir handoffs que viraram durante a
  // meia-noite.
  const events = readEvents(memDir, { sinceDays: 2 });
  const states = deriveState(events);
  const pending = [];
  for (const [trace_id, last] of states) {
    if (last.type === 'responded' || last.type === 'acked' ||
        last.type === 'reported' || last.type === 'failed' ||
        last.type === 'timed_out') continue;
    const summary = traceSummary(events, trace_id);
    pending.push({ trace_id, state: last.type, ...summary });
  }
  return pending;
}

// Panorama geral — útil pro /api/bus/state e pra UI.
function getState(memDir, { recent = 50, sinceDays } = {}) {
  const cfg = getConfig(memDir);
  const win = sinceDays || cfg.retention_days;
  const events = readEvents(memDir, { sinceDays: win });
  const states = deriveState(events);
  const buckets = { created: [], delivered: [], responded: [], acked: [],
                    reported: [], retry: [], timed_out: [], failed: [] };
  for (const [trace_id, last] of states) {
    if (buckets[last.type]) buckets[last.type].push({ trace_id, ...last });
  }
  return {
    totals: Object.fromEntries(Object.entries(buckets).map(([k, v]) => [k, v.length])),
    pending: [...buckets.created, ...buckets.delivered, ...buckets.retry],
    recent_events: events.slice(-recent),
    retention_days: cfg.retention_days,
    day_files: listDayFiles(memDir),
  };
}

// ─── Sweep: retry + timeout ─────────────────────────────────────────────────

/**
 * sweepPending(memDir, { onRetry, onTimeout, now }) → { retried, timedOut }
 *
 * - `created` sem `delivered` em DELIVERY_DEADLINE_MS → retry até MAX_ATTEMPTS,
 *   depois failed.
 * - `delivered` sem `responded` em RESPONSE_DEADLINE_MS → timed_out.
 *
 * Só olha os últimos 2 dias — deadlines são curtos (< 10min), janela maior é
 * desperdício.
 *
 * Callbacks são onde a ação real acontece (re-enqueue, escalar firefighter).
 * Bus só decide quando — não decide como.
 */
function sweepPending(memDir, { onRetry, onTimeout, now } = {}) {
  const events = readEvents(memDir, { sinceDays: 2 });
  const states = deriveState(events);
  const nowMs = now ? new Date(now).getTime() : Date.now();
  const retried = [];
  const timedOut = [];

  for (const [trace_id, last] of states) {
    const summary = traceSummary(events, trace_id);
    if (!summary || !summary.created) continue;
    const lastMs = new Date(last.ts).getTime();
    const age = nowMs - lastMs;

    if ((last.type === 'created' || last.type === 'retry') && age > DELIVERY_DEADLINE_MS) {
      if (summary.attempts >= MAX_ATTEMPTS) {
        emit(memDir, {
          type: 'failed', trace_id,
          reason: 'max_delivery_attempts',
          attempts: summary.attempts,
        });
        retried.push({ trace_id, outcome: 'failed' });
        continue;
      }
      emit(memDir, {
        type: 'retry', trace_id,
        attempt: summary.attempts + 1,
        reason: 'delivery_deadline',
      });
      if (typeof onRetry === 'function') {
        try { onRetry(summary.created); } catch {}
      }
      retried.push({ trace_id, outcome: 'retry', attempt: summary.attempts + 1 });
    } else if (last.type === 'delivered' && age > RESPONSE_DEADLINE_MS) {
      emit(memDir, {
        type: 'timed_out', trace_id,
        stage: 'response',
        waited_ms: age,
      });
      if (typeof onTimeout === 'function') {
        try { onTimeout(summary.created); } catch {}
      }
      timedOut.push({ trace_id });
    }
  }

  return { retried, timedOut };
}

// ─── Constantes exportadas pra testes e integração ──────────────────────────

module.exports = {
  emit,
  readEvents,
  deriveState,
  traceSummary,
  getMessage,
  getPending,
  getState,
  sweepPending,
  getConfig,
  setConfig,
  compactOldFiles,
  migrateLegacy,
  listDayFiles,
  DELIVERY_DEADLINE_MS,
  RESPONSE_DEADLINE_MS,
  MAX_ATTEMPTS,
  DEFAULT_RETENTION,
  MIN_RETENTION,
  MAX_RETENTION,
};
