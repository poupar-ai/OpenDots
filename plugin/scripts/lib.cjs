'use strict';

const { execSync } = require('child_process');
const fs   = require('fs');
const path = require('path');

const MEM_DIR_NAME     = '.claude-project-memory';
const CLAUDE_MD_START  = '<!-- claude-project-memory:start -->';
const CLAUDE_MD_END    = '<!-- claude-project-memory:end -->';

// ─── Project root ─────────────────────────────────────────────────────────────

function findProjectRoot(startCwd) {
  try {
    return execSync('git rev-parse --show-toplevel', {
      cwd: startCwd,
      stdio: ['pipe', 'pipe', 'pipe'],
      timeout: 3000
    }).toString().trim();
  } catch {
    return startCwd;
  }
}

// ─── Directory helpers ────────────────────────────────────────────────────────

function getMemDir(cwd) {
  return path.join(findProjectRoot(cwd), MEM_DIR_NAME);
}

function getTmpDir(memDir) {
  return path.join(memDir, 'tmp');
}

function ensureMemDir(memDir) {
  if (!fs.existsSync(memDir)) fs.mkdirSync(memDir, { recursive: true });
}

function ensureTmpDir(memDir) {
  const tmp = getTmpDir(memDir);
  if (!fs.existsSync(tmp)) fs.mkdirSync(tmp, { recursive: true });
  return tmp;
}

// ─── JSONL ────────────────────────────────────────────────────────────────────

function readJsonl(file) {
  if (!fs.existsSync(file)) return [];
  return fs.readFileSync(file, 'utf8')
    .split('\n')
    .filter(l => l.trim())
    .map(l => { try { return JSON.parse(l); } catch { return null; } })
    .filter(Boolean);
}

function appendJsonl(file, obj) {
  fs.appendFileSync(file, JSON.stringify(obj) + '\n');
}

// Reads only the last non-empty line of a JSONL file.
// Avoids loading the entire file just to get the most recent entry.
function readLastJsonlLine(file) {
  if (!fs.existsSync(file)) return null;
  const stat = fs.statSync(file);
  if (stat.size === 0) return null;

  // Read from the end in 2KB chunks to find the last line
  const CHUNK = 2048;
  const fd = fs.openSync(file, 'r');
  let pos  = stat.size;
  let buf  = '';

  try {
    while (pos > 0) {
      const readSize = Math.min(CHUNK, pos);
      pos -= readSize;
      const chunk = Buffer.alloc(readSize);
      fs.readSync(fd, chunk, 0, readSize, pos);
      buf = chunk.toString('utf8') + buf;
      const lines = buf.split('\n').filter(l => l.trim());
      if (lines.length >= 1) {
        const last = lines[lines.length - 1];
        try { return JSON.parse(last); } catch { /* keep scanning */ }
      }
    }
  } finally {
    fs.closeSync(fd);
  }
  return null;
}

// ─── Stats cache ──────────────────────────────────────────────────────────────

function loadStats(memDir) {
  const f = path.join(memDir, 'stats.json');
  if (!fs.existsSync(f)) return null;
  try { return JSON.parse(fs.readFileSync(f, 'utf8')); } catch { return null; }
}

function saveStats(memDir, stats) {
  try {
    fs.writeFileSync(path.join(memDir, 'stats.json'), JSON.stringify(stats, null, 2));
  } catch {}
}

function incrementStats(memDir, field) {
  const stats = loadStats(memDir) || { sessions: 0, observations: 0 };
  stats[field] = (stats[field] || 0) + 1;
  saveStats(memDir, stats);
  return stats;
}

// ─── Project config ───────────────────────────────────────────────────────────

function loadProjectConfig(memDir) {
  const f = path.join(memDir, 'config.json');
  if (!fs.existsSync(f)) return null;
  try { return JSON.parse(fs.readFileSync(f, 'utf8')); } catch { return null; }
}

function saveProjectConfig(memDir, cfg) {
  fs.writeFileSync(path.join(memDir, 'config.json'), JSON.stringify(cfg, null, 2) + '\n');
}

function initProjectConfig(memDir, projectName) {
  const existing = loadProjectConfig(memDir);
  if (existing) return existing;
  const cfg = {
    version: '1',
    project: projectName,
    created_at: new Date().toISOString()
  };
  saveProjectConfig(memDir, cfg);
  return cfg;
}

// ─── CLAUDE.md sync ───────────────────────────────────────────────────────────
//
// Syncs PROJECT.md content into .claude/CLAUDE.md at the project root so
// Claude reads project context natively via the CLAUDE.md mechanism,
// independently of hook injection.
//
// Uses marker comments to manage our section without clobbering user content.

function syncClaudeMd(projectRoot, projectMdContent) {
  const claudeDir    = path.join(projectRoot, '.claude');
  const claudeMdPath = path.join(claudeDir, 'CLAUDE.md');

  const block = `${CLAUDE_MD_START}\n${projectMdContent.trim()}\n${CLAUDE_MD_END}`;

  if (!fs.existsSync(claudeMdPath)) {
    fs.mkdirSync(claudeDir, { recursive: true });
    fs.writeFileSync(claudeMdPath, block + '\n');
    return;
  }

  const existing = fs.readFileSync(claudeMdPath, 'utf8');

  if (existing.includes(CLAUDE_MD_START)) {
    // Update our managed section only
    const startIdx = existing.indexOf(CLAUDE_MD_START);
    const endIdx   = existing.indexOf(CLAUDE_MD_END);
    if (endIdx === -1) {
      // Malformed — append fresh block
      fs.writeFileSync(claudeMdPath, existing.trimEnd() + '\n\n' + block + '\n');
    } else {
      const before = existing.slice(0, startIdx);
      const after  = existing.slice(endIdx + CLAUDE_MD_END.length);
      fs.writeFileSync(claudeMdPath, before + block + after);
    }
  } else {
    // Append our section to existing user content
    fs.writeFileSync(claudeMdPath, existing.trimEnd() + '\n\n' + block + '\n');
  }
}

// ─── Stdin / CWD ──────────────────────────────────────────────────────────────

function readStdin() {
  if (process.stdin.isTTY) return {};
  try {
    return JSON.parse(fs.readFileSync(0, 'utf8'));
  } catch {
    return {};
  }
}

function getCwd(input) {
  return (input && input.cwd) || process.env.CLAUDE_CWD || process.env.PWD || process.cwd();
}

module.exports = {
  MEM_DIR_NAME,
  findProjectRoot,
  getMemDir,
  getTmpDir,
  ensureMemDir,
  ensureTmpDir,
  readJsonl,
  appendJsonl,
  readLastJsonlLine,
  loadStats,
  saveStats,
  incrementStats,
  loadProjectConfig,
  saveProjectConfig,
  initProjectConfig,
  syncClaudeMd,
  readStdin,
  getCwd
};
