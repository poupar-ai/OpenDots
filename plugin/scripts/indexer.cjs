'use strict';

// Code index no padrão que a indústria de IA converge (Aider repo-map,
// Cursor, Sourcegraph): extração de símbolos + ranking por importância +
// atualização incremental + injeção dentro do orçamento de tokens.
//
// - Extração regex por linguagem (py, ts/tsx, js/jsx, rs, sql) — sem tree-sitter
//   pra manter o plugin dependency-free. A fidelidade é boa o suficiente pra
//   ranking; quando não é, o Claude grep no arquivo real.
// - Ranking = hot_file_weight (graph.cjs) × log(1+refs) × recência.
// - Armazenado como code_index.json (consulta) + CODE_INDEX.md (legível).
// - Incremental: session-end passa só os arquivos editados; rebuild completo
//   via skill /mem-index.

const fs   = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');
const { estimateTokens } = require('./lib.cjs');

const INDEX_JSON = 'code_index.json';
const INDEX_MD   = 'CODE_INDEX.md';

const MAX_FILE_BYTES = 512 * 1024; // pula arquivos muito grandes
const MAX_FILES      = 2000;       // teto geral de segurança

// ── Linguagens ────────────────────────────────────────────────────────────────

const LANGS = {
  '.py':   'python',
  '.pyi':  'python',
  '.ts':   'typescript',
  '.tsx':  'typescript',
  '.js':   'javascript',
  '.jsx':  'javascript',
  '.mjs':  'javascript',
  '.cjs':  'javascript',
  '.rs':   'rust',
  '.sql':  'sql',
  '.go':   'go'
};

function detectLang(file) {
  const ext = path.extname(file).toLowerCase();
  return LANGS[ext] || null;
}

// ── Extratores ────────────────────────────────────────────────────────────────

function extractPython(content) {
  const out = [];
  const lines = content.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];

    // async def foo(args) -> Return:
    let m = /^(\s*)(async\s+)?def\s+(\w+)\s*\(([^)]*)\)(?:\s*->\s*([^:]+))?\s*:/.exec(l);
    if (m) {
      const indent = m[1].length;
      if (indent === 0) {
        out.push({ type: m[2] ? 'async-fn' : 'fn', name: m[3], sig: `def ${m[3]}(${compactArgs(m[4])})${m[5] ? ' -> ' + m[5].trim() : ''}`, line: i + 1 });
      }
      continue;
    }

    // class Foo(Base):
    m = /^(\s*)class\s+(\w+)(?:\s*\(([^)]*)\))?\s*:/.exec(l);
    if (m && m[1].length === 0) {
      out.push({ type: 'class', name: m[2], sig: `class ${m[2]}${m[3] ? '(' + m[3].trim() + ')' : ''}`, line: i + 1 });
      continue;
    }

    // FastAPI: @router.get("/path") logo antes de def
    m = /^\s*@(\w+)\.(get|post|put|delete|patch|websocket)\s*\(\s*["']([^"']+)["']/.exec(l);
    if (m) {
      const next = (lines[i + 1] || '') + (lines[i + 2] || '');
      const fnMatch = /def\s+(\w+)/.exec(next);
      if (fnMatch) {
        out.push({ type: 'route', name: fnMatch[1], sig: `${m[2].toUpperCase()} ${m[3]}`, line: i + 1 });
      }
    }
  }
  return out;
}

function extractTs(content) {
  const out = [];
  const lines = content.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];

    // export (async) function name(args)
    let m = /^export\s+(default\s+)?(async\s+)?function\s+(\w+)\s*\(([^)]*)\)/.exec(l);
    if (m) {
      out.push({ type: m[2] ? 'async-fn' : 'fn', name: m[3], sig: `function ${m[3]}(${compactArgs(m[4])})`, line: i + 1, exported: true });
      continue;
    }

    // export const name = (args) => ... | export const name = async (...) => | export const name: Type =
    m = /^export\s+(default\s+)?(const|let|var)\s+(\w+)\s*(?::\s*[^=]+)?=/.exec(l);
    if (m) {
      const rest = l.slice(m[0].length);
      const arrow = /^\s*(async\s+)?\(([^)]*)\)\s*(?::[^=]+)?=>/.exec(rest);
      if (arrow) {
        out.push({ type: arrow[1] ? 'async-fn' : 'fn', name: m[3], sig: `${m[3]} = (${compactArgs(arrow[2])}) =>`, line: i + 1, exported: true });
      } else {
        out.push({ type: 'const', name: m[3], sig: `const ${m[3]}`, line: i + 1, exported: true });
      }
      continue;
    }

    // export class Name
    m = /^export\s+(default\s+)?(abstract\s+)?class\s+(\w+)(?:\s+extends\s+(\w+))?/.exec(l);
    if (m) {
      out.push({ type: 'class', name: m[3], sig: `class ${m[3]}${m[4] ? ' extends ' + m[4] : ''}`, line: i + 1, exported: true });
      continue;
    }

    // export interface / type / enum
    m = /^export\s+(interface|type|enum)\s+(\w+)/.exec(l);
    if (m) {
      out.push({ type: m[1], name: m[2], sig: `${m[1]} ${m[2]}`, line: i + 1, exported: true });
      continue;
    }

    // App Router: export async function GET/POST/PUT/DELETE/PATCH
    m = /^export\s+(async\s+)?function\s+(GET|POST|PUT|DELETE|PATCH|OPTIONS|HEAD)\s*\(/.exec(l);
    if (m) {
      out[out.length - 1] = { ...out[out.length - 1], type: 'route' };
    }
  }
  return out;
}

function extractRust(content) {
  const out = [];
  const lines = content.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];

    let m = /^(\s*)(pub\s+(?:\([^)]+\)\s+)?)?(async\s+)?fn\s+(\w+)\s*(?:<[^>]*>)?\s*\(([^)]*)\)/.exec(l);
    if (m && m[1].length === 0) {
      const exported = !!m[2];
      out.push({ type: m[3] ? 'async-fn' : 'fn', name: m[4], sig: `fn ${m[4]}(${compactArgs(m[5])})`, line: i + 1, exported });
      continue;
    }
    m = /^(pub\s+)?struct\s+(\w+)/.exec(l);
    if (m) { out.push({ type: 'struct', name: m[2], sig: `struct ${m[2]}`, line: i + 1, exported: !!m[1] }); continue; }
    m = /^(pub\s+)?enum\s+(\w+)/.exec(l);
    if (m) { out.push({ type: 'enum', name: m[2], sig: `enum ${m[2]}`, line: i + 1, exported: !!m[1] }); continue; }
    m = /^(pub\s+)?trait\s+(\w+)/.exec(l);
    if (m) { out.push({ type: 'trait', name: m[2], sig: `trait ${m[2]}`, line: i + 1, exported: !!m[1] }); continue; }
  }
  return out;
}

function extractSql(content) {
  const out = [];
  const lines = content.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    let m = /create\s+table\s+(?:if\s+not\s+exists\s+)?["']?(\w+)["']?/i.exec(l);
    if (m) { out.push({ type: 'table', name: m[1], sig: `TABLE ${m[1]}`, line: i + 1 }); continue; }
    m = /create\s+type\s+(\w+)/i.exec(l);
    if (m) { out.push({ type: 'type', name: m[1], sig: `TYPE ${m[1]}`, line: i + 1 }); continue; }
    m = /create\s+(?:or\s+replace\s+)?(?:unique\s+)?index\s+(?:if\s+not\s+exists\s+)?(\w+)/i.exec(l);
    if (m) { out.push({ type: 'index', name: m[1], sig: `INDEX ${m[1]}`, line: i + 1 }); }
  }
  return out;
}

function extractGo(content) {
  const out = [];
  const lines = content.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    let m = /^func\s+(?:\([^)]+\)\s+)?([A-Z]\w*|[a-z]\w*)\s*\(/.exec(l);
    if (m) { out.push({ type: 'fn', name: m[1], sig: `func ${m[1]}`, line: i + 1, exported: /^[A-Z]/.test(m[1]) }); continue; }
    m = /^type\s+(\w+)\s+(struct|interface)/.exec(l);
    if (m) { out.push({ type: m[2], name: m[1], sig: `${m[2]} ${m[1]}`, line: i + 1, exported: /^[A-Z]/.test(m[1]) }); }
  }
  return out;
}

const EXTRACTORS = {
  python:     extractPython,
  typescript: extractTs,
  javascript: extractTs,
  rust:       extractRust,
  sql:        extractSql,
  go:         extractGo
};

function compactArgs(args) {
  if (!args) return '';
  const s = args.replace(/\s+/g, ' ').trim();
  return s.length > 60 ? s.slice(0, 57) + '...' : s;
}

// ── Leitura do repo ───────────────────────────────────────────────────────────

function listTrackedFiles(projectRoot) {
  try {
    const out = execFileSync('git', ['ls-files'], {
      cwd: projectRoot, timeout: 5000, stdio: ['pipe', 'pipe', 'pipe'], maxBuffer: 10 * 1024 * 1024
    }).toString();
    return out.split('\n').filter(Boolean);
  } catch {
    return [];
  }
}

function extractFile(projectRoot, relPath) {
  const lang = detectLang(relPath);
  if (!lang) return null;
  const fn = EXTRACTORS[lang];
  if (!fn) return null;

  const abs = path.join(projectRoot, relPath);
  let stat;
  try { stat = fs.statSync(abs); } catch { return null; }
  if (!stat.isFile() || stat.size > MAX_FILE_BYTES) return null;

  let content;
  try { content = fs.readFileSync(abs, 'utf8'); }
  catch { return null; }

  const symbols = fn(content);
  return {
    file: relPath,
    lang,
    mtime: stat.mtimeMs,
    symbols
  };
}

// ── Reference counting (custo baixo via regex de palavra) ─────────────────────

function countReferences(projectRoot, files, symbolsByFile) {
  // Coleta nomes únicos com ocorrências externas (símbolos "públicos")
  const uniqueNames = new Set();
  for (const entries of Object.values(symbolsByFile)) {
    for (const s of entries) {
      if (s.name.length < 3) continue;              // 2-char symbols ruim pra contar
      if (/^(new|get|set|is|to|of|on|in|as|at|do|up|or|if|_)/.test(s.name)) continue;
      if (s.exported !== false) uniqueNames.add(s.name);
    }
  }

  // Constrói contador — percorre arquivos uma vez, aplica regexes concatenadas
  const refs = Object.fromEntries([...uniqueNames].map(n => [n, 0]));
  const nameList = [...uniqueNames];
  if (nameList.length === 0) return refs;

  // Dividir em chunks de 200 nomes para regexes viáveis
  const chunkSize = 200;
  const patterns = [];
  for (let i = 0; i < nameList.length; i += chunkSize) {
    const chunk = nameList.slice(i, i + chunkSize).map(escapeRegex);
    patterns.push(new RegExp(`\\b(${chunk.join('|')})\\b`, 'g'));
  }

  for (const relPath of files) {
    const abs = path.join(projectRoot, relPath);
    let content;
    try {
      const st = fs.statSync(abs);
      if (st.size > MAX_FILE_BYTES) continue;
      content = fs.readFileSync(abs, 'utf8');
    } catch { continue; }

    // Subtrai definições do próprio arquivo
    const ownSymbols = new Set((symbolsByFile[relPath] || []).map(s => s.name));

    for (const re of patterns) {
      re.lastIndex = 0;
      let m;
      while ((m = re.exec(content)) !== null) {
        const name = m[1];
        if (ownSymbols.has(name)) continue;
        refs[name] = (refs[name] || 0) + 1;
      }
    }
  }

  return refs;
}

function escapeRegex(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

// ── Rebuild completo ──────────────────────────────────────────────────────────

function rebuildIndex(memDir, projectRoot, { verbose = false } = {}) {
  const tracked = listTrackedFiles(projectRoot);
  if (tracked.length === 0) return { files: {}, generated_at: new Date().toISOString() };

  const files = tracked.filter(f => LANGS[path.extname(f).toLowerCase()]).slice(0, MAX_FILES);
  const symbolsByFile = {};
  const meta = {};

  for (const f of files) {
    const entry = extractFile(projectRoot, f);
    if (!entry) continue;
    symbolsByFile[f] = entry.symbols;
    meta[f] = { lang: entry.lang, mtime: entry.mtime };
  }

  const refs = countReferences(projectRoot, tracked, symbolsByFile);

  const index = {
    version: 1,
    generated_at: new Date().toISOString(),
    files: {}
  };
  for (const [f, symbols] of Object.entries(symbolsByFile)) {
    index.files[f] = {
      lang:    meta[f].lang,
      mtime:   meta[f].mtime,
      symbols: symbols.map(s => ({ ...s, refs: refs[s.name] || 0 }))
    };
  }

  saveIndex(memDir, index);
  return index;
}

// ── Update incremental ────────────────────────────────────────────────────────

function updateIndexForFiles(memDir, projectRoot, changedFiles) {
  let index = loadIndex(memDir);
  if (!index) return rebuildIndex(memDir, projectRoot);

  for (const f of changedFiles) {
    const abs = path.join(projectRoot, f);
    if (!fs.existsSync(abs)) { delete index.files[f]; continue; }
    const entry = extractFile(projectRoot, f);
    if (!entry) { delete index.files[f]; continue; }
    index.files[f] = {
      lang:    entry.lang,
      mtime:   entry.mtime,
      symbols: entry.symbols.map(s => ({ ...s, refs: 0 })) // refs recalculados no próximo rebuild
    };
  }

  index.generated_at = new Date().toISOString();
  saveIndex(memDir, index);
  return index;
}

// ── I/O ───────────────────────────────────────────────────────────────────────

function loadIndex(memDir) {
  const file = path.join(memDir, INDEX_JSON);
  if (!fs.existsSync(file)) return null;
  try { return JSON.parse(fs.readFileSync(file, 'utf8')); }
  catch { return null; }
}

function saveIndex(memDir, index) {
  fs.writeFileSync(path.join(memDir, INDEX_JSON), JSON.stringify(index, null, 2));
  writeIndexMd(memDir, index);
}

// ── Ranking + renderização ────────────────────────────────────────────────────
//
// Ranking estilo Aider (simplificado):
//   score = hot_file_weight × 0.5
//         + log2(1 + refs)   × 0.3
//         + recency_decay    × 0.2
//
// hot_files vem do graph.cjs (arquivos mais editados).
// refs vem do countReferences (quantas vezes o símbolo aparece em outros arquivos).
// recency usa mtime do arquivo.

function rankSymbols(index, graph) {
  const now = Date.now();
  const DAY = 86400000;

  // Mapa file → count do grafo
  const hotCount = {};
  if (graph && graph.nodes) {
    for (const n of Object.values(graph.nodes)) {
      if (n.kind === 'file') hotCount[n.label] = n.count;
    }
  }
  const maxHot = Math.max(1, ...Object.values(hotCount));

  const maxMtime = Math.max(1, ...Object.values(index.files).map(f => f.mtime || 0));

  const all = [];
  for (const [file, entry] of Object.entries(index.files)) {
    const hot = (hotCount[file] || 0) / maxHot;
    const ageDays = Math.max(0, (now - (entry.mtime || now)) / DAY);
    const recency = Math.exp(-ageDays / 30); // meia-vida ~30 dias

    for (const s of entry.symbols) {
      const refsScore = Math.log2(1 + (s.refs || 0)) / 6; // normaliza 0..1 até ~64 refs
      const score = (hot * 0.5) + (refsScore * 0.3) + (recency * 0.2);
      all.push({ file, lang: entry.lang, score, ...s });
    }
  }

  all.sort((a, b) => b.score - a.score);
  return all;
}

function writeIndexMd(memDir, index) {
  const graphFile = path.join(memDir, 'graph.json');
  let graph = null;
  try { graph = JSON.parse(fs.readFileSync(graphFile, 'utf8')); } catch {}

  const ranked = rankSymbols(index, graph);
  const totalSymbols = ranked.length;
  const totalFiles   = Object.keys(index.files).length;

  const byLang = {};
  for (const r of ranked) {
    byLang[r.lang] = (byLang[r.lang] || 0) + 1;
  }

  const out = [
    '# Code Index',
    `> Gerado em: ${index.generated_at}`,
    `> ${totalFiles} arquivos · ${totalSymbols} símbolos · linguagens: ${Object.entries(byLang).map(([l, n]) => `${l} (${n})`).join(', ')}`,
    ''
  ];

  // Top 40 símbolos
  out.push('## Top símbolos (por importância)');
  out.push('');
  for (const s of ranked.slice(0, 40)) {
    const refsStr = s.refs ? ` · ${s.refs} refs` : '';
    out.push(`- **\`${s.name}\`** (${s.type}) — \`${s.file}:${s.line}\`${refsStr}`);
    if (s.sig && s.sig !== s.name) out.push(`  \`${s.sig}\``);
  }
  out.push('');

  // Por linguagem (resumo)
  const byFile = {};
  for (const r of ranked) {
    (byFile[r.file] = byFile[r.file] || []).push(r);
  }
  const topFiles = Object.entries(byFile)
    .sort((a, b) => b[1].length - a[1].length)
    .slice(0, 15);

  out.push('## Arquivos mais densos');
  out.push('');
  for (const [file, syms] of topFiles) {
    out.push(`### \`${file}\` _(${syms.length} símbolos)_`);
    for (const s of syms.slice(0, 8)) {
      out.push(`- \`${s.sig || s.name}\` — L${s.line}`);
    }
    if (syms.length > 8) out.push(`- _... mais ${syms.length - 8}_`);
    out.push('');
  }

  try { fs.writeFileSync(path.join(memDir, INDEX_MD), out.join('\n')); } catch {}
}

// ── Bloco injetável (para session-init) ───────────────────────────────────────
//
// Retorna linhas prontas pra virar um bloco de contexto. Só mostra símbolos de
// arquivos "quentes" (top 10 do grafo) pra manter tokens baixos.

function getContextLines(memDir, maxSymbols = 25) {
  const index = loadIndex(memDir);
  if (!index || Object.keys(index.files).length === 0) return [];

  const graphFile = path.join(memDir, 'graph.json');
  let graph = null;
  try { graph = JSON.parse(fs.readFileSync(graphFile, 'utf8')); } catch {}

  const hotFiles = new Set();
  if (graph && graph.nodes) {
    const files = Object.values(graph.nodes)
      .filter(n => n.kind === 'file')
      .sort((a, b) => b.count - a.count)
      .slice(0, 10)
      .map(n => n.label);
    for (const f of files) hotFiles.add(f);
  }

  const ranked = rankSymbols(index, graph)
    .filter(s => hotFiles.size === 0 || hotFiles.has(s.file))
    .slice(0, maxSymbols);

  if (ranked.length === 0) return [];

  const lines = ['## Code Index (símbolos reutilizáveis)'];
  for (const s of ranked) {
    const refsStr = s.refs ? ` _${s.refs}↑_` : '';
    lines.push(`- \`${s.name}\` ${s.type} — \`${s.file}:${s.line}\`${refsStr}`);
  }
  return lines;
}

module.exports = {
  rebuildIndex,
  updateIndexForFiles,
  loadIndex,
  getContextLines,
  rankSymbols,
  detectLang,
  EXTRACTORS
};
