'use strict';

const http = require('http');
const fs   = require('fs');
const path = require('path');
const { findProjectRoot, getMemDir, readJsonl } = require('../scripts/lib.cjs');

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

  return {
    project:      { name: projectName, root: projectRoot },
    soulMd,
    projectMd,
    decisionsMd,
    gotchasMd,
    glossaryMd,
    codeIndexMd,
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

const server = http.createServer((req, res) => {
  const url = new URL(req.url, `http://localhost:${PORT}`);

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
