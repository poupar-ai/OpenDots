'use strict';

// Roteador determinístico de handoff entre agentes.
//
// Responde: "dado que o agente `from` quer acionar alguém sobre `file`/`topic`,
// quem é o melhor destino dentro da sua `can_talk_to`?" — usando o grafo como
// fonte de sinal (quem editou mais o arquivo, quem mais recebeu delegate sobre
// algo similar, quem é mais ativo).
//
// Quando não há sinal forte, cai pra ordem natural de `can_talk_to`. Nunca
// inventa destinos fora dessa lista — o endpoint de handoff já rejeita, mas
// aqui filtramos antes pra não desperdiçar tentativa.

const path = require('path');
const graph = require('./graph.cjs');
const agents = require('./agents.cjs');

const HALF_LIFE_DAYS = 30;
const HALF_LIFE_MS   = HALF_LIFE_DAYS * 864e5;

function decayScore(entity, now = Date.now()) {
  if (!entity || !entity.last_seen) return entity?.count || 0;
  const age = now - new Date(entity.last_seen).getTime();
  if (!Number.isFinite(age) || age < 0) return entity.count;
  return entity.count * Math.exp(-age / HALF_LIFE_MS);
}

// Lê todos os sinais do grafo que ligam arquivo → agente.
// Fontes: arestas `edits` (agent → file via agent_id em observações).
function agentsForFile(g, file) {
  const results = new Map();
  if (!g || !g.edges || !file) return results;
  const targetId = `file:${file}`;
  const now = Date.now();
  for (const e of Object.values(g.edges)) {
    if (e.rel !== 'edits' || e.to !== targetId) continue;
    const agent = e.from.replace(/^agent:/, '');
    const score = decayScore(e, now);
    results.set(agent, (results.get(agent) || 0) + score);
  }
  return results;
}

// Atividade recente do agente = soma decay dos `handoff`/`edits`/`runs` incidentes.
function agentActivity(g, agent) {
  if (!g || !g.edges) return 0;
  const id = `agent:${agent}`;
  const now = Date.now();
  let sum = 0;
  for (const e of Object.values(g.edges)) {
    if (e.to !== id && e.from !== id) continue;
    if (e.rel === 'edits' || e.rel === 'runs' || e.rel === 'handoff') {
      sum += decayScore(e, now);
    }
  }
  return sum;
}

// Score principal: 2× sinal de arquivo + 0.2× atividade geral (tie-breaker).
function scoreCandidate(g, agent, fileSignals) {
  const fileScore = fileSignals.get(agent) || 0;
  const activity  = agentActivity(g, agent);
  return fileScore * 2 + activity * 0.2;
}

function buildReason(g, agent, fileSignals, file) {
  if (file && fileSignals.get(agent) > 0) {
    const edits = Object.values(g.edges).find(
      e => e.rel === 'edits' && e.from === `agent:${agent}` && e.to === `file:${file}`
    );
    const n = edits ? edits.count : 0;
    return `editou \`${file}\` ${n}x`;
  }
  const activity = agentActivity(g, agent);
  if (activity > 0) return `ativo recentemente`;
  return 'na sua can_talk_to';
}

/**
 * route({ memDir, from, file?, topic?, intent? }) →
 *   { candidates: [{ agent, score, reason, in_can_talk_to: true }], can_talk_to }
 *
 * - file: string — arquivo sobre o qual `from` quer conversar.
 * - topic: string — texto livre (usado só como último recurso heurístico).
 * - intent: 'ask'|'delegate'|'report'|'ack'|'escalate' — opcional, altera ranking.
 *     'escalate' prioriza firefighter do manifest se estiver em can_talk_to.
 */
function route({ memDir, from, file, topic, intent }) {
  const manifest = agents.loadManifest(memDir);
  if (!manifest || !manifest.agents || !manifest.agents[from]) {
    return { error: `agent "${from}" not found`, candidates: [] };
  }
  const cfg = manifest.agents[from];
  const canTalk = Array.isArray(cfg.can_talk_to) ? cfg.can_talk_to : [];
  if (canTalk.length === 0) {
    return { candidates: [], can_talk_to: [], note: 'sem can_talk_to configurado' };
  }

  const g = graph.loadGraph(memDir);
  const fileSignals = file ? agentsForFile(g, file) : new Map();

  // Escalate: priorize firefighter/internal_firefighter se permitido
  let prioritized = [...canTalk];
  if (intent === 'escalate') {
    const firefighters = prioritized.filter(a => /firefighter/i.test(a));
    const rest = prioritized.filter(a => !firefighters.includes(a));
    prioritized = [...firefighters, ...rest];
  }

  const candidates = prioritized.map((agent, idx) => {
    const base = scoreCandidate(g, agent, fileSignals);
    const canTalkBonus = (prioritized.length - idx) * 0.01; // preserva ordem original como tie-break
    return {
      agent,
      score: base + canTalkBonus,
      reason: buildReason(g, agent, fileSignals, file),
      in_can_talk_to: true,
    };
  }).sort((a, b) => b.score - a.score);

  return { candidates, can_talk_to: canTalk };
}

/**
 * suggestTargets({ memDir, from, message }) — quando um handoff foi negado,
 * sugere quais agentes DENTRO de can_talk_to são melhores pro conteúdo da
 * mensagem. Heurística simples: se a mensagem menciona um arquivo conhecido,
 * usa esse file. Senão, ranking por atividade geral.
 */
function suggestTargets({ memDir, from, message }) {
  const match = (message || '').match(/`([^`\n]+)`|\b([a-zA-Z0-9_\-./]+\.(py|ts|tsx|js|cjs|rs|md|json|yaml|yml|sql))\b/);
  const file = match ? (match[1] || match[2]) : null;
  const { candidates } = route({ memDir, from, file });
  return { file_detected: file, candidates: candidates.slice(0, 3) };
}

module.exports = { route, suggestTargets };
