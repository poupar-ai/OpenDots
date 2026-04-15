'use strict';

// Updates ~/.claude/settings.json to register project-man as a local plugin.
// Safe to run multiple times — merges without overwriting existing entries.

const fs = require('fs');
const path = require('path');
const os = require('os');

const installDir = process.argv[2];
if (!installDir) {
  console.error('Usage: node install.cjs <install-dir>');
  process.exit(1);
}

const settingsPath = path.join(
  process.env.CLAUDE_CONFIG_DIR || path.join(os.homedir(), '.claude'),
  'settings.json'
);

// Read existing settings or start fresh
let settings = {};
if (fs.existsSync(settingsPath)) {
  try {
    settings = JSON.parse(fs.readFileSync(settingsPath, 'utf8'));
  } catch (e) {
    console.error(`Erro ao ler ${settingsPath}: ${e.message}`);
    console.error('Verifique se o arquivo é um JSON válido antes de continuar.');
    process.exit(1);
  }
}

// Merge extraKnownMarketplaces
settings.extraKnownMarketplaces = settings.extraKnownMarketplaces || {};
settings.extraKnownMarketplaces['project-man'] = {
  source: {
    source: 'directory',
    path: installDir
  }
};

// Merge enabledPlugins
settings.enabledPlugins = settings.enabledPlugins || {};
settings.enabledPlugins['project-man@project-man'] = true;

// Merge mcpServers
settings.mcpServers = settings.mcpServers || {};
settings.mcpServers['project-mem'] = {
  type: 'stdio',
  command: process.execPath,
  args: [path.join(installDir, 'plugin', 'mcp', 'server.cjs')]
};

// Ensure settings dir exists
fs.mkdirSync(path.dirname(settingsPath), { recursive: true });

// Write back with 2-space indent
fs.writeFileSync(settingsPath, JSON.stringify(settings, null, 2) + '\n');

console.log(`✓ settings.json atualizado: ${settingsPath}`);
console.log(`✓ Plugin registrado em: ${installDir}`);
console.log(`✓ MCP server registrado: project-mem`);
