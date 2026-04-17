'use strict';

// Fired on SessionEnd.
// - Generates a summary of the session and appends it to sessions.jsonl
// - Classifica a sessão (kind) a partir de prompt inicial + arquivos tocados
// - Detecta sequências fail→retry→success e grava em gotchas.jsonl
// - Regenera DECISIONS.md / GOTCHAS.md / GLOSSARY.md a partir dos JSONLs

const path = require('path');
const fs   = require('fs');
const {
  getMemDir, readJsonl, appendJsonl,
  incrementStats, getTmpDir,
  readStdin, getCwd
} = require('./lib.cjs');

const input     = readStdin();
const sessionId = input.session_id;
const cwd       = getCwd(input);
const memDir    = getMemDir(cwd);

const obsFile         = path.join(memDir, 'observations.jsonl');
const sessFile        = path.join(memDir, 'sessions.jsonl');
const gotchasFile     = path.join(memDir, 'gotchas.jsonl');
const decisionsFile   = path.join(memDir, 'decisions.jsonl');
const decisionsMd     = path.join(memDir, 'DECISIONS.md');
const gotchasMd       = path.join(memDir, 'GOTCHAS.md');
const glossaryMd      = path.join(memDir, 'GLOSSARY.md');
const glossaryFile    = path.join(memDir, 'glossary.jsonl');

if (!fs.existsSync(obsFile)) {
  regenerateMds();
  process.exit(0);
}

const allObs     = readJsonl(obsFile);
const sessionObs = sessionId ? allObs.filter(o => o.session_id === sessionId) : [];

if (sessionObs.length === 0) {
  regenerateMds();
  process.exit(0);
}

// ── Resumo textual (mantido) ─────────────────────────────────────────────────
const editedFiles = [...new Set(
  sessionObs.filter(o => o.type === 'file_edit').map(o => o.file.split('/').pop())
)].slice(0, 8);

const cmds = [...new Set(
  sessionObs.filter(o => o.type === 'bash').map(o => o.cmd.split(/\s+/).slice(0, 4).join(' '))
)].slice(0, 5);

const notes = sessionObs.filter(o => o.type === 'note').map(o => o.content).slice(0, 3);

const parts = [];
if (editedFiles.length > 0) parts.push(`Editado: ${editedFiles.join(', ')}`);
if (cmds.length > 0)        parts.push(`Rodou: ${cmds.join('; ')}`);
if (notes.length > 0)       parts.push(`Notas: ${notes.join('; ')}`);

const summary = parts.length > 0 ? parts.join('. ') : `${sessionObs.length} operações`;

// ── Classificação da sessão (#7) ─────────────────────────────────────────────
const kind = classifySession(sessionObs);

// ── Detecção de gotchas (#4): fail → retry → success no mesmo arquivo ────────
try { detectGotchas(sessionObs); } catch {}

appendJsonl(sessFile, {
  type: 'end',
  session_id: sessionId,
  ended_at: new Date().toISOString(),
  obs_count: sessionObs.length,
  kind,
  summary
});

incrementStats(memDir, 'sessions');

// ── Regenera MDs hierárquicos (#8) ───────────────────────────────────────────
regenerateMds();

// ── Update incremental do code index (Aider-style) ──────────────────────────
try {
  const { updateIndexForFiles } = require('./indexer.cjs');
  const projectRoot = path.dirname(memDir);
  const changedFiles = [...new Set(
    sessionObs.filter(o => o.type === 'file_edit' && o.file).map(o => o.file)
  )];
  if (changedFiles.length > 0) updateIndexForFiles(memDir, projectRoot, changedFiles);
} catch {}

// Clean up session flag file from tmp/
if (sessionId) {
  const flagFile = path.join(getTmpDir(memDir), `.init-${sessionId}`);
  try { if (fs.existsSync(flagFile)) fs.unlinkSync(flagFile); } catch {}
}

process.exit(0);

// ─── Helpers ────────────────────────────────────────────────────────────────

function classifySession(obs) {
  const files = obs.filter(o => o.type === 'file_edit').map(o => o.file || '');
  const cmds  = obs.filter(o => o.type === 'bash').map(o => (o.cmd || '').toLowerCase());
  const allText = [...files, ...cmds].join(' ').toLowerCase();

  const onlyDocs = files.length > 0 && files.every(f => /\.md$|readme|docs\//i.test(f));
  if (onlyDocs) return 'docs';

  if (files.some(f => /\.(env|config|toml|yml|yaml|json)$|dockerfile|\.dockerignore|makefile/i.test(f))
      && !files.some(f => /\.(py|ts|tsx|js|jsx|rs|go|java)$/i.test(f))) {
    return 'config';
  }

  if (cmds.some(c => /\b(fix|bugfix|debug|hotfix)\b/.test(c)) ||
      /\bfix\b|\bbug\b/.test(allText) && files.some(f => /test/i.test(f))) {
    return 'bugfix';
  }

  if (cmds.some(c => /\brefactor\b|\bmove\b|\brename\b/.test(c))) return 'refactor';

  const hasTests = files.some(f => /test/i.test(f));
  const hasSrc   = files.some(f => /\.(py|ts|tsx|js|jsx|rs|go|java)$/i.test(f));
  if (hasTests && hasSrc) return 'feature';
  if (hasSrc)             return 'feature';

  if (obs.filter(o => o.type === 'file_edit').length === 0 && cmds.length > 0) return 'exploration';
  return 'unknown';
}

function detectGotchas(obs) {
  // Sequência: bash falhou → edit → bash do mesmo tipo → sucesso
  // Quando detecta, grava uma gotcha apontando o arquivo editado entre as tentativas.
  for (let i = 0; i < obs.length - 2; i++) {
    const a = obs[i], b = obs[i + 1], c = obs[i + 2];
    if (a.type !== 'bash' || (a.exit ?? 0) === 0) continue;
    if (b.type !== 'file_edit') continue;
    if (c.type !== 'bash') continue;

    const sameCmd = (a.cmd || '').split(/\s+/).slice(0, 3).join(' ') ===
                    (c.cmd || '').split(/\s+/).slice(0, 3).join(' ');
    if (!sameCmd) continue;
    if ((c.exit ?? 1) !== 0) continue;

    const description = `\`${(a.cmd || '').slice(0, 80)}\` falhou até editar \`${b.file}\``;
    appendJsonl(gotchasFile, {
      session_id: sessionId,
      file: b.file,
      command: (a.cmd || '').slice(0, 120),
      failure_hint: a.output_summary || null,
      description,
      ts: new Date().toISOString()
    });
  }
}

function regenerateMds() {
  // DECISIONS.md
  if (fs.existsSync(decisionsFile)) {
    const decisions = readJsonl(decisionsFile).reverse();
    if (decisions.length > 0) {
      const out = ['# Decisões do projeto', ''];
      for (const d of decisions) {
        const date = (d.ts || '').slice(0, 10);
        out.push(`## ${date} — ${d.what || d.text || '(sem título)'}`);
        if (d.why) { out.push(''); out.push(d.why); }
        out.push('');
      }
      try { fs.writeFileSync(decisionsMd, out.join('\n')); } catch {}
    }
  }

  // GOTCHAS.md — deduplica por (file, command)
  if (fs.existsSync(gotchasFile)) {
    const gotchas = readJsonl(gotchasFile);
    const seen = new Set();
    const unique = [];
    for (const g of gotchas.slice().reverse()) {
      const key = `${g.file}|${(g.command || '').slice(0, 40)}`;
      if (seen.has(key)) continue;
      seen.add(key);
      unique.push(g);
    }
    if (unique.length > 0) {
      const out = ['# Armadilhas conhecidas', ''];
      for (const g of unique) {
        const date = (g.ts || '').slice(0, 10);
        out.push(`## ${date} — \`${g.file}\``);
        out.push('');
        out.push(g.description);
        if (g.failure_hint) { out.push(''); out.push(`_Sintoma:_ ${g.failure_hint}`); }
        out.push('');
      }
      try { fs.writeFileSync(gotchasMd, out.join('\n')); } catch {}
    }
  }

  // GLOSSARY.md — renderiza glossário.jsonl se existir (alimentado manualmente)
  if (fs.existsSync(glossaryFile)) {
    const entries = readJsonl(glossaryFile);
    if (entries.length > 0) {
      const out = ['# Glossário do projeto', ''];
      for (const e of entries) {
        out.push(`## ${e.term}`);
        out.push('');
        out.push(e.definition || '');
        out.push('');
      }
      try { fs.writeFileSync(glossaryMd, out.join('\n')); } catch {}
    }
  }
}
