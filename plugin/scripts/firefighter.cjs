'use strict';

// Poller de bombeiros. Lê o manifest, consulta fontes (Datadog), avalia
// threshold, dispara triage via `claude -p` com o mini-contexto do agente.
//
// Uso direto:
//   node scripts/firefighter.cjs watch              # loop contínuo
//   node scripts/firefighter.cjs watch --once       # uma iteração só
//   node scripts/firefighter.cjs watch --interval 60
//   node scripts/firefighter.cjs watch --agent <nome>   # roda só um
//   node scripts/firefighter.cjs watch --dry-run    # não dispara claude, só reporta
//
// Lê creds do ambiente (DD_API_KEY, DD_APP_KEY, DD_SITE). Nunca guarda valores.

const fs      = require('fs');
const path    = require('path');
const https   = require('https');
const { spawn } = require('child_process');
const { scheduleNudge } = require('./nudge.cjs');
const { findProjectRoot, getMemDir } = require('./lib.cjs');
const agents  = require('./agents.cjs');

const STATE_FILE = 'firefighter_state.json';

// ── State (cooldown + rate limit) ──────────────────────────────────────────────

function stateFile(memDir) { return path.join(memDir, STATE_FILE); }

function loadState(memDir) {
  const p = stateFile(memDir);
  if (!fs.existsSync(p)) return {};
  try { return JSON.parse(fs.readFileSync(p, 'utf8')); } catch { return {}; }
}

function saveState(memDir, state) {
  fs.writeFileSync(stateFile(memDir), JSON.stringify(state, null, 2) + '\n');
}

function pruneOldTriages(entries) {
  const cutoff = Date.now() - 3600_000;
  return (entries || []).filter(e => new Date(e.ts).getTime() > cutoff);
}

// ── Datadog Logs Search ────────────────────────────────────────────────────────

function ddRequest({ site, apiKey, appKey }, body) {
  const host = site || 'datadoghq.com';
  const postData = JSON.stringify(body);
  const options = {
    hostname: `api.${host}`,
    port: 443,
    path: '/api/v2/logs/events/search',
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'DD-API-KEY': apiKey,
      'DD-APPLICATION-KEY': appKey,
      'Content-Length': Buffer.byteLength(postData)
    }
  };
  return new Promise((resolve, reject) => {
    const req = https.request(options, res => {
      let data = '';
      res.on('data', c => data += c);
      res.on('end', () => {
        if (res.statusCode >= 200 && res.statusCode < 300) {
          try { resolve(JSON.parse(data)); } catch (e) { reject(e); }
        } else {
          reject(new Error(`DD ${res.statusCode}: ${data.slice(0, 300)}`));
        }
      });
    });
    req.on('error', reject);
    req.write(postData);
    req.end();
  });
}

async function fetchDatadogLogs(watch) {
  const apiKey = process.env.DD_API_KEY;
  const appKey = process.env.DD_APP_KEY;
  const site   = process.env.DD_SITE || 'datadoghq.com';
  if (!apiKey || !appKey) {
    throw new Error('DD_API_KEY / DD_APP_KEY ausentes no ambiente');
  }
  const data = await ddRequest({ site, apiKey, appKey }, {
    filter: {
      query: watch.query,
      from:  `now-${watch.window_minutes}m`,
      to:    'now'
    },
    page: { limit: 50 },
    sort: '-timestamp'
  });
  return (data && data.data) || [];
}

// ── Observações ────────────────────────────────────────────────────────────────

function appendObservation(memDir, obs) {
  const p = path.join(memDir, 'observations.jsonl');
  fs.appendFileSync(p, JSON.stringify(obs) + '\n');
}

// ── Triage (spawn claude -p) ───────────────────────────────────────────────────

function buildTriagePrompt(agentName, agentCfg, miniContext, logs) {
  const summary = logs.slice(0, 10).map((l, i) => {
    const attrs = (l.attributes && l.attributes.attributes) || {};
    const msg   = (l.attributes && l.attributes.message) || JSON.stringify(attrs).slice(0, 200);
    const svc   = attrs.service || '?';
    const host  = attrs.host    || '?';
    const ts    = (l.attributes && l.attributes.timestamp) || '?';
    return `[${i+1}] ${ts} service=${svc} host=${host}\n    ${msg.slice(0, 400)}`;
  }).join('\n');

  return `Você é o bombeiro **${agentName}**. Incêndio detectado:
- Query: \`${agentCfg.watch.query}\`
- Eventos encontrados: ${logs.length} (threshold era ${agentCfg.watch.threshold})
- Janela: últimos ${agentCfg.watch.window_minutes} minutos

## Seu runbook (mini-contexto)
${miniContext}

## Amostra de logs (até 10)
${summary || '_(sem detalhes)_'}

## Sua tarefa
1. Classifique a severidade (baixa / média / alta / crítica).
2. Hipótese de causa raiz (1-3 frases).
3. Próximo passo sugerido (investigar X, rodar Y, consultar agente Z).
4. Se achar que precisa de ação (motor), diga qual — mas **nunca execute**. Apenas escale ao Manager.

Seja direto. Português. Máximo 12 linhas.`;
}

function runClaude({ cwd, model, prompt, timeoutMs = 120_000 }) {
  return new Promise((resolve) => {
    const args = ['-p', '--output-format', 'text'];
    const resolvedModel = agents.resolveCliModel(model);
    if (resolvedModel) args.push('--model', resolvedModel);
    args.push(prompt);

    const proc = spawn('claude', args, { cwd, stdio: ['ignore', 'pipe', 'pipe'] });
    let out = '', err = '';
    const timer = setTimeout(() => { try { proc.kill('SIGTERM'); } catch {} }, timeoutMs);
    proc.stdout.on('data', c => out += c.toString());
    proc.stderr.on('data', c => err += c.toString());
    proc.on('close', code => {
      clearTimeout(timer);
      resolve({ code, out, err });
    });
    proc.on('error', e => { clearTimeout(timer); resolve({ code: -1, out, err: e.message }); });
  });
}

// ── Loop principal ─────────────────────────────────────────────────────────────

async function checkAgent({ memDir, projectRoot, name, agentCfg, state, dryRun }) {
  const w = agentCfg.watch;
  if (!w || !w.query) return { skipped: 'sem watch config' };

  const now = new Date();
  const s = state[name] = state[name] || { last_triage_at: null, triages_last_hour: [] };
  s.triages_last_hour = pruneOldTriages(s.triages_last_hour);

  if (s.last_triage_at) {
    const sinceMs = now.getTime() - new Date(s.last_triage_at).getTime();
    if (sinceMs < w.cooldown_seconds * 1000) {
      return { cooldown: Math.ceil((w.cooldown_seconds * 1000 - sinceMs) / 1000) + 's' };
    }
  }

  if (s.triages_last_hour.length >= w.max_triages_per_hour) {
    return { rate_limited: `${s.triages_last_hour.length}/${w.max_triages_per_hour} na última hora` };
  }

  let logs = [];
  try {
    logs = await fetchDatadogLogs(w);
  } catch (err) {
    return { error: err.message };
  }

  if (logs.length < w.threshold) {
    return { ok: `${logs.length}/${w.threshold} eventos` };
  }

  // Fogo! Dispara triage.
  const miniContext = agents.loadMiniContext(memDir, agentCfg.context_file) || '';
  const prompt = buildTriagePrompt(name, agentCfg, miniContext, logs);

  if (dryRun) {
    return { would_triage: true, events: logs.length, prompt_chars: prompt.length };
  }

  const { code, out, err } = await runClaude({
    cwd: projectRoot,
    model: agentCfg.model,
    prompt
  });

  const ts = new Date().toISOString();
  s.last_triage_at = ts;
  s.triages_last_hour.push({ ts, events: logs.length });

  appendObservation(memDir, {
    type:      'firefighter_triage',
    ts,
    agent:     name,
    query:     w.query,
    events:    logs.length,
    threshold: w.threshold,
    diagnosis: out.trim(),
    exit_code: code,
    stderr:    code !== 0 ? err.slice(0, 300) : undefined
  });

  if (code === 0 && out.trim()) {
    scheduleNudge({
      memDir, projectRoot,
      agent: name, agentCfg,
      responseText: out,
      trigger: 'firefighter_triage',
    });
  }

  return { triaged: true, events: logs.length, exit_code: code };
}

async function pollOnce({ memDir, projectRoot, agentName, dryRun }) {
  const manifest = agents.loadManifest(memDir);
  if (!manifest || !manifest.agents) return { error: 'sem manifest' };

  const state = loadState(memDir);
  const results = {};

  for (const [name, cfg] of Object.entries(manifest.agents)) {
    if (cfg.type !== 'firefighter') continue;
    if (agentName && name !== agentName) continue;
    try {
      results[name] = await checkAgent({ memDir, projectRoot, name, agentCfg: cfg, state, dryRun });
    } catch (err) {
      results[name] = { error: err.message };
    }
  }

  saveState(memDir, state);
  return results;
}

async function watch({ memDir, projectRoot, interval = 60, once = false, agentName, dryRun }) {
  const tick = async () => {
    const ts = new Date().toISOString().slice(0, 19).replace('T', ' ');
    const r = await pollOnce({ memDir, projectRoot, agentName, dryRun });
    console.log(`[${ts}]`, JSON.stringify(r));
  };

  await tick();
  if (once) return;

  const timer = setInterval(tick, interval * 1000);
  const stop = () => { clearInterval(timer); console.log('\nparando…'); process.exit(0); };
  process.on('SIGINT',  stop);
  process.on('SIGTERM', stop);
}

// ── CLI ────────────────────────────────────────────────────────────────────────

function parseArgs(argv) {
  const args = { interval: 60, once: false, dryRun: false, agentName: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--once')     args.once    = true;
    else if (a === '--dry-run') args.dryRun = true;
    else if (a === '--interval') args.interval = parseInt(argv[++i], 10) || 60;
    else if (a === '--agent')    args.agentName = argv[++i];
  }
  return args;
}

if (require.main === module) {
  const sub = process.argv[2];
  if (sub !== 'watch') {
    console.error('Uso: node firefighter.cjs watch [--once] [--interval <s>] [--agent <nome>] [--dry-run]');
    process.exit(1);
  }
  const args = parseArgs(process.argv.slice(3));
  const cwd = process.cwd();
  const projectRoot = findProjectRoot(cwd);
  const memDir      = getMemDir(cwd);
  console.log(`bombeiros em watch · project=${path.basename(projectRoot)} · interval=${args.interval}s · dry=${args.dryRun}`);
  watch({ memDir, projectRoot, ...args }).catch(err => {
    console.error('erro fatal:', err.message);
    process.exit(1);
  });
}

module.exports = {
  pollOnce,
  checkAgent,
  loadState,
  saveState,
  fetchDatadogLogs
};
