'use strict';

// Skills — integrações instaláveis com MCP opcional.
//
// Cada skill vive dentro de um seed pack em:
//   plugin/seeds/<cat>/<pack>/skills/<name>/SKILL.md
//   plugin/seeds/<cat>/<pack>/skills/<name>/mcp.json      (opcional)
//
// Install copia SKILL.md pra memDir/skills/<name>/ e faz MERGE do mcp.json
// no .mcp.json do projeto (não sobrescreve entradas existentes sem flag).
//
// DNA original project-mem: skills vêm com acesso à memória — o SKILL.md
// pode referenciar observações e decisões do projeto, não são integrações
// cegas como em outros frameworks.

const fs   = require('fs');
const path = require('path');

const SEEDS_DIR = path.join(__dirname, '..', 'seeds');

function listSkillsInPack(category, pack) {
  const dir = path.join(SEEDS_DIR, category, pack, 'skills');
  if (!fs.existsSync(dir)) return [];
  return fs.readdirSync(dir)
    .filter((d) => {
      const p = path.join(dir, d);
      return fs.statSync(p).isDirectory() && fs.existsSync(path.join(p, 'SKILL.md'));
    })
    .map((name) => {
      const sdir = path.join(dir, name);
      const skillMd = fs.readFileSync(path.join(sdir, 'SKILL.md'), 'utf8');
      const mcpPath = path.join(sdir, 'mcp.json');
      const fm = parseFrontmatter(skillMd);
      return {
        name,
        description: fm.description || '',
        scope: fm.scope || '',
        version: fm.version || '',
        requires_mcp: !!fm.requires_mcp,
        env: fm.env || [],
        has_mcp: fs.existsSync(mcpPath),
      };
    });
}

function parseFrontmatter(md) {
  const m = md.match(/^---\n([\s\S]*?)\n---/);
  if (!m) return {};
  const out = {};
  for (const line of m[1].split('\n')) {
    const kv = line.match(/^(\w+):\s*(.*)$/);
    if (!kv) continue;
    let [, k, v] = kv;
    if (v === 'true') out[k] = true;
    else if (v === 'false') out[k] = false;
    else if (/^\[.*\]$/.test(v)) out[k] = v.slice(1, -1).split(',').map(s => s.trim()).filter(Boolean);
    else if (/^".*"$/.test(v) || /^'.*'$/.test(v)) out[k] = v.slice(1, -1);
    else out[k] = v;
  }
  return out;
}

function applySkills({ category, pack, names, memDir, projectRoot, overwrite }) {
  const srcDir = path.join(SEEDS_DIR, category, pack, 'skills');
  if (!fs.existsSync(srcDir)) return { copied: [], preserved: [], mcp_added: [], mcp_preserved: [] };

  const outDir = path.join(memDir, 'skills');
  if (!fs.existsSync(outDir)) fs.mkdirSync(outDir, { recursive: true });

  const copied = [], preserved = [];
  const mcpAdded = [], mcpPreserved = [];

  const all = listSkillsInPack(category, pack);
  const pick = names && names.length
    ? all.filter(s => names.includes(s.name))
    : all;

  // merge .mcp.json at project root (create if missing)
  const mcpProjectPath = path.join(projectRoot, '.mcp.json');
  let mcpRoot = { mcpServers: {} };
  if (fs.existsSync(mcpProjectPath)) {
    try { mcpRoot = JSON.parse(fs.readFileSync(mcpProjectPath, 'utf8')); } catch { /* keep default */ }
    if (!mcpRoot.mcpServers) mcpRoot.mcpServers = {};
  }

  for (const skill of pick) {
    const sSrc = path.join(srcDir, skill.name, 'SKILL.md');
    const sDstDir = path.join(outDir, skill.name);
    if (!fs.existsSync(sDstDir)) fs.mkdirSync(sDstDir, { recursive: true });
    const sDst = path.join(sDstDir, 'SKILL.md');
    if (fs.existsSync(sDst) && !overwrite) {
      preserved.push(skill.name);
    } else {
      fs.copyFileSync(sSrc, sDst);
      copied.push(skill.name);
    }

    const mcpSrc = path.join(srcDir, skill.name, 'mcp.json');
    if (fs.existsSync(mcpSrc)) {
      // copy ref too
      fs.copyFileSync(mcpSrc, path.join(sDstDir, 'mcp.json'));
      try {
        const entry = JSON.parse(fs.readFileSync(mcpSrc, 'utf8'));
        if (mcpRoot.mcpServers[skill.name] && !overwrite) {
          mcpPreserved.push(skill.name);
        } else {
          mcpRoot.mcpServers[skill.name] = entry;
          mcpAdded.push(skill.name);
        }
      } catch { /* skip malformed */ }
    }
  }

  // persist .mcp.json only if we had any mcp change to avoid creating empty file
  if (mcpAdded.length > 0) {
    fs.writeFileSync(mcpProjectPath, JSON.stringify(mcpRoot, null, 2) + '\n');
  }

  return { copied, preserved, mcp_added: mcpAdded, mcp_preserved: mcpPreserved };
}

module.exports = {
  listSkillsInPack,
  applySkills,
};
