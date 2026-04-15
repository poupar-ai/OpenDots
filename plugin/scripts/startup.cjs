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

// Warn if MCP server is not registered in settings.json
const os = require('os');
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
