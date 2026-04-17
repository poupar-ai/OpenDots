'use strict';

// Carrega e aplica best-practices.
// Armazenadas em plugin/best-practices/<id>/practice.md + _catalog.yaml.
// Seeds referenciam por ID em best_practices: [...] no pack.meta.json
// ou no agente individual (agentes.<nome>.best_practices).

const fs   = require('fs');
const path = require('path');

const BP_DIR      = path.join(__dirname, '..', 'best-practices');
const BP_CATALOG  = path.join(BP_DIR, '_catalog.yaml');

// ── YAML mínimo (sem dep externa) ────────────────────────────────────────────
// Parser tolerante só pro formato do catalog — não é YAML completo.

function parseCatalog(text) {
  const lines = text.split('\n');
  const practices = [];
  let current = null;
  let inMulti = null; // nome do campo em modo multi-linha '|'
  let multiBuf = [];
  let multiIndent = 0;

  const flushMulti = () => {
    if (inMulti && current) {
      current[inMulti] = multiBuf.join('\n').trim();
      inMulti = null; multiBuf = []; multiIndent = 0;
    }
  };

  for (const raw of lines) {
    const line = raw.replace(/\r$/, '');
    if (!line.trim() || line.trim().startsWith('#')) continue;

    if (inMulti) {
      // linha de bloco multi-linha (indentada mais que o campo)
      const m = line.match(/^(\s*)(.*)$/);
      if (m && m[1].length >= multiIndent && multiIndent > 0) {
        multiBuf.push(m[2]);
        continue;
      }
      flushMulti();
    }

    const item = line.match(/^\s*-\s+id:\s*(.+)$/);
    if (item) {
      if (current) practices.push(current);
      current = { id: item[1].trim() };
      continue;
    }
    if (!current) continue;

    const kv = line.match(/^\s+(\w+):\s*(.*)$/);
    if (!kv) continue;
    const key = kv[1];
    let val = kv[2];

    if (val === '|') {
      inMulti = key;
      multiBuf = [];
      multiIndent = (line.match(/^\s*/)[0].length) + 2;
      continue;
    }
    if (key === 'tags') {
      const arr = val.match(/^\[(.*)\]$/);
      if (arr) {
        current.tags = arr[1].split(',').map(s => s.trim()).filter(Boolean);
        continue;
      }
    }
    // quote stripping
    if (/^".*"$/.test(val) || /^'.*'$/.test(val)) val = val.slice(1, -1);
    current[key] = val;
  }
  flushMulti();
  if (current) practices.push(current);
  return { practices };
}

function listPractices() {
  if (!fs.existsSync(BP_CATALOG)) return [];
  try {
    const txt = fs.readFileSync(BP_CATALOG, 'utf8');
    const { practices } = parseCatalog(txt);
    return practices.map((p) => ({
      ...p,
      available: fs.existsSync(path.join(BP_DIR, p.dir || p.id, 'practice.md')),
    }));
  } catch {
    return [];
  }
}

function loadPracticeContent(id) {
  const list = listPractices();
  const p = list.find(x => x.id === id);
  if (!p) return null;
  const file = path.join(BP_DIR, p.dir || p.id, 'practice.md');
  if (!fs.existsSync(file)) return null;
  return fs.readFileSync(file, 'utf8');
}

function applyPractices({ ids, memDir, overwrite }) {
  const list = listPractices();
  const byId = Object.fromEntries(list.map(p => [p.id, p]));
  const outDir = path.join(memDir, 'best-practices');
  if (!fs.existsSync(outDir)) fs.mkdirSync(outDir, { recursive: true });

  const copied = [], preserved = [], missing = [];
  for (const id of ids || []) {
    const p = byId[id];
    if (!p) { missing.push(id); continue; }
    const src = path.join(BP_DIR, p.dir || p.id, 'practice.md');
    if (!fs.existsSync(src)) { missing.push(id); continue; }
    const dstDir = path.join(outDir, id);
    if (!fs.existsSync(dstDir)) fs.mkdirSync(dstDir, { recursive: true });
    const dst = path.join(dstDir, 'practice.md');
    if (fs.existsSync(dst) && !overwrite) { preserved.push(id); continue; }
    fs.copyFileSync(src, dst);
    // meta.json pra o runtime saber quando mostrar
    fs.writeFileSync(path.join(dstDir, 'meta.json'), JSON.stringify({
      id: p.id,
      name: p.name,
      scope: p.scope || '',
      tags: p.tags || [],
      when_to_use: p.when_to_use || '',
    }, null, 2) + '\n');
    copied.push(id);
  }
  return { copied, preserved, missing };
}

const ID_RE = /^[a-z][a-z0-9-]{1,48}[a-z0-9]$/;

function listLocalPractices(memDir) {
  const dir = path.join(memDir, 'best-practices');
  if (!fs.existsSync(dir)) return [];
  const out = [];
  for (const entry of fs.readdirSync(dir)) {
    const full = path.join(dir, entry);
    if (!fs.statSync(full).isDirectory()) continue;
    if (!fs.existsSync(path.join(full, 'practice.md'))) continue;
    const metaFile = path.join(full, 'meta.json');
    let meta = { id: entry };
    if (fs.existsSync(metaFile)) {
      try { meta = { ...meta, ...JSON.parse(fs.readFileSync(metaFile, 'utf8')) }; } catch {}
    }
    out.push({
      id: meta.id || entry,
      name: meta.name || meta.id || entry,
      scope: meta.scope || '',
      tags: Array.isArray(meta.tags) ? meta.tags : [],
      when_to_use: meta.when_to_use || '',
      not_for: meta.not_for || '',
      dir: entry,
      source: meta.source || 'local',
      available: true,
    });
  }
  return out;
}

function createLocalPractice({ memDir, input, overwrite = false }) {
  const { id, name, scope, tags, when_to_use, not_for, content } = input || {};
  if (!id || typeof id !== 'string' || !ID_RE.test(id)) {
    throw new Error('id inválido (slug [a-z][a-z0-9-]+[a-z0-9], 3–50 chars)');
  }
  if (!name || typeof name !== 'string') throw new Error('name obrigatório');
  if (typeof content !== 'string' || !content.trim()) throw new Error('content obrigatório');
  if (content.length > 200_000) throw new Error('content muito longo (máx 200KB)');

  const dir = path.join(memDir, 'best-practices', id);
  const practiceFile = path.join(dir, 'practice.md');
  if (fs.existsSync(practiceFile) && !overwrite) {
    throw new Error(`best-practice "${id}" já existe — use overwrite=true pra substituir`);
  }
  fs.mkdirSync(dir, { recursive: true });
  const normalized = content.endsWith('\n') ? content : content + '\n';
  fs.writeFileSync(practiceFile, normalized);

  const meta = {
    id,
    name: String(name).trim(),
    scope: scope ? String(scope).trim() : '',
    tags: Array.isArray(tags) ? tags.filter(t => typeof t === 'string' && t.trim()).map(t => t.trim()) : [],
    when_to_use: when_to_use ? String(when_to_use).trim() : '',
    not_for: not_for ? String(not_for).trim() : '',
    source: 'local',
  };
  fs.writeFileSync(path.join(dir, 'meta.json'), JSON.stringify(meta, null, 2) + '\n');
  return { id, path: practiceFile, meta };
}

module.exports = {
  BP_DIR,
  listPractices,
  listLocalPractices,
  loadPracticeContent,
  applyPractices,
  createLocalPractice,
};
