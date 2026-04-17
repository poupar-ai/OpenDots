'use strict';

const http = require('http');
const fs   = require('fs');
const path = require('path');
const { spawn } = require('child_process');
const { findProjectRoot, getMemDir, readJsonl } = require('../scripts/lib.cjs');
const agents = require('../scripts/agents.cjs');

const PORT        = parseInt(process.env.PROJECT_MEM_PORT || '37778', 10);
const cwd         = process.cwd();
const projectRoot = findProjectRoot(cwd);
const memDir      = getMemDir(cwd);
const projectName = path.basename(projectRoot);
const HTML_PATH   = path.join(__dirname, 'index.html');

function readMdIfExists(file) {
  const p = path.join(memDir, file);
  return fs.existsSync(p) ? fs.readFileSync(p, 'utf8') : null;
}

function getData() {
  const sessionsRaw  = readJsonl(path.join(memDir, 'sessions.jsonl'));
  const observations = readJsonl(path.join(memDir, 'observations.jsonl'));
  const decisions    = readJsonl(path.join(memDir, 'decisions.jsonl'));
  const corrections  = readJsonl(path.join(memDir, 'corrections.jsonl'));
  const gotchas      = readJsonl(path.join(memDir, 'gotchas.jsonl'));

  const soulMd      = readMdIfExists('SOUL.md');
  const projectMd   = readMdIfExists('PROJECT.md');
  const decisionsMd = readMdIfExists('DECISIONS.md');
  const gotchasMd   = readMdIfExists('GOTCHAS.md');
  const glossaryMd  = readMdIfExists('GLOSSARY.md');
  const codeIndexMd = readMdIfExists('CODE_INDEX.md');

  let codeIndex = null;
  try {
    const idxFile = path.join(memDir, 'code_index.json');
    if (fs.existsSync(idxFile)) codeIndex = JSON.parse(fs.readFileSync(idxFile, 'utf8'));
  } catch {}

  let stats = null;
  try {
    const statsFile = path.join(memDir, 'stats.json');
    if (fs.existsSync(statsFile)) stats = JSON.parse(fs.readFileSync(statsFile, 'utf8'));
  } catch {}

  const sessionMap = {};
  for (const s of sessionsRaw) {
    if (!sessionMap[s.session_id]) sessionMap[s.session_id] = {};
    Object.assign(sessionMap[s.session_id], s);
  }
  const sessions = Object.values(sessionMap)
    .filter(s => s.ended_at)
    .sort((a, b) => (b.ended_at || '').localeCompare(a.ended_at || ''));

  const codeIndexStats = codeIndex ? {
    files: Object.keys(codeIndex.files || {}).length,
    symbols: Object.values(codeIndex.files || {}).reduce((n, f) => n + (f.symbols || []).length, 0),
    generated_at: codeIndex.generated_at
  } : null;

  // Agents: manifest + mini-contexts + invocation counts
  let agentsData = null;
  try {
    const manifest = agents.loadManifest(memDir);
    if (manifest) {
      const { counts = {}, recent = {} } = agents.countInvocations(memDir);
      const contexts = {};
      for (const [name, a] of Object.entries(manifest.agents || {})) {
        contexts[name] = agents.loadMiniContext(memDir, a.context_file) || '';
      }
      agentsData = { manifest, contexts, counts, recent };
    }
  } catch {}

  return {
    project:      { name: projectName, root: projectRoot },
    soulMd,
    projectMd,
    decisionsMd,
    gotchasMd,
    glossaryMd,
    codeIndexMd,
    agents:       agentsData,
    sessions,
    observations: [...observations].reverse(),
    decisions:    [...decisions].reverse(),
    corrections:  [...corrections].reverse(),
    gotchas:      [...gotchas].reverse(),
    stats: {
      sessions:     sessions.length,
      observations: observations.length,
      decisions:    decisions.length,
      corrections:  corrections.length,
      gotchas:      gotchas.length,
      hasSoul:      !!soulMd,
      hasProjectMd: !!projectMd,
      codeIndex:    codeIndexStats,
      tokens: stats ? {
        last: stats.last_injection_tokens || 0,
        budget: stats.last_injection_budget || 0,
        blocks: stats.last_injection_blocks || [],
        skipped: stats.last_injection_skipped || [],
        at: stats.last_injection_at || null
      } : null
    }
  };
}

// ── Chat com agentes (spawn `claude -p`) ─────────────────────────────────────

const chatDir = path.join(memDir, 'agent_chats');

function chatLogPath(agent) { return path.join(chatDir, `${agent}.jsonl`); }

function appendChat(agent, entry) {
  if (!fs.existsSync(chatDir)) fs.mkdirSync(chatDir, { recursive: true });
  fs.appendFileSync(chatLogPath(agent), JSON.stringify(entry) + '\n');
}

function readChat(agent) {
  const p = chatLogPath(agent);
  if (!fs.existsSync(p)) return [];
  return fs.readFileSync(p, 'utf8').split('\n').filter(Boolean).map(l => {
    try { return JSON.parse(l); } catch { return null; }
  }).filter(Boolean);
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    let data = '';
    req.on('data', c => { data += c; if (data.length > 100_000) req.destroy(); });
    req.on('end', () => resolve(data));
    req.on('error', reject);
  });
}

function composePrompt(agent, agentCfg, miniContext, userMessage, history) {
  const hist = (history || []).slice(-6).map(m => `**${m.role === 'user' ? 'Engenheiro' : agent}:** ${m.content}`).join('\n\n');
  return `Você é o agente **${agent}** neste projeto, com o papel definido abaixo. O Engenheiro humano está falando com você direto pela UI do project-mem.

## Seu mini-contexto
${miniContext || '_(nenhum mini-contexto definido)_'}

## Regras
- Responda dentro do seu papel (${agentCfg.type || 'specialist'}).
- Se for ação que exige confirmação do Manager (regra de ouro do projeto), diga isso explicitamente em vez de agir.
- Seja direto. Sem preâmbulo. Português.
${hist ? '\n## Conversa recente\n' + hist + '\n' : ''}
## Mensagem do Engenheiro
${userMessage}
`;
}

function handleChatHistory(url, res) {
  const agent = url.searchParams.get('agent');
  if (!agent) {
    res.writeHead(400, { 'Content-Type': 'application/json' });
    return res.end(JSON.stringify({ error: 'missing agent' }));
  }
  const messages = readChat(agent);
  res.writeHead(200, { 'Content-Type': 'application/json', 'Access-Control-Allow-Origin': '*' });
  res.end(JSON.stringify({ agent, messages }));
}

async function handleChatSend(req, res) {
  const raw = await readBody(req);
  let body;
  try { body = JSON.parse(raw); } catch {
    res.writeHead(400, { 'Content-Type': 'application/json' });
    return res.end(JSON.stringify({ error: 'invalid json' }));
  }

  const { agent, message } = body || {};
  if (!agent || !message || typeof message !== 'string') {
    res.writeHead(400, { 'Content-Type': 'application/json' });
    return res.end(JSON.stringify({ error: 'need {agent, message}' }));
  }

  const manifest = agents.loadManifest(memDir);
  const agentCfg = manifest && manifest.agents && manifest.agents[agent];
  if (!agentCfg) {
    res.writeHead(404, { 'Content-Type': 'application/json' });
    return res.end(JSON.stringify({ error: `agent "${agent}" not found` }));
  }

  const miniContext = agents.loadMiniContext(memDir, agentCfg.context_file) || '';
  const history = readChat(agent);
  const prompt = composePrompt(agent, agentCfg, miniContext, message, history);

  const userTs = new Date().toISOString();
  appendChat(agent, { ts: userTs, role: 'user', content: message });

  res.writeHead(200, {
    'Content-Type': 'text/event-stream',
    'Cache-Control': 'no-cache',
    'Connection':    'keep-alive',
    'Access-Control-Allow-Origin': '*'
  });

  const sse = (event, data) => res.write(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
  sse('start', { agent, ts: userTs });

  const args = ['-p', '--output-format', 'text'];
  if (agentCfg.model) args.push('--model', agentCfg.model);
  args.push(prompt);

  const started = Date.now();
  let proc;
  try {
    proc = spawn('claude', args, { cwd: projectRoot, stdio: ['ignore', 'pipe', 'pipe'] });
  } catch (err) {
    sse('error', { message: `falha ao iniciar claude CLI: ${err.message}` });
    sse('done', { duration_ms: 0 });
    return res.end();
  }

  let full = '';
  let errBuf = '';

  proc.stdout.on('data', chunk => {
    const text = chunk.toString();
    full += text;
    sse('text', { text });
  });
  proc.stderr.on('data', chunk => { errBuf += chunk.toString(); });

  proc.on('error', err => {
    sse('error', { message: err.message });
    sse('done', { duration_ms: Date.now() - started });
    res.end();
  });

  proc.on('close', code => {
    const duration_ms = Date.now() - started;
    const agentTs = new Date().toISOString();
    if (code !== 0 && !full) {
      sse('error', { code, stderr: errBuf.slice(0, 500) });
    } else {
      appendChat(agent, { ts: agentTs, role: 'agent', content: full, duration_ms });
    }
    sse('done', { duration_ms, code });
    res.end();
  });

  req.on('close', () => { try { proc.kill('SIGTERM'); } catch {} });
}

const server = http.createServer((req, res) => {
  const url = new URL(req.url, `http://localhost:${PORT}`);

  if (url.pathname === '/api/chat' && req.method === 'POST') {
    handleChatSend(req, res).catch(err => {
      try {
        res.writeHead(500, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ error: err.message }));
      } catch {}
    });
    return;
  }

  if (url.pathname === '/api/chat/history' && req.method === 'GET') {
    handleChatHistory(url, res);
    return;
  }

  if (url.pathname === '/api/data') {
    try {
      res.writeHead(200, {
        'Content-Type':                'application/json',
        'Access-Control-Allow-Origin': '*',
        'Cache-Control':               'no-cache'
      });
      res.end(JSON.stringify(getData()));
    } catch (err) {
      res.writeHead(500, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ error: err.message }));
    }
    return;
  }

  if (url.pathname === '/') {
    try {
      res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
      res.end(fs.readFileSync(HTML_PATH, 'utf8'));
    } catch {
      res.writeHead(500);
      res.end('index.html not found');
    }
    return;
  }

  res.writeHead(404);
  res.end('Not found');
});

server.listen(PORT, '127.0.0.1', () => {
  console.log(`\nproject-mem UI rodando em: http://localhost:${PORT}`);
  console.log(`Projeto: ${projectName}`);
  console.log(`Memória: ${memDir}`);
  console.log('\nCtrl+C para parar\n');
});

server.on('error', err => {
  if (err.code === 'EADDRINUSE') {
    console.error(`Porta ${PORT} já em uso. Tente: PROJECT_MEM_PORT=37779 node server.cjs`);
  } else {
    console.error(err.message);
  }
  process.exit(1);
});
