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
const DIST_DIR    = path.resolve(__dirname, '../..', 'ui-next', 'dist');
const HAS_DIST    = fs.existsSync(path.join(DIST_DIR, 'index.html'));

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js':   'application/javascript; charset=utf-8',
  '.mjs':  'application/javascript; charset=utf-8',
  '.css':  'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg':  'image/svg+xml',
  '.png':  'image/png',
  '.jpg':  'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif':  'image/gif',
  '.ico':  'image/x-icon',
  '.woff': 'font/woff',
  '.woff2':'font/woff2',
  '.map':  'application/json',
};

function serveStaticFromDist(pathname, res) {
  const clean = pathname.replace(/^\/+/, '').replace(/\?.*$/, '');
  const target = clean === '' ? 'index.html' : clean;
  const filePath = path.resolve(DIST_DIR, target);
  if (!filePath.startsWith(DIST_DIR)) { res.writeHead(403); res.end('forbidden'); return true; }
  if (!fs.existsSync(filePath) || !fs.statSync(filePath).isFile()) return false;
  const ext = path.extname(filePath).toLowerCase();
  res.writeHead(200, {
    'Content-Type': MIME[ext] || 'application/octet-stream',
    'Cache-Control': ext === '.html' ? 'no-cache' : 'public, max-age=3600'
  });
  res.end(fs.readFileSync(filePath));
  return true;
}

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
      ensureAgentChannels(manifest);
      const { counts = {}, recent = {} } = agents.countInvocations(memDir);
      const contexts = {};
      for (const [name, a] of Object.entries(manifest.agents || {})) {
        contexts[name] = agents.loadMiniContext(memDir, a.context_file) || '';
      }
      const logs = agentLogSummary();
      agentsData = { manifest, contexts, counts, recent, logs };
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
const logDir  = path.join(memDir, 'agent_logs');

function chatLogPath(agent) { return path.join(chatDir, `${agent}.jsonl`); }
function agentLogPath(agent) { return path.join(logDir, `${agent}.jsonl`); }

function appendChat(agent, entry) {
  if (!fs.existsSync(chatDir)) fs.mkdirSync(chatDir, { recursive: true });
  fs.appendFileSync(chatLogPath(agent), JSON.stringify(entry) + '\n');
}

// Garante que todo agente do manifest tenha seu arquivo de canal — permite
// que agentes (subprocessos) façam append sem precisar criar o arquivo.
function ensureAgentChannels(manifest) {
  if (!manifest || !manifest.agents) return;
  if (!fs.existsSync(chatDir)) fs.mkdirSync(chatDir, { recursive: true });
  for (const name of Object.keys(manifest.agents)) {
    const p = chatLogPath(name);
    if (!fs.existsSync(p)) fs.writeFileSync(p, '');
  }
}

function appendAgentLog(agent, entry) {
  if (!fs.existsSync(logDir)) fs.mkdirSync(logDir, { recursive: true });
  fs.appendFileSync(agentLogPath(agent), JSON.stringify(entry) + '\n');
}

function readChat(agent) {
  const p = chatLogPath(agent);
  if (!fs.existsSync(p)) return [];
  return fs.readFileSync(p, 'utf8').split('\n').filter(Boolean).map(l => {
    try { return JSON.parse(l); } catch { return null; }
  }).filter(Boolean);
}

function readAgentLog(agent, limit) {
  const p = agentLogPath(agent);
  if (!fs.existsSync(p)) return [];
  const lines = fs.readFileSync(p, 'utf8').split('\n').filter(Boolean);
  const slice = typeof limit === 'number' && limit > 0 ? lines.slice(-limit) : lines;
  return slice.map(l => { try { return JSON.parse(l); } catch { return null; } }).filter(Boolean);
}

// Classifica mensagens de erro comuns em sugestões de correção acionáveis.
// Retorna string curta (1 linha) ou null quando não souber sugerir.
function classifyErrorHint(msg, agent) {
  if (!msg) return null;
  const m = String(msg);
  const low = m.toLowerCase();
  if (low.includes('permission denied') || low.includes('eacces')) {
    return 'Libere o caminho em .claude/settings.json ou aprove o prompt de permissão.';
  }
  if (low.includes('enoent') || /no such file/i.test(m)) {
    if (low.includes('agent_chats')) {
      return `Canal ${agent}.jsonl não existe — reinicie o servidor pra pré-criar.`;
    }
    return 'Arquivo ou diretório não encontrado — confira o caminho.';
  }
  if (low.includes('spawn') && (low.includes('enoent') || low.includes('claude'))) {
    return 'Claude Code CLI não encontrado no PATH.';
  }
  if (/^exit \d+$/i.test(m.trim())) {
    return `Ver log completo em agent_logs/${agent}.jsonl.`;
  }
  if (low.includes('timeout')) {
    return 'Tempo esgotado — modelo ou rede demorou demais para responder.';
  }
  if (low.includes('rate limit') || low.includes('429')) {
    return 'Rate limit atingido — aguarde alguns minutos antes de retry.';
  }
  return `Ver log completo em agent_logs/${agent}.jsonl.`;
}

function agentLogSummary() {
  const out = {};
  if (!fs.existsSync(logDir)) return out;
  for (const f of fs.readdirSync(logDir)) {
    if (!f.endsWith('.jsonl')) continue;
    const name = f.replace(/\.jsonl$/, '');
    const entries = readAgentLog(name);
    let errors = 0, requests = 0, responses = 0;
    let lastErrorAt = null, lastErrorMsg = null;
    let lastDurationMs = null;
    for (const e of entries) {
      if (e.type === 'request') requests++;
      else if (e.type === 'response') { responses++; lastDurationMs = e.duration_ms; }
      else if (e.type === 'error') {
        errors++;
        lastErrorAt = e.ts;
        lastErrorMsg = e.stderr || e.message || `exit ${e.code}`;
        lastDurationMs = e.duration_ms;
      }
    }
    const lastErrorHint = errors > 0 ? classifyErrorHint(lastErrorMsg, name) : null;
    out[name] = {
      total: entries.length,
      requests,
      responses,
      errors,
      lastErrorAt,
      lastErrorMsg,
      lastErrorHint,
      lastDurationMs,
    };
  }
  return out;
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
  const isManager = agent === 'manager';
  const hist = (history || []).slice(-6).map(m => `**${m.role === 'user' ? 'Engenheiro' : agent}:** ${m.content}`).join('\n\n');
  return `Você é o agente **${agent}** neste projeto, com o papel definido abaixo. A mensagem abaixo chega pela UI do project-mem.

## Seu mini-contexto
${miniContext || '_(nenhum mini-contexto definido)_'}

## Atores e autoria
Separe sempre quem pediu o quê:
- **Engenheiro** — humano dono do projeto. É com ele que você está falando neste chat.
- **Manager** — agente orquestrador. Coordena delegações entre agentes. **Não é o Engenheiro.** Tarefas que o Manager repassa chegam em outro canal (Task tool), não por este chat.
- **Você (${agent})** — especialista na sua área.

Regras de autoria nesta conversa:
- Toda mensagem abaixo vem **do Engenheiro** (direto).
- Se o Engenheiro relatar uma decisão/ordem que veio do Manager ("o manager decidiu X"), trate isso como fato reportado, não como pedido direto do Manager.
- Nunca confunda "o Manager disse" com "o Engenheiro disse" ao registrar qualquer coisa — a origem importa.

## Regras de resposta
- Responda dentro do seu papel (${agentCfg.type || 'specialist'}).
- Seja direto. Sem preâmbulo. Português.
${isManager
  ? '- Você orquestra: decide quem faz o quê e responde com a decisão consolidada.'
  : '- Se o pedido é decisão ampla de projeto (stack, roadmap, valores, anti-padrões), responda que isso deve passar pelo Manager — **não decida sozinho**.'}

## Persistência de anotações (project-mem)
Quando o Engenheiro disser "anote", "registre", "salve" ou equivalente, **não pergunte onde** — classifique e grave. Siga a hierarquia do README:

| Tipo de anotação | Destino | Como salvar | Quem pode registrar |
|---|---|---|---|
| Decisão técnica local do SEU escopo (com "por quê") | \`decisions.jsonl\` → \`DECISIONS.md\` | \`/mem-decision <o que> — <por quê>\` | ${isManager ? 'Manager' : 'você mesmo'} |
| Decisão ampla do projeto (processo, roadmap, arquitetura global) | \`decisions.jsonl\` → \`DECISIONS.md\` | \`/mem-decision ...\` | **Manager** ${isManager ? '(você)' : '— escale'} |
| Identidade: valores, filosofia, voz, estética, anti-padrão | \`SOUL.md\` | \`/mem-soul add|update: ...\` | **Manager** ${isManager ? '(você)' : '— escale'} |
| Fato estrutural: stack, pasta, padrão, como rodar | \`PROJECT.md\` | editar direto (ou \`/mem-setup\`) | Manager (ou agente cujo domínio é o fato) |
| Armadilha (bug + conserto) do SEU escopo | \`gotchas.jsonl\` → \`GOTCHAS.md\` | append manual | ${isManager ? 'Manager' : 'você mesmo'} |
| Termo do glossário | \`glossary.jsonl\` → \`GLOSSARY.md\` | append manual | **Manager** ${isManager ? '(você)' : '— escale'} |

Diretrizes:
- Escolha UM destino. Não divida a mesma anotação.
- **Marque a origem** no próprio registro: \`origem: Engenheiro\` (quando veio direto do dev, como aqui) ou \`origem: Manager via <agente>\` (quando é repasse interno).
- Ao confirmar, responda em UMA linha: "registrei em <arquivo> (decisão/alma/gotcha) — origem: Engenheiro".
- Se estiver inseguro entre dois destinos, faça UMA pergunta curta; se estiver entre registrar agora ou escalar ao Manager, **escalar**.
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
  ensureAgentChannels(manifest);

  const miniContext = agents.loadMiniContext(memDir, agentCfg.context_file) || '';
  const history = readChat(agent);
  const prompt = composePrompt(agent, agentCfg, miniContext, message, history);

  const userTs = new Date().toISOString();
  appendChat(agent, { ts: userTs, role: 'user', content: message });
  appendAgentLog(agent, {
    ts: userTs,
    type: 'request',
    prompt_chars: prompt.length,
    user_chars: message.length,
    history_turns: history.length,
    model: agentCfg.model || null,
  });

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
    appendAgentLog(agent, {
      ts: new Date().toISOString(),
      type: 'error',
      stage: 'spawn',
      message: err.message,
      duration_ms: 0,
    });
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
    const duration_ms = Date.now() - started;
    sse('error', { message: err.message });
    sse('done', { duration_ms });
    appendAgentLog(agent, {
      ts: new Date().toISOString(),
      type: 'error',
      stage: 'proc',
      message: err.message,
      duration_ms,
    });
    res.end();
  });

  proc.on('close', code => {
    const duration_ms = Date.now() - started;
    const agentTs = new Date().toISOString();
    if (code !== 0 && !full) {
      sse('error', { code, stderr: errBuf.slice(0, 500) });
      appendAgentLog(agent, {
        ts: agentTs,
        type: 'error',
        stage: 'exit',
        code,
        stderr: errBuf.slice(0, 500),
        duration_ms,
      });
    } else {
      appendChat(agent, { ts: agentTs, role: 'agent', content: full, duration_ms });
      appendAgentLog(agent, {
        ts: agentTs,
        type: 'response',
        response_chars: full.length,
        duration_ms,
        code,
        stderr_preview: errBuf ? errBuf.slice(0, 200) : null,
      });
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

  if (url.pathname === '/api/agents/log' && req.method === 'GET') {
    const agent = url.searchParams.get('agent');
    const limit = parseInt(url.searchParams.get('limit') || '50', 10);
    if (!agent) {
      res.writeHead(400, { 'Content-Type': 'application/json' });
      return res.end(JSON.stringify({ error: 'missing agent' }));
    }
    res.writeHead(200, { 'Content-Type': 'application/json', 'Access-Control-Allow-Origin': '*' });
    res.end(JSON.stringify({ agent, entries: readAgentLog(agent, limit) }));
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

  if (HAS_DIST && (req.method === 'GET' || req.method === 'HEAD')) {
    if (serveStaticFromDist(url.pathname, res)) return;
    // SPA fallback
    res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-cache' });
    res.end(fs.readFileSync(path.join(DIST_DIR, 'index.html')));
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
  console.log(`UI: ${HAS_DIST ? 'ui-next/dist (React)' : 'plugin/ui/index.html (legacy)'}`);
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
