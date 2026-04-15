'use strict';

// Fired on UserPromptSubmit.
// Injects project context once per session via additionalContext.
// Also syncs PROJECT.md → .claude/CLAUDE.md for native Claude context reading.

const path = require('path');
const fs   = require('fs');
const {
  getMemDir, ensureMemDir, ensureTmpDir, getTmpDir,
  readJsonl, appendJsonl,
  initProjectConfig, syncClaudeMd,
  readStdin, getCwd, findProjectRoot
} = require('./lib.cjs');

const input       = readStdin();
const sessionId   = input.session_id;
const cwd         = getCwd(input);
const memDir      = getMemDir(cwd);
const projectRoot = path.dirname(memDir);
const projectName = path.basename(projectRoot);
const projectMd   = path.join(memDir, 'PROJECT.md');
const obsFile     = path.join(memDir, 'observations.jsonl');
const sessFile    = path.join(memDir, 'sessions.jsonl');

// ── Inject only once per session ──────────────────────────────────────────────
const tmpDir  = getTmpDir(memDir);
const flagFile = sessionId ? path.join(tmpDir, `.init-${sessionId}`) : null;

if (flagFile && fs.existsSync(flagFile)) {
  console.log(JSON.stringify({ continue: true, suppressOutput: true }));
  process.exit(0);
}

ensureMemDir(memDir);
ensureTmpDir(memDir);
if (flagFile) fs.writeFileSync(flagFile, '');

// ── First time in this project ────────────────────────────────────────────────
if (!fs.existsSync(projectMd) && !fs.existsSync(sessFile)) {
  initProjectConfig(memDir, projectName);

  appendJsonl(sessFile, {
    type: 'start',
    session_id: sessionId,
    started_at: new Date().toISOString(),
    cwd
  });

  const onboarding = `## 🆕 Novo projeto detectado pelo project-mem

Este é o primeiro uso do **project-mem** neste projeto. Antes de responder ao pedido do usuário:

1. Analise a estrutura do projeto (arquivos raiz, manifesto de dependências, pastas principais, 2-3 arquivos de código).
2. Apresente um resumo: stack, arquitetura, convenções identificadas, como rodar/testar.
3. Pergunte ao desenvolvedor: **"Há convenções, restrições ou contexto importante que devo saber sobre este projeto?"**
4. Com base na análise + resposta do dev, crie o arquivo \`.claude-project-memory/PROJECT.md\` usando o skill \`/mem-setup\` como referência de estrutura.

> Após criar o PROJECT.md, ele será injetado automaticamente em todas as sessões futuras.`;

  console.log(JSON.stringify({
    continue: true,
    suppressOutput: true,
    hookSpecificOutput: {
      hookEventName: 'UserPromptSubmit',
      additionalContext: onboarding
    }
  }));
  process.exit(0);
}

// ── Subsequent sessions ───────────────────────────────────────────────────────
appendJsonl(sessFile, {
  type: 'start',
  session_id: sessionId,
  started_at: new Date().toISOString(),
  cwd
});

// Sync PROJECT.md → .claude/CLAUDE.md so Claude reads context natively.
// This runs before context injection so the native read is always current.
if (fs.existsSync(projectMd)) {
  const content = fs.readFileSync(projectMd, 'utf8').trim();
  if (content) {
    try { syncClaudeMd(projectRoot, content); } catch {}
  }
}

const lines = [];

// PROJECT.md as top-priority permanent context
if (fs.existsSync(projectMd)) {
  const projectContent = fs.readFileSync(projectMd, 'utf8').trim();
  if (projectContent) {
    lines.push(projectContent);
    lines.push('');
    lines.push('---');
    lines.push('');
  }
}

// Last 5 session summaries
const sessions = readJsonl(sessFile);
const summaries = sessions
  .filter(s => s.type === 'end' && s.summary)
  .slice(-5)
  .reverse();

if (summaries.length > 0) {
  lines.push('## Sessões recentes');
  for (const s of summaries) {
    const date = (s.ended_at || s.started_at || '').slice(0, 10);
    lines.push(`- **${date}**: ${s.summary}`);
  }
  lines.push('');
}

// Last 40 observations from previous sessions
const observations = readJsonl(obsFile);
const recentObs = observations
  .filter(o => o.session_id !== sessionId)
  .slice(-40);

if (recentObs.length > 0) {
  lines.push('## Observações recentes');
  for (const o of recentObs) {
    const ts = (o.ts || '').slice(0, 16).replace('T', ' ');
    lines.push(formatObs(ts, o));
  }
  lines.push('');
}

// Graph patterns
const { loadGraph, getContextLines } = require('./graph.cjs');
try {
  const graph      = loadGraph(memDir);
  const graphLines = getContextLines(graph);
  if (graphLines.length > 0) {
    lines.push('## Padrões detectados');
    lines.push(...graphLines);
  }
} catch {}

function formatObs(ts, o) {
  switch (o.type) {
    case 'file_edit': return `- \`${ts}\` EDIT \`${o.file}\``;
    case 'bash': {
      const exit = o.exit !== undefined && o.exit !== null ? ` → exit ${o.exit}` : '';
      return `- \`${ts}\` BASH \`${(o.cmd || '').slice(0, 120)}\`${exit}`;
    }
    case 'note': return `- \`${ts}\` NOTE: ${o.content}`;
    default:     return `- \`${ts}\` ${o.type}`;
  }
}

if (lines.length === 0) {
  console.log(JSON.stringify({ continue: true, suppressOutput: true }));
  process.exit(0);
}

console.log(JSON.stringify({
  continue: true,
  suppressOutput: true,
  hookSpecificOutput: {
    hookEventName: 'UserPromptSubmit',
    additionalContext: lines.join('\n')
  }
}));
