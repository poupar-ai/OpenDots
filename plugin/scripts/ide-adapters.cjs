'use strict';

// IDE adapters — transforma agentes project-mem na convenção de cada IDE.
//
// DNA original project-mem: fonte única de verdade em .claude-project-memory/,
// adapters GERAM arquivos específicos por IDE a partir dela. Opostamente ao
// modelo de copiar templates separados por IDE — menos drift, sem duplicação.
//
// IDEs suportadas:
//   claude-code     → .claude/agents/<name>.md + .claude/CLAUDE.md (default, já existe)
//   cursor          → .cursor/rules/<name>.mdc + .cursorrules
//   vscode-copilot  → .github/copilot-instructions.md + .github/instructions/
//   codex           → .codex/instructions/<name>.md
//
// Cada adapter expõe: apply({ memDir, projectRoot, manifest, contexts }) → { written: [...] }

const fs   = require('fs');
const path = require('path');

const SUPPORTED = ['claude-code', 'cursor', 'vscode-copilot', 'codex'];

function readManifest(memDir) {
  const f = path.join(memDir, 'agents.json');
  if (!fs.existsSync(f)) return null;
  try { return JSON.parse(fs.readFileSync(f, 'utf8')); } catch { return null; }
}

function readContext(memDir, contextFile) {
  if (!contextFile) return '';
  const f = path.join(memDir, contextFile);
  if (!fs.existsSync(f)) return '';
  return fs.readFileSync(f, 'utf8');
}

function ensureDir(dir) {
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
}

// ── Claude Code (default) ────────────────────────────────────────────────────
// Já é o formato nativo. Aqui só faz um "mirror" explícito em .claude/agents/.

function applyClaudeCode({ memDir, projectRoot, manifest }) {
  const written = [];
  const outDir = path.join(projectRoot, '.claude', 'agents');
  ensureDir(outDir);
  for (const [name, cfg] of Object.entries(manifest.agents || {})) {
    const ctx = readContext(memDir, cfg.context_file);
    const md = [
      `---`,
      `name: ${name}`,
      `description: ${cfg.description || cfg.label || name}`,
      `---`,
      '',
      ctx.trim(),
      '',
    ].join('\n');
    const out = path.join(outDir, `${name}.md`);
    fs.writeFileSync(out, md);
    written.push(path.relative(projectRoot, out));
  }
  return { written };
}

// ── Cursor ───────────────────────────────────────────────────────────────────

function applyCursor({ memDir, projectRoot, manifest }) {
  const written = [];
  const outDir = path.join(projectRoot, '.cursor', 'rules');
  ensureDir(outDir);
  for (const [name, cfg] of Object.entries(manifest.agents || {})) {
    const ctx = readContext(memDir, cfg.context_file);
    const md = [
      `---`,
      `description: ${cfg.description || cfg.label || name}`,
      `globs: ["**/*"]`,
      `alwaysApply: false`,
      `---`,
      ``,
      `# ${cfg.label || name}`,
      ``,
      ctx.trim(),
      ``,
    ].join('\n');
    const out = path.join(outDir, `${name}.mdc`);
    fs.writeFileSync(out, md);
    written.push(path.relative(projectRoot, out));
  }
  // .cursorrules índice
  const idx = [
    `# Agentes disponíveis (project-mem)`,
    ``,
    ...Object.entries(manifest.agents || {}).map(([n, c]) =>
      `- **${n}** — ${c.description || c.label || n}`),
  ].join('\n') + '\n';
  const idxPath = path.join(projectRoot, '.cursorrules');
  fs.writeFileSync(idxPath, idx);
  written.push('.cursorrules');
  return { written };
}

// ── VS Code + Copilot ────────────────────────────────────────────────────────

function applyVscodeCopilot({ memDir, projectRoot, manifest }) {
  const written = [];
  const outDir = path.join(projectRoot, '.github', 'instructions');
  ensureDir(outDir);
  for (const [name, cfg] of Object.entries(manifest.agents || {})) {
    const ctx = readContext(memDir, cfg.context_file);
    const md = [
      `---`,
      `applyTo: "**"`,
      `---`,
      ``,
      `# ${cfg.label || name}`,
      `> ${cfg.description || ''}`,
      ``,
      ctx.trim(),
      ``,
    ].join('\n');
    const out = path.join(outDir, `${name}.instructions.md`);
    fs.writeFileSync(out, md);
    written.push(path.relative(projectRoot, out));
  }
  // índice global
  const mainPath = path.join(projectRoot, '.github', 'copilot-instructions.md');
  ensureDir(path.dirname(mainPath));
  const main = [
    `# Agentes disponíveis (project-mem)`,
    ``,
    `Este projeto usa project-mem. Os agentes abaixo têm arquivo próprio em .github/instructions/:`,
    ``,
    ...Object.entries(manifest.agents || {}).map(([n, c]) =>
      `- **${n}** — ${c.description || c.label || n}`),
    ``,
  ].join('\n');
  fs.writeFileSync(mainPath, main);
  written.push('.github/copilot-instructions.md');
  return { written };
}

// ── Codex ────────────────────────────────────────────────────────────────────

function applyCodex({ memDir, projectRoot, manifest }) {
  const written = [];
  const outDir = path.join(projectRoot, '.codex', 'instructions');
  ensureDir(outDir);
  for (const [name, cfg] of Object.entries(manifest.agents || {})) {
    const ctx = readContext(memDir, cfg.context_file);
    const md = [
      `# ${cfg.label || name}`,
      `> ${cfg.description || ''}`,
      ``,
      ctx.trim(),
      ``,
    ].join('\n');
    const out = path.join(outDir, `${name}.md`);
    fs.writeFileSync(out, md);
    written.push(path.relative(projectRoot, out));
  }
  return { written };
}

// ── Dispatcher ───────────────────────────────────────────────────────────────

function apply({ ide, memDir, projectRoot }) {
  if (!SUPPORTED.includes(ide)) {
    throw new Error(`IDE não suportada: ${ide}. Use: ${SUPPORTED.join(', ')}`);
  }
  const manifest = readManifest(memDir);
  if (!manifest) throw new Error(`agents.json não encontrado em ${memDir}`);

  const ctx = { memDir, projectRoot, manifest };
  switch (ide) {
    case 'claude-code':    return applyClaudeCode(ctx);
    case 'cursor':         return applyCursor(ctx);
    case 'vscode-copilot': return applyVscodeCopilot(ctx);
    case 'codex':          return applyCodex(ctx);
    default:               throw new Error(`adapter ausente: ${ide}`);
  }
}

module.exports = {
  SUPPORTED,
  apply,
};
