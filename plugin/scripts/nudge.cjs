'use strict';

// Proactive memory nudge for autonomous agent runs.
//
// Quando um agente executa sem humano presente (auto-handoff, triage de
// firefighter, chat em modo high-autonomy sem human_id), disparamos uma segunda
// chamada barata ao claude -p perguntando: "você aprendeu algo que vale
// persistir?". A resposta vem como JSON estruturado e é roteada pra
// decisions.jsonl ou gotchas.jsonl (com re-render do MD).
//
// Não é chamado em turnos humanos — a curadoria on-demand já existe via
// /mem-decision. O nudge cobre o gap dos agentes que rodam e morrem.

const fs   = require('fs');
const path = require('path');
const { spawn } = require('child_process');

const NUDGE_STATE_FILE = 'nudge_state.json';
const MIN_GAP_MS       = 60 * 60 * 1000;  // 1h mínimo entre nudges/agente
const MIN_RESPONSE_LEN = 120;              // respostas curtas = nada pra extrair
const NUDGE_TIMEOUT_MS = 60_000;
const NUDGE_MODEL      = 'claude-haiku-4-5-20251001';

const inFlight = new Set();

// ── Estado (rate-limit por agente) ───────────────────────────────────────────

function statePath(memDir) {
  return path.join(memDir, NUDGE_STATE_FILE);
}

function loadState(memDir) {
  try { return JSON.parse(fs.readFileSync(statePath(memDir), 'utf8')); }
  catch { return {}; }
}

function saveState(memDir, state) {
  try { fs.writeFileSync(statePath(memDir), JSON.stringify(state, null, 2) + '\n'); }
  catch {}
}

function withinCooldown(state, agent, now = Date.now()) {
  const s = state[agent];
  if (!s || !s.last_ts) return false;
  const last = new Date(s.last_ts).getTime();
  if (Number.isNaN(last)) return false;
  return (now - last) < MIN_GAP_MS;
}

// ── Re-render dos markdowns a partir dos JSONLs ──────────────────────────────

function readJsonl(file) {
  if (!fs.existsSync(file)) return [];
  return fs.readFileSync(file, 'utf8').split('\n').filter(Boolean)
    .map(l => { try { return JSON.parse(l); } catch { return null; } })
    .filter(Boolean);
}

function renderDecisionsMd(memDir) {
  const jsonl = path.join(memDir, 'decisions.jsonl');
  const md    = path.join(memDir, 'DECISIONS.md');
  const decisions = readJsonl(jsonl).reverse();
  if (!decisions.length) return;
  const out = ['# Decisões do projeto', ''];
  for (const d of decisions) {
    const date = (d.ts || '').slice(0, 10);
    out.push(`## ${date} — ${d.what || d.text || '(sem título)'}`);
    if (d.why) { out.push(''); out.push(d.why); }
    out.push('');
  }
  try { fs.writeFileSync(md, out.join('\n')); } catch {}
}

function renderGotchasMd(memDir) {
  const jsonl = path.join(memDir, 'gotchas.jsonl');
  const md    = path.join(memDir, 'GOTCHAS.md');
  const gotchas = readJsonl(jsonl);
  if (!gotchas.length) return;
  const seen = new Set();
  const unique = [];
  for (const g of gotchas.slice().reverse()) {
    const key = `${g.file}|${(g.description || '').slice(0, 60)}`;
    if (seen.has(key)) continue;
    seen.add(key);
    unique.push(g);
  }
  if (!unique.length) return;
  const out = ['# Armadilhas conhecidas', ''];
  for (const g of unique) {
    const date = (g.ts || '').slice(0, 10);
    out.push(`## ${date} — \`${g.file || '(sem arquivo)'}\``);
    out.push('');
    out.push(g.description || '');
    if (g.failure_hint) { out.push(''); out.push(`_Sintoma:_ ${g.failure_hint}`); }
    out.push('');
  }
  try { fs.writeFileSync(md, out.join('\n')); } catch {}
}

// ── Prompt + extração ────────────────────────────────────────────────────────

function buildNudgePrompt(agent, responseText, trigger) {
  const trimmed = String(responseText || '').slice(0, 2000);
  return `Você é o agente **${agent}**. Acabou de executar uma tarefa em modo autônomo (trigger: ${trigger}). Sua resposta foi registrada no canal do agente.

## Sua resposta
${trimmed}

## Sua tarefa agora
Olhe SÓ pra sua resposta acima. Você identificou algo concreto que vale persistir na memória do projeto?

Responda EXCLUSIVAMENTE com UM JSON em UMA linha. Sem markdown. Sem comentário. Sem texto antes nem depois:

- \`{"kind":"none"}\` — rotina, nada significativo, sem material pra promover.
- \`{"kind":"decision","what":"<o que foi decidido>","why":"<por quê, 1-2 frases>"}\` — decisão técnica do seu escopo (com motivação), digna de DECISIONS.md.
- \`{"kind":"gotcha","file":"<arquivo ou área>","description":"<bug + conserto em 1-2 frases>","failure_hint":"<sintoma, opcional>"}\` — armadilha concreta (bug encontrado + correção aplicada).

Critérios:
- Decisão: específica + com motivo real. "Fiz X" sem por quê → none.
- Gotcha: erro concreto com correção. Frustração genérica → none.
- Em dúvida → none. Melhor perder sinal fraco do que poluir a memória.

Responda só o JSON.`;
}

function extractJson(text) {
  if (!text) return null;
  const match = String(text).match(/\{[\s\S]*\}/);
  if (!match) return null;
  try { return JSON.parse(match[0]); } catch { return null; }
}

// ── Spawn ─────────────────────────────────────────────────────────────────────

function runNudgeClaude({ cwd, prompt }) {
  return new Promise((resolve) => {
    const args = ['-p', '--output-format', 'text', '--model', NUDGE_MODEL, prompt];
    let proc;
    try {
      proc = spawn('claude', args, {
        cwd,
        stdio: ['ignore', 'pipe', 'pipe'],
        env: { ...process.env, CLAUDE_PROJECT_MEM_NUDGE: '1' },
      });
    } catch (err) {
      return resolve({ code: -1, out: '', err: err.message });
    }
    let out = '', err = '';
    const timer = setTimeout(() => { try { proc.kill('SIGTERM'); } catch {} }, NUDGE_TIMEOUT_MS);
    proc.stdout.on('data', c => out += c.toString());
    proc.stderr.on('data', c => err += c.toString());
    proc.on('close', code => { clearTimeout(timer); resolve({ code, out, err }); });
    proc.on('error', e => { clearTimeout(timer); resolve({ code: -1, out, err: e.message }); });
  });
}

// ── Persist ──────────────────────────────────────────────────────────────────

function persistDecision(memDir, agent, parsed, trigger) {
  const what = (parsed.what || '').trim();
  const why  = (parsed.why  || '').trim();
  if (!what) return null;
  const entry = {
    what, why,
    ts: new Date().toISOString(),
    session_id: `nudge:${agent}`,
    source: 'nudge',
    agent,
    trigger,
  };
  const file = path.join(memDir, 'decisions.jsonl');
  try {
    fs.appendFileSync(file, JSON.stringify(entry) + '\n');
    renderDecisionsMd(memDir);
  } catch (e) { return { err: e.message }; }
  return { entry };
}

function persistGotcha(memDir, agent, parsed, trigger) {
  const description = (parsed.description || '').trim();
  if (!description) return null;
  const entry = {
    file: (parsed.file || '').trim() || null,
    description,
    failure_hint: (parsed.failure_hint || '').trim() || null,
    ts: new Date().toISOString(),
    source: 'nudge',
    agent,
    trigger,
  };
  const file = path.join(memDir, 'gotchas.jsonl');
  try {
    fs.appendFileSync(file, JSON.stringify(entry) + '\n');
    renderGotchasMd(memDir);
  } catch (e) { return { err: e.message }; }
  return { entry };
}

function appendObservation(memDir, obs) {
  try {
    fs.appendFileSync(path.join(memDir, 'observations.jsonl'), JSON.stringify(obs) + '\n');
  } catch {}
}

// ── Entry point ──────────────────────────────────────────────────────────────

// fire-and-forget — chamador NÃO deve esperar. Rejeita silenciosamente em
// casos de cooldown, resposta curta, erro de parse. Sempre grava observation
// quando spawn é disparado, com outcome (none/decision/gotcha/parse_error).
function scheduleNudge({ memDir, projectRoot, agent, agentCfg, responseText, trigger }) {
  if (!memDir || !agent || !agentCfg) return;
  if (process.env.CLAUDE_PROJECT_MEM_NUDGE === '1') return; // no recursion
  if (!responseText || String(responseText).length < MIN_RESPONSE_LEN) return;
  if (inFlight.has(agent)) return;

  const state = loadState(memDir);
  if (withinCooldown(state, agent)) return;

  inFlight.add(agent);
  // Reserva cooldown imediatamente pra prevenir corrida em múltiplos triggers
  // próximos — se a chamada falhar, o cooldown expira em MIN_GAP_MS e libera.
  state[agent] = { last_ts: new Date().toISOString(), trigger };
  saveState(memDir, state);

  (async () => {
    const startTs = new Date().toISOString();
    const started = Date.now();
    const prompt = buildNudgePrompt(agent, responseText, trigger);
    const { code, out, err } = await runNudgeClaude({
      cwd: projectRoot || process.cwd(),
      prompt,
    });
    const duration_ms = Date.now() - started;
    const parsed = extractJson(out);
    const kind = parsed?.kind === 'decision' || parsed?.kind === 'gotcha' ? parsed.kind : 'none';

    let persisted = null;
    if (kind === 'decision') persisted = persistDecision(memDir, agent, parsed, trigger);
    else if (kind === 'gotcha') persisted = persistGotcha(memDir, agent, parsed, trigger);

    const outcome =
      code !== 0 ? 'error' :
      !parsed ? 'parse_error' :
      kind === 'none' ? 'none' :
      persisted?.entry ? kind :
      'empty';

    appendObservation(memDir, {
      type: 'nudge',
      ts: new Date().toISOString(),
      agent,
      trigger,
      outcome,
      what: persisted?.entry?.what || persisted?.entry?.description || null,
      duration_ms,
    });

    try {
      const logDir = path.join(memDir, 'agent_logs');
      if (!fs.existsSync(logDir)) fs.mkdirSync(logDir, { recursive: true });
      fs.appendFileSync(path.join(logDir, `${agent}.jsonl`), JSON.stringify({
        ts: new Date().toISOString(),
        type: 'nudge',
        stage: 'post_autonomous',
        trigger,
        outcome,
        duration_ms,
        code,
        start_ts: startTs,
        stderr: code !== 0 ? (err || '').slice(0, 200) : undefined,
      }) + '\n');
    } catch {}
  })().finally(() => {
    inFlight.delete(agent);
  });
}

module.exports = {
  scheduleNudge,
  renderDecisionsMd,
  renderGotchasMd,
  MIN_GAP_MS,
  MIN_RESPONSE_LEN,
};
