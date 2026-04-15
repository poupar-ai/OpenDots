'use strict';

// Removes project-man entries from ~/.claude/settings.json.

const fs = require('fs');
const path = require('path');
const os = require('os');

const settingsPath = path.join(
  process.env.CLAUDE_CONFIG_DIR || path.join(os.homedir(), '.claude'),
  'settings.json'
);

if (!fs.existsSync(settingsPath)) {
  console.log('settings.json não encontrado — nada a fazer.');
  process.exit(0);
}

let settings;
try {
  settings = JSON.parse(fs.readFileSync(settingsPath, 'utf8'));
} catch (e) {
  console.error(`Erro ao ler ${settingsPath}: ${e.message}`);
  process.exit(1);
}

let changed = false;

if (settings.extraKnownMarketplaces?.['project-man']) {
  delete settings.extraKnownMarketplaces['project-man'];
  if (Object.keys(settings.extraKnownMarketplaces).length === 0) {
    delete settings.extraKnownMarketplaces;
  }
  changed = true;
}

if (settings.enabledPlugins?.['project-man@project-man'] !== undefined) {
  delete settings.enabledPlugins['project-man@project-man'];
  if (Object.keys(settings.enabledPlugins).length === 0) {
    delete settings.enabledPlugins;
  }
  changed = true;
}

if (settings.mcpServers?.['project-mem']) {
  delete settings.mcpServers['project-mem'];
  if (Object.keys(settings.mcpServers).length === 0) {
    delete settings.mcpServers;
  }
  changed = true;
}

if (!changed) {
  console.log('project-man não estava registrado — nada a fazer.');
  process.exit(0);
}

fs.writeFileSync(settingsPath, JSON.stringify(settings, null, 2) + '\n');
console.log(`✓ project-man removido de: ${settingsPath}`);
