#!/usr/bin/env node
'use strict';

// CLI para recriar agentes-template em qualquer projeto.
//
// Packs vivem em plugin/seeds/<categoria>/<pack>/{agents.json, agents/*.md}
// Metadados: plugin/seeds/<categoria>/meta.json e <categoria>/<pack>/meta.json.
//
// Uso:
//   node seed.cjs setup                            # wizard interativo
//   node seed.cjs --list
//   node seed.cjs <categoria>/<pack> [flags]
//
// Flags:
//   --project <path>     projeto-alvo (default: git root do CWD)
//   --only a,b,c         cherry-pick agentes
//   --exclude x,y        excluir agentes
//   --overwrite          sobrescreve agentes/contexts existentes

const fs       = require('fs');
const path     = require('path');
const readline = require('readline');

const { findProjectRoot, MEM_DIR_NAME } = require('./lib.cjs');
const bp = require('./best-practices.cjs');
const skillsInstall = require('./skills-install.cjs');
const ideAdapters   = require('./ide-adapters.cjs');

const SEEDS_DIR = path.join(__dirname, '..', 'seeds');

// ── Args ─────────────────────────────────────────────────────────────────────

function parseArgs(argv) {
  const args = { flags: {}, pos: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--list' || a === '-l') { args.flags.list = true; continue; }
    if (a === '--overwrite' || a === '-f') { args.flags.overwrite = true; continue; }
    if (a === '--help' || a === '-h') { args.flags.help = true; continue; }
    if (a === '--project' || a === '-p') { args.flags.project = argv[++i]; continue; }
    if (a === '--only')    { args.flags.only    = argv[++i]; continue; }
    if (a === '--exclude') { args.flags.exclude = argv[++i]; continue; }
    if (a.startsWith('--')) { console.error(`flag desconhecida: ${a}`); process.exit(2); }
    args.pos.push(a);
  }
  return args;
}

function usage() {
  console.log(`project-mem seed — agentes-template pra qualquer projeto.

Uso:
  seed.cjs setup                              # wizard interativo
  seed.cjs --list                             # listar categorias/packs
  seed.cjs <categoria>/<pack> [flags]

Flags:
  --project <path>        projeto-alvo (default: git root do CWD)
  --only a,b,c            cherry-pick agentes
  --exclude x,y           excluir agentes
  --overwrite             sobrescreve o que já existe

Seeds em: ${SEEDS_DIR}`);
}

// ── Pack discovery ───────────────────────────────────────────────────────────

function readMeta(dir) {
  const p = path.join(dir, 'meta.json');
  if (!fs.existsSync(p)) return {};
  try { return JSON.parse(fs.readFileSync(p, 'utf8')); } catch { return {}; }
}

function listCategories() {
  if (!fs.existsSync(SEEDS_DIR)) return [];
  return fs.readdirSync(SEEDS_DIR)
    .filter((d) => {
      const p = path.join(SEEDS_DIR, d);
      return fs.statSync(p).isDirectory();
    })
    .map((name) => ({ name, ...readMeta(path.join(SEEDS_DIR, name)) }));
}

function listPacks(category) {
  const catDir = path.join(SEEDS_DIR, category);
  if (!fs.existsSync(catDir)) return [];
  return fs.readdirSync(catDir)
    .filter((d) => {
      const p = path.join(catDir, d);
      if (!fs.statSync(p).isDirectory()) return false;
      return fs.existsSync(path.join(p, 'agents.json'));
    })
    .map((name) => {
      const dir = path.join(catDir, name);
      const meta = readMeta(dir);
      const manifest = JSON.parse(fs.readFileSync(path.join(dir, 'agents.json'), 'utf8'));
      return {
        name,
        dir,
        meta,
        manifest,
        agentNames: Object.keys(manifest.agents || {}),
      };
    });
}

function loadPackRef(ref) {
  const [category, pack] = String(ref).split('/');
  if (!category || !pack) throw new Error(`referência inválida: "${ref}". Use <categoria>/<pack>.`);
  const dir = path.join(SEEDS_DIR, category, pack);
  if (!fs.existsSync(dir)) throw new Error(`pack "${ref}" não existe em ${SEEDS_DIR}`);
  const manifestPath = path.join(dir, 'agents.json');
  if (!fs.existsSync(manifestPath)) throw new Error(`pack "${ref}" sem agents.json`);
  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
  return { category, pack, dir, manifest, meta: readMeta(dir) };
}

// ── Filter ───────────────────────────────────────────────────────────────────

function filterAgents(agents, onlyStr, excludeStr) {
  const only    = (onlyStr    || '').split(',').map((s) => s.trim()).filter(Boolean);
  const exclude = (excludeStr || '').split(',').map((s) => s.trim()).filter(Boolean);
  const out = {};
  for (const [name, cfg] of Object.entries(agents)) {
    if (only.length   && !only.includes(name))   continue;
    if (exclude.length &&  exclude.includes(name)) continue;
    out[name] = cfg;
  }
  return out;
}

// ── Apply ────────────────────────────────────────────────────────────────────

function apply({ ref, projectRoot, only, exclude, overwrite, skills, ide }) {
  const { dir: packDir, manifest: src } = loadPackRef(ref);

  const memDir = path.join(projectRoot, MEM_DIR_NAME);
  if (!fs.existsSync(memDir)) fs.mkdirSync(memDir, { recursive: true });

  const agentsDir = path.join(memDir, 'agents');
  if (!fs.existsSync(agentsDir)) fs.mkdirSync(agentsDir, { recursive: true });

  const targetManifestPath = path.join(memDir, 'agents.json');
  let target;
  if (fs.existsSync(targetManifestPath)) {
    target = JSON.parse(fs.readFileSync(targetManifestPath, 'utf8'));
  } else {
    target = { version: 1, manager: src.manager || 'manager', oracle: src.oracle || 'oracle', rule: src.rule, agents: {} };
  }
  if (!target.agents) target.agents = {};

  const srcAgents = filterAgents(src.agents || {}, only, exclude);

  const added = [], skipped = [], replaced = [], mdCopied = [], mdSkipped = [];

  for (const [name, cfg] of Object.entries(srcAgents)) {
    const exists = !!target.agents[name];
    if (exists && !overwrite) {
      skipped.push(name);
    } else {
      target.agents[name] = cfg;
      if (exists) replaced.push(name); else added.push(name);
    }
    const rel = cfg.context_file || `agents/${name}.md`;
    const srcMd = path.join(packDir, rel);
    const dstMd = path.join(memDir, rel);
    if (fs.existsSync(srcMd)) {
      const dstDir = path.dirname(dstMd);
      if (!fs.existsSync(dstDir)) fs.mkdirSync(dstDir, { recursive: true });
      if (!fs.existsSync(dstMd) || overwrite) {
        fs.copyFileSync(srcMd, dstMd);
        mdCopied.push(rel);
      } else {
        mdSkipped.push(rel);
      }
    }
  }

  if (!target.manager && src.manager) target.manager = src.manager;
  if (!target.oracle  && src.oracle)  target.oracle  = src.oracle;
  if (!target.rule    && src.rule)    target.rule    = src.rule;

  // Rastreia origem dos agentes pro UI saber onde propagar edições (ex: autonomia).
  // Formato: _meta.agent_origin[<agente>] = { category, slug }
  const [seedCategory, seedSlug] = (ref || '').split('/');
  if (seedCategory && seedSlug) {
    if (!target._meta) target._meta = {};
    if (!target._meta.agent_origin) target._meta.agent_origin = {};
    for (const name of Object.keys(srcAgents)) {
      target._meta.agent_origin[name] = { category: seedCategory, slug: seedSlug };
    }
  }

  fs.writeFileSync(targetManifestPath, JSON.stringify(target, null, 2) + '\n');

  // Best-practices — pack-level (meta.best_practices) + agent-level (cfg.best_practices).
  const packMeta = readMeta(packDir);
  const practiceIds = new Set([...(packMeta.best_practices || [])]);
  for (const cfg of Object.values(srcAgents)) {
    for (const id of cfg.best_practices || []) practiceIds.add(id);
  }
  const practicesReport = practiceIds.size
    ? bp.applyPractices({ ids: [...practiceIds], memDir, overwrite })
    : { copied: [], preserved: [], missing: [] };

  // Skills — opt-in. Se `skills` é lista (mesmo vazia), aplica filtro; se
  // true, aplica todas; se undefined/false, pula.
  let skillsReport = { copied: [], preserved: [], mcp_added: [], mcp_preserved: [] };
  if (skills) {
    const [category, pack] = ref.split('/');
    const names = Array.isArray(skills) ? skills : null;
    skillsReport = skillsInstall.applySkills({
      category, pack, names, memDir, projectRoot, overwrite,
    });
  }

  // IDE adapter — opt-in. `ide` é string: claude-code | cursor | vscode-copilot | codex
  let ideReport = null;
  if (ide) {
    ideReport = ideAdapters.apply({ ide, memDir, projectRoot });
  }

  return {
    added, replaced, skipped, mdCopied, mdSkipped, memDir,
    practices: practicesReport,
    skills:    skillsReport,
    ide:       ideReport,
  };
}

// ── Pretty --list ────────────────────────────────────────────────────────────

function printList() {
  const cats = listCategories();
  if (cats.length === 0) { console.log('(nenhuma categoria em ' + SEEDS_DIR + ')'); return; }
  for (const c of cats) {
    const label = c.label ? `${c.label} — ` : '';
    console.log(`\n▸ ${label}${c.name}${c.description ? `\n  ${c.description}` : ''}`);
    const packs = listPacks(c.name);
    for (const p of packs) {
      const lbl = p.meta.label ? ` — ${p.meta.label}` : '';
      console.log(`  • ${c.name}/${p.name}${lbl}  (${p.agentNames.length} agentes)`);
      if (p.meta.description) {
        console.log(`      ${p.meta.description}`);
      }
      console.log(`      ${p.agentNames.join(', ')}`);
    }
  }
}

function printResult(ref, r) {
  console.log(`✓ pack "${ref}" aplicado em ${r.memDir}`);
  if (r.added.length)     console.log(`  + adicionados: ${r.added.join(', ')}`);
  if (r.replaced.length)  console.log(`  ↻ substituídos: ${r.replaced.join(', ')}`);
  if (r.skipped.length)   console.log(`  = existentes (use --overwrite): ${r.skipped.join(', ')}`);
  if (r.mdCopied.length)  console.log(`  📄 contextos copiados: ${r.mdCopied.length}`);
  if (r.mdSkipped.length) console.log(`  = contextos preservados: ${r.mdSkipped.length}`);
  if (r.practices?.copied?.length)    console.log(`  📚 práticas aplicadas: ${r.practices.copied.join(', ')}`);
  if (r.practices?.preserved?.length) console.log(`  = práticas preservadas: ${r.practices.preserved.join(', ')}`);
  if (r.practices?.missing?.length)   console.log(`  ! práticas não encontradas: ${r.practices.missing.join(', ')}`);
}

// ── Wizard ───────────────────────────────────────────────────────────────────

function prompt(rl, q, def) {
  const suffix = def ? ` [${def}]` : '';
  return new Promise((resolve) => {
    rl.question(`${q}${suffix}: `, (ans) => resolve((ans || '').trim() || def || ''));
  });
}

async function runWizard() {
  const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
  try {
    console.log('\n🧠 project-mem — setup de agentes\n');

    const cats = listCategories().filter((c) => listPacks(c.name).length > 0);
    if (cats.length === 0) {
      console.log('Nenhum pack disponível em ' + SEEDS_DIR);
      return;
    }

    console.log('Categorias disponíveis:');
    cats.forEach((c, i) => {
      console.log(`  ${i + 1}. ${c.label || c.name}${c.description ? ` — ${c.description}` : ''}`);
    });
    const catPick = await prompt(rl, '\nEscolha a categoria (número)', '1');
    const catIdx = Math.max(1, Math.min(cats.length, parseInt(catPick, 10) || 1)) - 1;
    const category = cats[catIdx];

    const packs = listPacks(category.name);
    console.log(`\nPacks em "${category.label || category.name}":`);
    packs.forEach((p, i) => {
      const lbl = p.meta.label || p.name;
      console.log(`  ${i + 1}. ${lbl}  (${p.agentNames.length} agentes)`);
      if (p.meta.description) console.log(`     ${p.meta.description}`);
    });
    const packPick = await prompt(rl, '\nEscolha o pack (número)', '1');
    const packIdx = Math.max(1, Math.min(packs.length, parseInt(packPick, 10) || 1)) - 1;
    const pack = packs[packIdx];

    console.log(`\nAgentes no pack "${pack.meta.label || pack.name}":`);
    pack.agentNames.forEach((n, i) => console.log(`  ${i + 1}. ${n}`));
    const filterAns = await prompt(
      rl,
      '\nFiltrar? [enter=todos, "a,b,c"=só esses, "-x,y"=excluir esses]',
      '',
    );
    let only = '', exclude = '';
    if (filterAns.startsWith('-')) exclude = filterAns.slice(1);
    else if (filterAns)            only    = filterAns;

    const projectAns = await prompt(rl, 'Projeto-alvo', findProjectRoot(process.cwd()));
    const projectRoot = path.resolve(projectAns);
    if (!fs.existsSync(projectRoot)) {
      console.error(`\nprojeto não existe: ${projectRoot}`);
      return;
    }

    const overwriteAns = await prompt(rl, 'Sobrescrever agentes existentes? (s/N)', 'n');
    const overwrite = /^s(im)?$/i.test(overwriteAns);

    console.log('\nResumo:');
    console.log(`  pack:       ${category.name}/${pack.name}`);
    console.log(`  projeto:    ${projectRoot}`);
    if (only)       console.log(`  só esses:   ${only}`);
    if (exclude)    console.log(`  excluir:    ${exclude}`);
    console.log(`  overwrite:  ${overwrite ? 'sim' : 'não'}`);

    const confirm = await prompt(rl, '\nConfirmar? (S/n)', 's');
    if (!/^s(im)?$/i.test(confirm)) { console.log('Cancelado.'); return; }

    const r = apply({
      ref: `${category.name}/${pack.name}`,
      projectRoot,
      only,
      exclude,
      overwrite,
    });
    console.log('');
    printResult(`${category.name}/${pack.name}`, r);
  } finally {
    rl.close();
  }
}

// ── Main ─────────────────────────────────────────────────────────────────────

async function main() {
  const args = parseArgs(process.argv.slice(2));

  if (args.flags.help) { usage(); return; }
  if (args.flags.list) { printList(); return; }
  if (args.pos[0] === 'setup') { await runWizard(); return; }

  if (args.pos.length === 0) { usage(); process.exit(1); }

  const ref = args.pos[0];
  const projectRoot = args.flags.project
    ? path.resolve(args.flags.project)
    : findProjectRoot(process.cwd());
  if (!fs.existsSync(projectRoot)) {
    console.error(`projeto não existe: ${projectRoot}`);
    process.exit(1);
  }
  const r = apply({
    ref,
    projectRoot,
    only: args.flags.only,
    exclude: args.flags.exclude,
    overwrite: !!args.flags.overwrite,
  });
  printResult(ref, r);
}

if (require.main === module) {
  main().catch((e) => { console.error('erro:', e.message); process.exit(1); });
}

module.exports = { listCategories, listPacks, loadPackRef, apply, SEEDS_DIR };
