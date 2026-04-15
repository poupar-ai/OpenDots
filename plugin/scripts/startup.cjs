'use strict';

// Fired on SessionStart (startup | clear | compact).
// Shows a status banner. Uses stats.json cache to avoid reading full JSONL files.

const path = require('path');
const fs   = require('fs');
const { findProjectRoot, getMemDir, loadStats, readJsonl } = require('./lib.cjs');

const cwd         = process.env.CLAUDE_CWD || process.env.PWD || process.cwd();
const projectRoot = findProjectRoot(cwd);
const projectName = path.basename(projectRoot);
const memDir      = getMemDir(cwd);
const PORT        = process.env.PROJECT_MEM_PORT || 37778;

let line = `🧠 project-mem | ${projectName}`;

if (!fs.existsSync(memDir)) {
  line += ' | sem memória ainda — rode /mem-setup para iniciar';
} else {
  // Use stats.json cache when available to avoid reading full JSONL files
  let sessions     = 0;
  let observations = 0;

  const stats = loadStats(memDir);
  if (stats) {
    sessions     = stats.sessions     || 0;
    observations = stats.observations || 0;
  } else {
    // Fallback: count from JSONL (only on first run before stats.json exists)
    sessions     = readJsonl(path.join(memDir, 'sessions.jsonl')).filter(s => s.type === 'end').length;
    observations = readJsonl(path.join(memDir, 'observations.jsonl')).length;
  }

  const hasProjectMd = fs.existsSync(path.join(memDir, 'PROJECT.md'));

  line += ` | ${sessions} sessões · ${observations} obs`;
  line += hasProjectMd ? ' | PROJECT.md ✓' : ' | PROJECT.md ✗ — rode /mem-setup';
  line += ` | UI → http://localhost:${PORT}`;
}

console.log(JSON.stringify({ continue: true, systemMessage: line }));
