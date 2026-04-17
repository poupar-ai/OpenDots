'use strict';

// Fired on SessionStart (startup | clear | compact).
// Shows a status banner. Uses stats.json cache to avoid reading full JSONL files.
// Also ensures the UI server is running (spawns detached if not).

const path         = require('path');
const fs           = require('fs');
const os           = require('os');
const { spawn }    = require('child_process');
const { findProjectRoot, getMemDir, loadStats, readJsonl } = require('./lib.cjs');

const cwd         = process.env.CLAUDE_CWD || process.env.PWD || process.cwd();
const projectRoot = findProjectRoot(cwd);
const projectName = path.basename(projectRoot);
const memDir      = getMemDir(cwd);
const PORT        = parseInt(process.env.PROJECT_MEM_PORT || '37778', 10);

function ensureUiRunning() {
  // Auto-start da UI. Usa lockfile com PID no tmpdir para evitar múltiplos servers.
  const pluginRoot = process.env.CLAUDE_PLUGIN_ROOT || path.join(__dirname, '..');
  const serverPath = path.join(pluginRoot, 'ui', 'server.cjs');
  if (!fs.existsSync(serverPath)) return;

  const lockPath = path.join(os.tmpdir(), `project-mem-ui-${PORT}.pid`);

  // Se já há um PID vivo no lock, assume que o server está rodando.
  try {
    const existingPid = parseInt(fs.readFileSync(lockPath, 'utf8').trim(), 10);
    if (existingPid) {
      process.kill(existingPid, 0); // sinal 0 = checa se o processo existe
      return;
    }
  } catch {
    // Lock ausente ou PID morto — segue para spawn.
  }

  let out = 'ignore';
  try {
    out = fs.openSync(path.join(os.tmpdir(), 'project-mem-ui.log'), 'a');
  } catch {}

  const child = spawn(process.execPath, [serverPath], {
    cwd,
    env: { ...process.env, PROJECT_MEM_PORT: String(PORT) },
    detached: true,
    stdio: ['ignore', out, out],
  });
  child.unref();

  try { fs.writeFileSync(lockPath, String(child.pid)); } catch {}
}

try { ensureUiRunning(); } catch {}

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

  if (stats && stats.last_injection_tokens) {
    const budget  = stats.last_injection_budget || 4000;
    const skipped = Array.isArray(stats.last_injection_skipped) ? stats.last_injection_skipped.length : 0;
    line += ` | ~${stats.last_injection_tokens}/${budget}t${skipped > 0 ? ` (⚠${skipped} omitidos)` : ''}`;
  }

  line += ` | UI → http://localhost:${PORT}`;
}

// Warn if MCP server is not registered in settings.json
try {
  const settingsPath = path.join(
    process.env.CLAUDE_CONFIG_DIR || path.join(os.homedir(), '.claude'),
    'settings.json'
  );
  const settings = JSON.parse(fs.readFileSync(settingsPath, 'utf8'));
  if (!settings.mcpServers?.['project-mem']) {
    const pluginRoot = process.env.CLAUDE_PLUGIN_ROOT || path.join(__dirname, '..');
    const mcpPath    = path.join(pluginRoot, 'mcp', 'server.cjs');
    line += ` | ⚠ MCP não registrado — adicione em settings.json: {"mcpServers":{"project-mem":{"type":"stdio","command":"node","args":["${mcpPath}"]}}}`;
  }
} catch {}

console.log(JSON.stringify({ continue: true, systemMessage: line }));
