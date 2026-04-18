'use strict';

// Grafo de conhecimento do projeto (v2).
//
// Fonte da verdade: rebuildGraph() replica TODOS os JSONLs já existentes
// (observations, agent_logs, agent_chats, human_chats, decisions, gotchas)
// e reconstrói graph.json do zero. Incremental updates (updateGraph,
// onAgentLogEvent, onChatRow, onDecision, onGotcha) mantêm o grafo vivo em
// produção — se divergirem, um rebuild corrige.
//
// Se graph.json tem version < atual, carregar dispara rebuild automático.
//
// Nós:
//   file       — arquivo editado
//   command    — comando bash (normalizado: 3 primeiras palavras)
//   agent      — agente do manifest
//   human      — humano (de humans.json / human_chats)
//   decision   — decisão persistida (decisions.jsonl)
//   gotcha     — armadilha persistida (gotchas.jsonl)
//
// Arestas:
//   co_edited        file ↔ file       editados na mesma janela/sessão
//   followed_by      file → command    editei → rodei
//   then_edited      command → file    rodei → editei
//   edits            agent → file      agente editou arquivo (agent_id na obs)
//   runs             agent → command   agente rodou comando
//   invokes          agent → agent     Task/Agent tool
//   handoff          agent → agent     handoff_out de agent_logs
//   violation        agent → agent     handoff_denied (rompeu can_talk_to)
//   escalates        agent → agent     auto_escalation_out
//   talks_to         human → agent     agent_chats onde `from` é humano
//   talks_with       human ↔ human     human_chats
//   touches          decision → file   arquivos editados na mesma sessão da decisão
//   gotcha_for       gotcha → file     campo `file` da gotcha

const fs   = require('fs');
const path = require('path');

const GRAPH_FILE             = 'graph.json';
const VERSION                = 2;
const COUPLING_WINDOW_MS     = 15 * 60 * 1000;       // janela de co-ocorrência em observations
const HALF_LIFE_DAYS         = 30;                   // decay pra ranking
const HALF_LIFE_MS           = HALF_LIFE_DAYS * 864e5;

// ─── I/O ──────────────────────────────────────────────────────────────────────

function empty() {
  return { version: VERSION, rebuilt_at: null, updated_at: null, nodes: {}, edges: {} };
}

function graphPath(memDir) { return path.join(memDir, GRAPH_FILE); }

function loadGraph(memDir) {
  const file = graphPath(memDir);
  if (!fs.existsSync(file)) return empty();
  let g;
  try { g = JSON.parse(fs.readFileSync(file, 'utf8')); }
  catch { return empty(); }
  if (!g || typeof g !== 'object') return empty();
  if (g.version !== VERSION) {
    // Auto-migra: o grafo v1 (ou anterior) é descartável — rebuild recupera tudo.
    try {
      const { readJsonl } = require('./lib.cjs');
      const fresh = rebuildGraph(memDir, readJsonl);
      return fresh;
    } catch { return empty(); }
  }
  return g;
}

function saveGraph(memDir, graph) {
  graph.updated_at = new Date().toISOString();
  fs.writeFileSync(graphPath(memDir), JSON.stringify(graph, null, 2));
}

// ─── Primitives ──────────────────────────────────────────────────────────────

function nodeId(kind, key) { return `${kind}:${key}`; }

function touchNode(graph, id, kind, label, ts) {
  let n = graph.nodes[id];
  if (!n) { n = graph.nodes[id] = { kind, label, count: 0, last_seen: ts }; }
  n.count++;
  if (!n.last_seen || new Date(ts) > new Date(n.last_seen)) n.last_seen = ts;
  if (label && !n.label) n.label = label;
}

function touchEdge(graph, fromId, toId, rel, ts, meta) {
  const key = `${fromId}→${toId}→${rel}`;
  let e = graph.edges[key];
  if (!e) { e = graph.edges[key] = { from: fromId, to: toId, rel, count: 0, last_seen: ts }; }
  e.count++;
  if (!e.last_seen || new Date(ts) > new Date(e.last_seen)) e.last_seen = ts;
  // Tally por intent em arestas de handoff/escalates/violation (suporta ranking
  // com granularidade "quantos ask vs delegate vs ack entre A e B").
  if (meta && meta.intent) {
    if (!e.intents) e.intents = {};
    e.intents[meta.intent] = (e.intents[meta.intent] || 0) + 1;
  }
}

function scoreOf(entity, now) {
  if (!entity || !entity.last_seen) return entity?.count || 0;
  const age = now - new Date(entity.last_seen).getTime();
  if (!Number.isFinite(age) || age < 0) return entity.count;
  return entity.count * Math.exp(-age / HALF_LIFE_MS);
}

function sameWindow(ts1, ts2, windowMs = COUPLING_WINDOW_MS) {
  return Math.abs(new Date(ts1) - new Date(ts2)) <= windowMs;
}

// ─── Observation helpers (file/bash/agent_call) ─────────────────────────────

function obsNodeId(obs) {
  if (obs.type === 'file_edit') return nodeId('file', obs.file);
  if (obs.type === 'bash') {
    const norm = (obs.cmd || '').trim().replace(/\s+/g, ' ').split(' ').slice(0, 3).join(' ');
    return nodeId('command', norm);
  }
  return null;
}

function obsLabel(obs) {
  if (obs.type === 'file_edit') return obs.file;
  if (obs.type === 'bash') return (obs.cmd || '').trim().split(' ').slice(0, 4).join(' ');
  return '';
}

// Ingere UMA observação e (opcionalmente) sua anterior pra criar arestas de
// co-ocorrência. Lida também com agent_call e com obs.agent_id.
function ingestObservation(graph, current, prev) {
  if (!current || !current.ts) return;

  if (current.type === 'agent_call' && current.agent) {
    touchNode(graph, nodeId('agent', current.agent), 'agent', current.agent, current.ts);
    if (current.agent_id) {
      touchNode(graph, nodeId('agent', current.agent_id), 'agent', current.agent_id, current.ts);
      touchEdge(graph, nodeId('agent', current.agent_id), nodeId('agent', current.agent),
                'invokes', current.ts);
    }
    return;
  }

  const curId = obsNodeId(current);
  if (!curId) return;

  const kind = current.type === 'file_edit' ? 'file' : 'command';
  touchNode(graph, curId, kind, obsLabel(current), current.ts);

  // Autoria por agente (quando o PostToolUse veio de subprocesso de agente)
  if (current.agent_id) {
    const aId = nodeId('agent', current.agent_id);
    touchNode(graph, aId, 'agent', current.agent_id, current.ts);
    touchEdge(graph, aId, curId, current.type === 'file_edit' ? 'edits' : 'runs', current.ts);
  }

  if (prev && prev.session_id === current.session_id && sameWindow(prev.ts, current.ts)) {
    const prevId = obsNodeId(prev);
    if (prevId && prevId !== curId) {
      if (prev.type === 'file_edit' && current.type === 'file_edit') {
        touchEdge(graph, prevId, curId, 'co_edited', current.ts);
        touchEdge(graph, curId, prevId, 'co_edited', current.ts);
      } else if (prev.type === 'file_edit' && current.type === 'bash') {
        touchEdge(graph, prevId, curId, 'followed_by', current.ts);
      } else if (prev.type === 'bash' && current.type === 'file_edit') {
        touchEdge(graph, prevId, curId, 'then_edited', current.ts);
      }
    }
  }
}

// ─── Agent log events ────────────────────────────────────────────────────────

function ingestAgentLogEvent(graph, agent, ev) {
  if (!agent || !ev || !ev.ts) return;
  const aId = nodeId('agent', agent);
  touchNode(graph, aId, 'agent', agent, ev.ts);

  const meta = ev.intent ? { intent: ev.intent } : null;
  if (ev.type === 'handoff_out' && ev.target) {
    // `handoff_out` é fonte da verdade — `handoff_in` é contrapartida no canal
    // do destinatário e seria double-count se também tocasse aresta.
    const tId = nodeId('agent', ev.target);
    touchNode(graph, tId, 'agent', ev.target, ev.ts);
    touchEdge(graph, aId, tId, 'handoff', ev.ts, meta);
  } else if (ev.type === 'handoff_in' && ev.source) {
    // Só garante que os nós existem — não cria aresta.
    touchNode(graph, nodeId('agent', ev.source), 'agent', ev.source, ev.ts);
  } else if (ev.type === 'violation' && ev.target) {
    const tId = nodeId('agent', ev.target);
    touchNode(graph, tId, 'agent', ev.target, ev.ts);
    touchEdge(graph, aId, tId, 'violation', ev.ts);
  } else if (ev.type === 'auto_escalation_out' && ev.target) {
    const tId = nodeId('agent', ev.target);
    touchNode(graph, tId, 'agent', ev.target, ev.ts);
    touchEdge(graph, aId, tId, 'escalates', ev.ts);
  }
}

// ─── Agent chat rows ─────────────────────────────────────────────────────────
// Row shape: { ts, role: 'user'|'agent', from?, content, auto?, ... }
// `from` pode ser agent OU human — precisamos do conjunto conhecido pra distinguir.

// humanLookup: Map<string, string> — aceita tanto id (`tiago-5`) quanto nome
// (`Tiago`, case-insensitive) e resolve pro id canônico. knownAgents é Set.
function ingestAgentChatRow(graph, agent, row, humanLookup, knownAgents) {
  if (!agent || !row || !row.ts || row.role !== 'user' || !row.from) return;
  const aId = nodeId('agent', agent);
  touchNode(graph, aId, 'agent', agent, row.ts);

  const humanId = humanLookup
    ? (humanLookup.get(row.from) || humanLookup.get(String(row.from).toLowerCase()))
    : null;

  if (humanId) {
    const hId = nodeId('human', humanId);
    touchNode(graph, hId, 'human', humanId, row.ts);
    touchEdge(graph, hId, aId, 'talks_to', row.ts);
  } else if (knownAgents && knownAgents.has(row.from)) {
    const fId = nodeId('agent', row.from);
    touchNode(graph, fId, 'agent', row.from, row.ts);
    touchEdge(graph, fId, aId, 'handoff', row.ts);
  }
}

// ─── Human chat rows ─────────────────────────────────────────────────────────
// Filename: <a>__<b>.jsonl (ordem alfabética em geral). Row: { ts, from, to, content }

function ingestHumanChatRow(graph, row) {
  if (!row || !row.ts || !row.from || !row.to) return;
  const fId = nodeId('human', row.from);
  const tId = nodeId('human', row.to);
  touchNode(graph, fId, 'human', row.from, row.ts);
  touchNode(graph, tId, 'human', row.to, row.ts);
  touchEdge(graph, fId, tId, 'talks_with', row.ts);
  touchEdge(graph, tId, fId, 'talks_with', row.ts);
}

// ─── Decisions ──────────────────────────────────────────────────────────────
// A decisão "toca" os arquivos editados na mesma session_id (todo o escopo da
// sessão). Sem session_id, cria só o nó.

function decisionId(d) {
  const what = (d.what || d.text || '').slice(0, 80).toLowerCase().replace(/[^a-z0-9]+/g, '-');
  return nodeId('decision', what || `anon-${(d.ts || '').slice(0, 19)}`);
}

function ingestDecision(graph, decision, sessionFileIds) {
  if (!decision || !decision.ts) return;
  const did = decisionId(decision);
  touchNode(graph, did, 'decision',
    (decision.what || decision.text || 'decisão').slice(0, 80), decision.ts);
  if (Array.isArray(sessionFileIds)) {
    for (const fId of sessionFileIds) {
      if (fId.startsWith('file:')) touchEdge(graph, did, fId, 'touches', decision.ts);
    }
  }
}

// ─── Gotchas ────────────────────────────────────────────────────────────────

function gotchaId(g) {
  const base = `${g.file || 'unknown'}|${(g.description || '').slice(0, 60)}`;
  const hash = base.toLowerCase().replace(/[^a-z0-9]+/g, '-').slice(0, 80);
  return nodeId('gotcha', hash || `anon-${(g.ts || '').slice(0, 19)}`);
}

function ingestGotcha(graph, gotcha) {
  if (!gotcha || !gotcha.ts) return;
  const gid = gotchaId(gotcha);
  const label = (gotcha.description || 'gotcha').slice(0, 80);
  touchNode(graph, gid, 'gotcha', label, gotcha.ts);
  if (gotcha.file) {
    const fId = nodeId('file', gotcha.file);
    touchNode(graph, fId, 'file', gotcha.file, gotcha.ts);
    touchEdge(graph, gid, fId, 'gotcha_for', gotcha.ts);
  }
}

// ─── Incremental public API (wire-in nos call-sites) ─────────────────────────

function updateGraph(memDir, current, prev) {
  const graph = loadGraph(memDir);
  ingestObservation(graph, current, prev);
  saveGraph(memDir, graph);
}

function onAgentLogEvent(memDir, agent, ev) {
  try {
    const graph = loadGraph(memDir);
    ingestAgentLogEvent(graph, agent, ev);
    saveGraph(memDir, graph);
  } catch {}
}

function buildHumanLookup(memDir) {
  const m = new Map();
  try {
    const humans = JSON.parse(fs.readFileSync(path.join(memDir, 'humans.json'), 'utf8'));
    if (humans && humans.humans) {
      for (const [id, h] of Object.entries(humans.humans)) {
        m.set(id, id);
        if (h && h.name) {
          m.set(h.name, id);
          m.set(String(h.name).toLowerCase(), id);
        }
      }
    }
  } catch {}
  return m;
}

function buildAgentSet(memDir) {
  const s = new Set();
  try {
    const manifest = JSON.parse(fs.readFileSync(path.join(memDir, 'agents.json'), 'utf8'));
    if (manifest && manifest.agents) {
      for (const name of Object.keys(manifest.agents)) s.add(name);
    }
  } catch {}
  return s;
}

function onAgentChatRow(memDir, agent, row) {
  try {
    const graph = loadGraph(memDir);
    const humanLookup = buildHumanLookup(memDir);
    const knownAgents = buildAgentSet(memDir);
    ingestAgentChatRow(graph, agent, row, humanLookup, knownAgents);
    saveGraph(memDir, graph);
  } catch {}
}

function onHumanChatRow(memDir, row) {
  try {
    const graph = loadGraph(memDir);
    ingestHumanChatRow(graph, row);
    saveGraph(memDir, graph);
  } catch {}
}

function onDecision(memDir, decision, sessionFiles) {
  try {
    const graph = loadGraph(memDir);
    ingestDecision(graph, decision, sessionFiles || []);
    saveGraph(memDir, graph);
  } catch {}
}

function onGotcha(memDir, gotcha) {
  try {
    const graph = loadGraph(memDir);
    ingestGotcha(graph, gotcha);
    saveGraph(memDir, graph);
  } catch {}
}

// ─── Context generation ──────────────────────────────────────────────────────

function topBy(list, scoreFn, n) {
  return list
    .map(item => ({ item, score: scoreFn(item) }))
    .filter(x => x.score > 0)
    .sort((a, b) => b.score - a.score)
    .slice(0, n)
    .map(x => x.item);
}

function shortFile(id) { return id.replace(/^file:/, '').split('/').pop(); }
function shortCmd(id)  { return id.replace(/^command:/, ''); }
function shortAgent(id){ return id.replace(/^agent:/,  ''); }
function shortHuman(id){ return id.replace(/^human:/,  ''); }

function getContextLines(graph) {
  const lines = [];
  if (!graph || !graph.nodes) return lines;
  const now = Date.now();
  const nodes = Object.values(graph.nodes);
  const edges = Object.values(graph.edges);
  if (nodes.length === 0) return lines;

  // ── Arquivos mais ativos (decayed) ────────────────────────────────────────
  const files = nodes.filter(n => n.kind === 'file' && n.count > 1);
  const topFiles = topBy(files, n => scoreOf(n, now), 8);
  if (topFiles.length > 0) {
    lines.push('### Arquivos mais ativos');
    for (const f of topFiles) lines.push(`- \`${f.label}\` — ${f.count}x`);
    lines.push('');
  }

  // ── Acoplamentos (co_edited, dedup A<B) ───────────────────────────────────
  const coEdges = edges
    .filter(e => e.rel === 'co_edited' && e.count > 1 && e.from < e.to);
  const topCo = topBy(coEdges, e => scoreOf(e, now), 6);
  if (topCo.length > 0) {
    lines.push('### Acoplamentos (editados juntos)');
    for (const e of topCo) {
      lines.push(`- \`${shortFile(e.from)}\` ↔ \`${shortFile(e.to)}\` (${e.count}x)`);
    }
    lines.push('');
  }

  // ── Workflows editar → rodar ──────────────────────────────────────────────
  const wf = edges.filter(e => e.rel === 'followed_by' && e.count > 1);
  const topWf = topBy(wf, e => scoreOf(e, now), 5);
  if (topWf.length > 0) {
    lines.push('### Workflows detectados (editar → rodar)');
    for (const e of topWf) {
      lines.push(`- editar \`${shortFile(e.from)}\` → \`${shortCmd(e.to)}\` (${e.count}x)`);
    }
    lines.push('');
  }

  // ── Topologia de agentes (handoff entre agentes) ──────────────────────────
  const handoffs = edges.filter(e => e.rel === 'handoff' && e.count > 1);
  const topHand = topBy(handoffs, e => scoreOf(e, now), 6);
  if (topHand.length > 0) {
    lines.push('### Fluxo entre agentes (handoffs)');
    for (const e of topHand) {
      let tail = `${e.count}x`;
      if (e.intents) {
        const parts = Object.entries(e.intents)
          .sort((a, b) => b[1] - a[1])
          .map(([k, v]) => `${v} ${k}`);
        tail = parts.join(', ');
      }
      lines.push(`- \`${shortAgent(e.from)}\` → \`${shortAgent(e.to)}\` (${tail})`);
    }
    lines.push('');
  }

  // ── Especialização de agente (quais arquivos o agente mais toca) ─────────
  const edits = edges.filter(e => e.rel === 'edits' && e.count > 0);
  if (edits.length > 0) {
    const byAgent = new Map();
    for (const e of edits) {
      const a = e.from;
      if (!byAgent.has(a)) byAgent.set(a, []);
      byAgent.get(a).push(e);
    }
    const agentLines = [];
    for (const [a, evs] of byAgent) {
      const top3 = topBy(evs, e => scoreOf(e, now), 3);
      if (top3.length === 0) continue;
      const files = top3.map(e => `\`${shortFile(e.to)}\``).join(', ');
      agentLines.push({ agent: a, line: `- \`${shortAgent(a)}\` ↔ ${files}`,
                        score: top3.reduce((s, e) => s + scoreOf(e, now), 0) });
    }
    const topAgents = agentLines.sort((x, y) => y.score - x.score).slice(0, 4);
    if (topAgents.length > 0) {
      lines.push('### Especialização de agentes');
      for (const x of topAgents) lines.push(x.line);
      lines.push('');
    }
  }

  // ── Gotchas ativas (gotcha → file) ────────────────────────────────────────
  const gotchas = edges.filter(e => e.rel === 'gotcha_for');
  const recentGotchas = topBy(gotchas, e => scoreOf(e, now), 5);
  if (recentGotchas.length > 0) {
    lines.push('### Armadilhas ativas');
    for (const e of recentGotchas) {
      const g = graph.nodes[e.from];
      const label = g && g.label ? g.label : '(sem descrição)';
      lines.push(`- \`${shortFile(e.to)}\`: ${label}`);
    }
    lines.push('');
  }

  return lines;
}

// ─── Rebuild total ───────────────────────────────────────────────────────────

function rebuildGraph(memDir, readJsonl) {
  const graph = empty();

  // 1) Observações — file/bash/agent_call + co-ocorrência
  const obsFile = path.join(memDir, 'observations.jsonl');
  const observations = readJsonl(obsFile);
  for (let i = 0; i < observations.length; i++) {
    const prev = i > 0 ? observations[i - 1] : null;
    ingestObservation(graph, observations[i], prev);
  }

  // 1b) Indexar file-edits por session_id pra linkar decisions depois
  const filesBySession = new Map();
  for (const o of observations) {
    if (o.type === 'file_edit' && o.session_id && o.file) {
      if (!filesBySession.has(o.session_id)) filesBySession.set(o.session_id, new Set());
      filesBySession.get(o.session_id).add(nodeId('file', o.file));
    }
  }

  // 2) Agent logs — handoffs, violations, escalations
  const logsDir = path.join(memDir, 'agent_logs');
  if (fs.existsSync(logsDir)) {
    for (const f of fs.readdirSync(logsDir)) {
      if (!f.endsWith('.jsonl')) continue;
      const agent = f.replace(/\.jsonl$/, '');
      const rows = readJsonl(path.join(logsDir, f));
      for (const ev of rows) ingestAgentLogEvent(graph, agent, ev);
    }
  }

  // 3) Agent chats — quem fala com quem (human → agent, agent → agent)
  const chatsDir    = path.join(memDir, 'agent_chats');
  const knownAgents = buildAgentSet(memDir);
  const humanLookup = buildHumanLookup(memDir);

  if (fs.existsSync(chatsDir)) {
    for (const f of fs.readdirSync(chatsDir)) {
      if (!f.endsWith('.jsonl')) continue;
      const agent = f.replace(/\.jsonl$/, '');
      const rows = readJsonl(path.join(chatsDir, f));
      for (const row of rows) ingestAgentChatRow(graph, agent, row, humanLookup, knownAgents);
    }
  }

  // 4) Human chats — humano ↔ humano
  const humanChatsDir = path.join(memDir, 'human_chats');
  if (fs.existsSync(humanChatsDir)) {
    for (const f of fs.readdirSync(humanChatsDir)) {
      if (!f.endsWith('.jsonl')) continue;
      const rows = readJsonl(path.join(humanChatsDir, f));
      for (const row of rows) ingestHumanChatRow(graph, row);
    }
  }

  // 5) Decisions — liga à sessão (arquivos tocados na mesma session_id)
  const decisions = readJsonl(path.join(memDir, 'decisions.jsonl'));
  for (const d of decisions) {
    const sessionFiles = d.session_id && filesBySession.has(d.session_id)
      ? Array.from(filesBySession.get(d.session_id))
      : [];
    ingestDecision(graph, d, sessionFiles);
  }

  // 6) Gotchas — direct link to file
  const gotchas = readJsonl(path.join(memDir, 'gotchas.jsonl'));
  for (const g of gotchas) ingestGotcha(graph, g);

  graph.rebuilt_at = new Date().toISOString();
  saveGraph(memDir, graph);
  return graph;
}

// ─── CLI (rebuild sob demanda) ───────────────────────────────────────────────

if (require.main === module) {
  const sub = process.argv[2];
  const cwd = process.cwd();
  const { findProjectRoot, getMemDir, readJsonl } = require('./lib.cjs');
  const memDir = getMemDir(cwd) || path.join(findProjectRoot(cwd), '.claude-project-memory');
  if (sub === 'rebuild') {
    const g = rebuildGraph(memDir, readJsonl);
    const nodes = Object.values(g.nodes);
    const edges = Object.values(g.edges);
    const byKind = nodes.reduce((a, n) => (a[n.kind] = (a[n.kind] || 0) + 1, a), {});
    const byRel  = edges.reduce((a, e) => (a[e.rel]  = (a[e.rel]  || 0) + 1, a), {});
    console.log('rebuilt', memDir);
    console.log('  nodes:', nodes.length, byKind);
    console.log('  edges:', edges.length, byRel);
  } else if (sub === 'context') {
    const g = loadGraph(memDir);
    console.log(getContextLines(g).join('\n'));
  } else {
    console.error('uso: node graph.cjs <rebuild|context>');
    process.exit(1);
  }
}

module.exports = {
  loadGraph, saveGraph, rebuildGraph,
  updateGraph,            // observe.cjs
  onAgentLogEvent,        // server.cjs — appendAgentLog sites
  onAgentChatRow,         // server.cjs — appendChat sites
  onHumanChatRow,         // server.cjs — appendHumanChat sites
  onDecision, onGotcha,   // nudge.cjs + /api/mem/decision
  getContextLines,
  VERSION,
};
