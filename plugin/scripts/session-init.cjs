'use strict';

// Fired on UserPromptSubmit.
// Injects project context once per session via additionalContext.
// Also syncs PROJECT.md → .claude/CLAUDE.md for native Claude context reading.
// Roda detecção de correção em TODOS os turnos (não só no primeiro).

const path = require('path');
const fs   = require('fs');
const { execFileSync } = require('child_process');
const {
  getMemDir, ensureMemDir, ensureTmpDir, getTmpDir,
  readJsonl, readLastJsonlLine, appendJsonl,
  initProjectConfig, syncClaudeMd,
  readStdin, getCwd, findProjectRoot,
  estimateTokens, loadStats, saveStats
} = require('./lib.cjs');

const input       = readStdin();
const sessionId   = input.session_id;
const userPrompt  = typeof input.prompt === 'string' ? input.prompt : '';
const cwd         = getCwd(input);
const memDir      = getMemDir(cwd);
const projectRoot = path.dirname(memDir);
const projectName = path.basename(projectRoot);
const soulMd      = path.join(memDir, 'SOUL.md');
const projectMd   = path.join(memDir, 'PROJECT.md');
const obsFile     = path.join(memDir, 'observations.jsonl');
const sessFile    = path.join(memDir, 'sessions.jsonl');
const decisionsFile   = path.join(memDir, 'decisions.jsonl');
const correctionsFile = path.join(memDir, 'corrections.jsonl');
const gotchasFile     = path.join(memDir, 'gotchas.jsonl');
const decisionsMd     = path.join(memDir, 'DECISIONS.md');
const gotchasMd       = path.join(memDir, 'GOTCHAS.md');
const glossaryMd      = path.join(memDir, 'GLOSSARY.md');

const TOKEN_BUDGET = parseInt(process.env.PROJECT_MEM_TOKEN_BUDGET || '4000', 10);

ensureMemDir(memDir);
ensureTmpDir(memDir);

// ── Detector de correções (roda em todo UserPromptSubmit) ─────────────────────
// Heurística conservadora: mensagens curtas começando com marcadores explícitos
// de correção. Evita falsos positivos em prompts normais.

const CORRECTION_MARKERS = /^(\s*)(não\b|nao\b|pare\b|espera\b|calma\b|errado\b|incorret[oa]\b|volta\b|desfaz\b|stop\b|wait\b|don'?t\b|no[,!.\s]|wrong\b|incorrect\b|undo\b|revert\b)/i;

function maybeRecordCorrection() {
  if (!userPrompt) return;
  if (userPrompt.length > 600) return; // correções costumam ser curtas
  if (!CORRECTION_MARKERS.test(userPrompt)) return;

  const lastObs = readLastJsonlLine(obsFile);
  const previous = (lastObs && lastObs.session_id === sessionId) ? lastObs : null;

  appendJsonl(correctionsFile, {
    session_id: sessionId,
    prompt: userPrompt.slice(0, 500),
    previous_action: previous ? compactObs(previous) : null,
    ts: new Date().toISOString()
  });
}

function compactObs(o) {
  if (o.type === 'file_edit') return { type: 'file_edit', file: o.file };
  if (o.type === 'bash')      return { type: 'bash', cmd: (o.cmd || '').slice(0, 120) };
  return { type: o.type };
}

try { maybeRecordCorrection(); } catch {}

// ── Inject only once per session ──────────────────────────────────────────────
const tmpDir  = getTmpDir(memDir);
const flagFile = sessionId ? path.join(tmpDir, `.init-${sessionId}`) : null;

if (flagFile && fs.existsSync(flagFile)) {
  console.log(JSON.stringify({ continue: true, suppressOutput: true }));
  process.exit(0);
}

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

// Sync SOUL.md + PROJECT.md → .claude/CLAUDE.md so Claude reads context
// natively. Alma vem primeiro — é a lente pela qual os fatos são interpretados.
const soulContent    = fs.existsSync(soulMd)    ? fs.readFileSync(soulMd,    'utf8').trim() : '';
const projectContent = fs.existsSync(projectMd) ? fs.readFileSync(projectMd, 'utf8').trim() : '';
const nativeBlock    = [soulContent, projectContent].filter(Boolean).join('\n\n---\n\n');
if (nativeBlock) {
  try { syncClaudeMd(projectRoot, nativeBlock); } catch {}
}

// ── Blocos com prioridade (menor = mais importante, entra primeiro no budget) ─
const blocks = [];

function pushBlock(name, priority, content) {
  const text = Array.isArray(content) ? content.join('\n') : (content || '');
  const trimmed = text.trim();
  if (!trimmed) return;
  blocks.push({ name, priority, text: trimmed, tokens: estimateTokens(trimmed) });
}

// 1. SOUL.md — identidade sagrada
if (soulContent) {
  pushBlock('soul', 1, soulContent);
}

// 2. PROJECT.md — memória factual permanente
if (projectContent) {
  pushBlock('project', 2, projectContent);
}

// 3. MDs hierárquicos (DECISIONS/GOTCHAS/GLOSSARY)
for (const [name, title, file] of [
  ['decisions_md', '## Decisões registradas', decisionsMd],
  ['gotchas_md',   '## Armadilhas conhecidas', gotchasMd],
  ['glossary_md',  '## Glossário do projeto', glossaryMd]
]) {
  if (!fs.existsSync(file)) continue;
  const content = fs.readFileSync(file, 'utf8').trim().replace(/^#\s+.*\n/, '').trim();
  if (content) pushBlock(name, 3, [title, content]);
}

// 4. Agentes declarados (Manager + Oracle + roles) — guia pro Claude usar subagents
try {
  const { buildInjectionBlock } = require('./agents.cjs');
  const agentLines = buildInjectionBlock(memDir);
  if (agentLines.length > 0) pushBlock('agents', 4, agentLines);
} catch {}

// 5. Git context
try {
  const git = readGitContext(projectRoot);
  if (git) {
    const gl = ['## Estado do git', `- Branch: \`${git.branch}\``];
    if (git.modified && git.modified.length > 0) {
      gl.push(`- Modificados: ${git.modified.slice(0, 10).map(f => `\`${f}\``).join(', ')}${git.modified.length > 10 ? ` (+${git.modified.length - 10})` : ''}`);
    }
    if (git.recent_commits && git.recent_commits.length > 0) {
      gl.push('- Commits recentes:');
      for (const c of git.recent_commits) gl.push(`  - \`${c.sha}\` ${c.msg}`);
    }
    pushBlock('git', 4, gl);
  }
} catch {}

// 5. ADRs
try {
  const adrs = loadAdrs(projectRoot);
  if (adrs.length > 0) {
    const al = ['## ADRs do repositório'];
    for (const adr of adrs) al.push(`- \`${adr.file}\` — ${adr.title}`);
    pushBlock('adrs', 5, al);
  }
} catch {}

// 6. Sessões recentes
const sessions = readJsonl(sessFile);
const summaries = sessions
  .filter(s => s.type === 'end' && s.summary)
  .slice(-5)
  .reverse();
if (summaries.length > 0) {
  const sl = ['## Sessões recentes'];
  for (const s of summaries) {
    const date = (s.ended_at || s.started_at || '').slice(0, 10);
    const kind = s.kind ? ` _[${s.kind}]_` : '';
    sl.push(`- **${date}**${kind}: ${s.summary}`);
  }
  pushBlock('sessions', 7, sl);
}

// 7. Correções recentes (prioridade alta — não repetir erro)
const corrections = readJsonl(correctionsFile).slice(-8).reverse();
if (corrections.length > 0) {
  const cl = ['## Correções do dev (não repetir)'];
  for (const c of corrections) {
    const date = (c.ts || '').slice(0, 10);
    const prev = c.previous_action
      ? ` (após ${c.previous_action.type === 'file_edit'
          ? `editar ${c.previous_action.file}`
          : `rodar \`${c.previous_action.cmd}\``})`
      : '';
    cl.push(`- **${date}**${prev}: "${c.prompt.slice(0, 160)}"`);
  }
  pushBlock('corrections', 7, cl);
}

// 8. Decisões recentes (log bruto)
const decisions = readJsonl(decisionsFile).slice(-5).reverse();
if (decisions.length > 0) {
  const dl = ['## Decisões recentes (log bruto)'];
  for (const d of decisions) {
    const date = (d.ts || '').slice(0, 10);
    const what = d.what || d.text || '';
    const why  = d.why ? ` — _${d.why}_` : '';
    dl.push(`- **${date}**: ${what}${why}`);
  }
  pushBlock('decisions_log', 8, dl);
}

// 9. Gotchas log
const gotchas = readJsonl(gotchasFile).slice(-5).reverse();
if (gotchas.length > 0) {
  const gl = ['## Armadilhas detectadas'];
  for (const g of gotchas) {
    const date = (g.ts || '').slice(0, 10);
    gl.push(`- **${date}**: ${g.description}${g.file ? ` — \`${g.file}\`` : ''}`);
  }
  pushBlock('gotchas_log', 9, gl);
}

// 10. Observações recentes
const observations = readJsonl(obsFile);
const recentObs = observations
  .filter(o => o.session_id !== sessionId)
  .slice(-40);
if (recentObs.length > 0) {
  const ol = ['## Observações recentes'];
  for (const o of recentObs) {
    const ts = (o.ts || '').slice(0, 16).replace('T', ' ');
    ol.push(formatObs(ts, o));
  }
  pushBlock('observations', 10, ol);
}

// 11. Code index (símbolos reutilizáveis dos arquivos quentes)
try {
  const indexer = require('./indexer.cjs');
  const idxLines = indexer.getContextLines(memDir, 25);
  if (idxLines.length > 0) pushBlock('code_index', 6, idxLines);
} catch {}

// 12. Graph patterns
try {
  const { loadGraph, getContextLines } = require('./graph.cjs');
  const graph      = loadGraph(memDir);
  const graphLines = getContextLines(graph);
  if (graphLines.length > 0) {
    pushBlock('graph', 11, ['## Padrões detectados', ...graphLines]);
  }
} catch {}

// ── Aplica orçamento de tokens ────────────────────────────────────────────────
blocks.sort((a, b) => a.priority - b.priority);

const included = [];
const skipped  = [];
let totalTokens = 0;
for (const b of blocks) {
  if (totalTokens + b.tokens <= TOKEN_BUDGET) {
    included.push(b);
    totalTokens += b.tokens;
  } else {
    skipped.push(b);
  }
}

// Persist métricas na stats.json
try {
  const stats = loadStats(memDir) || { sessions: 0, observations: 0 };
  stats.last_injection_tokens  = totalTokens;
  stats.last_injection_budget  = TOKEN_BUDGET;
  stats.last_injection_at      = new Date().toISOString();
  stats.last_injection_blocks  = included.map(b => `${b.name}:${b.tokens}`);
  stats.last_injection_skipped = skipped.map(b => `${b.name}:${b.tokens}`);
  saveStats(memDir, stats);
} catch {}

// Monta texto final
const parts = [];
for (let i = 0; i < included.length; i++) {
  parts.push(included[i].text);
  if (i < included.length - 1) parts.push('\n\n---\n');
}

// Rodapé com telemetria do orçamento (baixo custo, ajuda debug)
if (skipped.length > 0) {
  parts.push(`\n\n<!-- project-mem: ~${totalTokens}/${TOKEN_BUDGET}t · omitido: ${skipped.map(b => b.name).join(', ')} -->`);
} else if (included.length > 0) {
  parts.push(`\n\n<!-- project-mem: ~${totalTokens}/${TOKEN_BUDGET}t -->`);
}

function formatObs(ts, o) {
  switch (o.type) {
    case 'file_edit': {
      const d = o.diff;
      const stat = d && typeof d.ins === 'number'
        ? ` (+${d.ins}/-${d.del || 0}${d.tracked === false ? ' novo' : ''})`
        : '';
      return `- \`${ts}\` EDIT \`${o.file}\`${stat}`;
    }
    case 'bash': {
      const exit = o.exit !== undefined && o.exit !== null ? ` → exit ${o.exit}` : '';
      const out = o.output_summary ? ` _${o.output_summary}_` : '';
      return `- \`${ts}\` BASH \`${(o.cmd || '').slice(0, 120)}\`${exit}${out}`;
    }
    case 'agent_call': {
      const desc = o.description ? ` — ${o.description}` : '';
      return `- \`${ts}\` AGENT \`${o.agent}\`${desc}`;
    }
    case 'note': return `- \`${ts}\` NOTE: ${o.content}`;
    default:     return `- \`${ts}\` ${o.type}`;
  }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

function readGitContext(root) {
  try {
    const branch = execFileSync('git', ['rev-parse', '--abbrev-ref', 'HEAD'], {
      cwd: root, timeout: 1500, stdio: ['pipe', 'pipe', 'pipe']
    }).toString().trim();

    const modifiedRaw = execFileSync('git', ['status', '--porcelain'], {
      cwd: root, timeout: 1500, stdio: ['pipe', 'pipe', 'pipe']
    }).toString();
    const modified = modifiedRaw.split('\n')
      .filter(l => l.trim())
      .map(l => l.slice(3).trim())
      .filter(Boolean);

    const logRaw = execFileSync('git', ['log', '-5', '--pretty=%h|%s'], {
      cwd: root, timeout: 1500, stdio: ['pipe', 'pipe', 'pipe']
    }).toString();
    const recent_commits = logRaw.split('\n')
      .filter(l => l.includes('|'))
      .map(l => {
        const [sha, ...rest] = l.split('|');
        return { sha, msg: rest.join('|').slice(0, 80) };
      });

    return { branch, modified, recent_commits };
  } catch {
    return null;
  }
}

function loadAdrs(root) {
  const candidates = ['docs/adr', 'docs/adrs', 'docs/decisions', 'adr'];
  for (const c of candidates) {
    const dir = path.join(root, c);
    if (!fs.existsSync(dir)) continue;
    try {
      const files = fs.readdirSync(dir).filter(f => f.endsWith('.md')).sort();
      const out = [];
      for (const f of files.slice(-8)) {
        const content = fs.readFileSync(path.join(dir, f), 'utf8');
        const titleMatch = content.match(/^#\s+(.+)$/m);
        const title = titleMatch ? titleMatch[1].trim() : f.replace(/\.md$/, '');
        out.push({ file: path.join(c, f), title: title.slice(0, 100) });
      }
      return out;
    } catch { return []; }
  }
  return [];
}

if (parts.length === 0) {
  console.log(JSON.stringify({ continue: true, suppressOutput: true }));
  process.exit(0);
}

console.log(JSON.stringify({
  continue: true,
  suppressOutput: true,
  hookSpecificOutput: {
    hookEventName: 'UserPromptSubmit',
    additionalContext: parts.join('\n')
  }
}));
