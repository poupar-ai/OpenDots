'use strict';

const fs = require('fs');
const path = require('path');
const os = require('os');

const settingsPath = path.join(
  process.env.CLAUDE_CONFIG_DIR || path.join(os.homedir(), '.claude'),
  'settings.json'
);

if (!fs.existsSync(settingsPath)) {
  console.log('✗ settings.json não encontrado');
  console.log('  → rode: make install');
  process.exit(0);
}

let settings;
try {
  settings = JSON.parse(fs.readFileSync(settingsPath, 'utf8'));
} catch {
  console.log('✗ settings.json inválido');
  process.exit(1);
}

const hasMarketplace = !!settings.extraKnownMarketplaces?.['project-man'];
const hasPlugin = !!settings.enabledPlugins?.['project-man@project-man'];

if (hasMarketplace && hasPlugin) {
  const dir = settings.extraKnownMarketplaces['project-man']?.source?.path || '?';
  console.log('✓ project-man instalado');
  console.log(`  Diretório: ${dir}`);
  console.log(`  Settings:  ${settingsPath}`);
} else {
  console.log('✗ project-man não instalado (ou instalação incompleta)');
  if (!hasMarketplace) console.log('  → falta: extraKnownMarketplaces["project-man"]');
  if (!hasPlugin)      console.log('  → falta: enabledPlugins["project-man@project-man"]');
  console.log('  → rode: make install');
}
