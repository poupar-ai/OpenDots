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

function getData() {
  const sessionsRaw  = readJsonl(path.join(memDir, 'sessions.jsonl'));
  const observations = readJsonl(path.join(memDir, 'observations.jsonl'));
  const projectMdPath = path.join(memDir, 'PROJECT.md');
  const projectMd = fs.existsSync(projectMdPath)
    ? fs.readFileSync(projectMdPath, 'utf8')
    : null;

  const sessionMap = {};
  for (const s of sessionsRaw) {
    if (!sessionMap[s.session_id]) sessionMap[s.session_id] = {};
    Object.assign(sessionMap[s.session_id], s);
  }
  const sessions = Object.values(sessionMap)
    .filter(s => s.ended_at)
    .sort((a, b) => (b.ended_at || '').localeCompare(a.ended_at || ''));

  return {
    project:      { name: projectName, root: projectRoot },
    projectMd,
    sessions,
    observations: [...observations].reverse(),
    stats: {
      sessions:     sessions.length,
      observations: observations.length,
      hasProjectMd: !!projectMd
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
