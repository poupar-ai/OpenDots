'use strict';

// Integrity watcher — defesa em profundidade contra auto-modificação.
//
// Mesmo com --allowed-tools e --disallowed-tools, um agente determinado pode
// achar um caminho pra editar arquivos do plugin ou configs do projeto
// (ex: Bash patterns que não previmos). Aqui tiramos um snapshot dos arquivos
// críticos ANTES do spawn e comparamos DEPOIS. Se algo mudou, é violação.
//
// Filosofia:
//   - Checar mtime+size primeiro (barato); se mudou, confirmar com sha256.
//   - Lista explícita de arquivos críticos (não walker recursivo) — mais rápido
//     e mais previsível.

const fs     = require('fs');
const path   = require('path');
const crypto = require('crypto');

function pluginRoot() {
  // plugin/scripts/integrity.cjs → sobe dois níveis pra pegar plugin/
  return path.resolve(__dirname, '..');
}

// Arquivos do plugin em si — se agente edita isto, é violação grave.
function pluginCriticalFiles() {
  const root = pluginRoot();
  const rels = [
    'scripts/agents.cjs',
    'scripts/bus.cjs',
    'scripts/queue.cjs',
    'scripts/router.cjs',
    'scripts/sigilo.cjs',
    'scripts/integrity.cjs',
    'scripts/graph.cjs',
    'scripts/lib.cjs',
    'scripts/nudge.cjs',
    'scripts/best-practices.cjs',
    'scripts/pipelines.cjs',
    'scripts/architect.cjs',
    'scripts/skills-install.cjs',
    'scripts/ide-adapters.cjs',
    'scripts/seed.cjs',
    'ui/server.cjs',
  ];
  return rels.map(r => path.join(root, r)).filter(p => fs.existsSync(p));
}

// Arquivos de config do projeto — agente não deve mexer na config de
// orquestração (só no próprio canal de chat).
function projectCriticalFiles(memDir) {
  const rels = [
    'agents.json',
    'bus-config.json',
    'humans.json',
  ];
  return rels.map(r => path.join(memDir, r)).filter(p => fs.existsSync(p));
}

function hashFile(p) {
  try {
    const buf = fs.readFileSync(p);
    return crypto.createHash('sha256').update(buf).digest('hex');
  } catch {
    return null;
  }
}

function statFile(p) {
  try {
    const st = fs.statSync(p);
    return { mtime: st.mtimeMs, size: st.size };
  } catch {
    return null;
  }
}

// Snapshot: retorna Map<absPath, { mtime, size, sha }>.
function snapshot(memDir) {
  const targets = [...pluginCriticalFiles(), ...projectCriticalFiles(memDir)];
  const map = new Map();
  for (const p of targets) {
    const st = statFile(p);
    if (!st) continue;
    map.set(p, { ...st, sha: hashFile(p) });
  }
  return map;
}

// Compara dois snapshots → lista de { path, change, details }.
// change ∈ 'modified' | 'deleted' | 'created'
function diff(before, after) {
  const changes = [];
  const allKeys = new Set([...before.keys(), ...after.keys()]);
  for (const p of allKeys) {
    const b = before.get(p);
    const a = after.get(p);
    if (b && !a) {
      changes.push({ path: p, change: 'deleted' });
      continue;
    }
    if (!b && a) {
      changes.push({ path: p, change: 'created' });
      continue;
    }
    if (!b || !a) continue;
    // Mudança rápida via mtime+size; se divergir, já é suficiente — mas
    // confirmamos com sha pra evitar falso-positivo de `touch`.
    if (b.mtime !== a.mtime || b.size !== a.size) {
      if (b.sha !== a.sha) {
        changes.push({
          path: p, change: 'modified',
          size_delta: a.size - b.size,
        });
      }
    }
  }
  return changes;
}

function classify(absPath) {
  const root = pluginRoot();
  if (absPath.startsWith(root + path.sep)) return 'plugin';
  if (absPath.endsWith('/agents.json')) return 'agents_manifest';
  if (absPath.endsWith('/bus-config.json')) return 'bus_config';
  if (absPath.endsWith('/humans.json')) return 'humans_manifest';
  return 'other';
}

module.exports = {
  snapshot,
  diff,
  classify,
  pluginRoot,
  pluginCriticalFiles,
  projectCriticalFiles,
};
