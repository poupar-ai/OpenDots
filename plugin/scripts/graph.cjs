'use strict';

// Lightweight in-memory graph built incrementally from observations.
// Stored as graph.json in the project's .claude-mem/ directory.
//
// Nodes: files and commands (identified by type+label)
// Edges: co_edited (file+file in same session), followed_by (file→bash), then_edited (bash→file)
//
// Updated on every PostToolUse. Read at SessionStart to enrich context.

const fs = require('fs');
const path = require('path');

const GRAPH_FILE = 'graph.json';
const WINDOW_MS = 15 * 60 * 1000; // 15-minute co-occurrence window

// ─── I/O ────────────────────────────────────────────────────────────────────

function loadGraph(memDir) {
  const file = path.join(memDir, GRAPH_FILE);
  if (!fs.existsSync(file)) return empty();
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch {
    return empty();
  }
}

function saveGraph(memDir, graph) {
  graph.updated_at = new Date().toISOString();
  fs.writeFileSync(path.join(memDir, GRAPH_FILE), JSON.stringify(graph, null, 2));
}

function empty() {
  return { version: 1, nodes: {}, edges: {} };
}

// ─── Node / Edge helpers ────────────────────────────────────────────────────

function obsId(obs) {
  if (obs.type === 'file_edit') return `file:${obs.file}`;
  if (obs.type === 'bash') {
    // Normalize: keep first 3 words so "pytest tests/ -v" → "pytest tests/ -v" stays grouped
    const norm = (obs.cmd || '').trim().replace(/\s+/g, ' ').split(' ').slice(0, 3).join(' ');
    return `cmd:${norm}`;
  }
  return null;
}

function obsLabel(obs) {
  if (obs.type === 'file_edit') return obs.file;
  return (obs.cmd || '').trim().split(' ').slice(0, 4).join(' ');
}

function touchNode(graph, id, label, kind, ts) {
  if (!graph.nodes[id]) graph.nodes[id] = { kind, label, count: 0, last_seen: ts };
  graph.nodes[id].count++;
  graph.nodes[id].last_seen = ts;
}

function touchEdge(graph, fromId, toId, rel, ts) {
  const key = `${fromId}→${toId}→${rel}`;
  if (!graph.edges[key]) graph.edges[key] = { from: fromId, to: toId, rel, count: 0, last_seen: ts };
  graph.edges[key].count++;
  graph.edges[key].last_seen = ts;
}

function sameWindow(ts1, ts2) {
  return Math.abs(new Date(ts1) - new Date(ts2)) <= WINDOW_MS;
}

// ─── Main update ─────────────────────────────────────────────────────────────

/**
 * Update the graph with a new observation.
 * prev = the observation immediately before this one (read from tail of observations.jsonl).
 */
function updateGraph(memDir, current, prev) {
  if (!current) return;
  const curId = obsId(current);
  if (!curId) return;

  const graph = loadGraph(memDir);
  const kind  = current.type === 'file_edit' ? 'file' : 'command';
  touchNode(graph, curId, obsLabel(current), kind, current.ts);

  if (prev && prev.session_id === current.session_id && sameWindow(prev.ts, current.ts)) {
    const prevId = obsId(prev);
    if (prevId && prevId !== curId) {
      if (prev.type === 'file_edit' && current.type === 'file_edit') {
        // Always store both directions so dedup in getContextLines is easy
        touchEdge(graph, prevId, curId, 'co_edited', current.ts);
        touchEdge(graph, curId, prevId, 'co_edited', current.ts);
      } else if (prev.type === 'file_edit' && current.type === 'bash') {
        touchEdge(graph, prevId, curId, 'followed_by', current.ts);
      } else if (prev.type === 'bash' && current.type === 'file_edit') {
        touchEdge(graph, prevId, curId, 'then_edited', current.ts);
      }
    }
  }

  saveGraph(memDir, graph);
}

// ─── Context generation ───────────────────────────────────────────────────────

/**
 * Returns an array of lines ready to be joined with '\n' for Claude context.
 * Only includes patterns that appear more than once (noise filter).
 */
function getContextLines(graph) {
  const lines = [];
  const nodes = Object.values(graph.nodes);
  if (nodes.length === 0) return lines;

  // Top files by edit frequency
  const files = nodes
    .filter(n => n.kind === 'file' && n.count > 1)
    .sort((a, b) => b.count - a.count)
    .slice(0, 8);

  if (files.length > 0) {
    lines.push('### Arquivos mais ativos');
    for (const f of files) {
      lines.push(`- \`${f.label}\` — ${f.count}x`);
    }
    lines.push('');
  }

  // Co-edited pairs (deduplicated: only keep A<B)
  const coEdges = Object.values(graph.edges)
    .filter(e => e.rel === 'co_edited' && e.count > 1 && e.from < e.to)
    .sort((a, b) => b.count - a.count)
    .slice(0, 6);

  if (coEdges.length > 0) {
    lines.push('### Acoplamentos (editados juntos)');
    for (const e of coEdges) {
      const a = e.from.replace(/^file:/, '').split('/').pop();
      const b = e.to.replace(/^file:/, '').split('/').pop();
      lines.push(`- \`${a}\` ↔ \`${b}\` (${e.count}x)`);
    }
    lines.push('');
  }

  // Detected workflows: file_edit → bash (count > 1)
  const workflows = Object.values(graph.edges)
    .filter(e => e.rel === 'followed_by' && e.count > 1)
    .sort((a, b) => b.count - a.count)
    .slice(0, 5);

  if (workflows.length > 0) {
    lines.push('### Workflows detectados (editar → rodar)');
    for (const e of workflows) {
      const file = e.from.replace(/^file:/, '').split('/').pop();
      const cmd  = e.to.replace(/^cmd:/, '');
      lines.push(`- editar \`${file}\` → \`${cmd}\` (${e.count}x)`);
    }
    lines.push('');
  }

  return lines;
}

// ─── Rebuild from scratch ────────────────────────────────────────────────────

/**
 * Rebuild the entire graph.json from observations.jsonl.
 * Use this if graph.json is lost or corrupted.
 */
function rebuildGraph(memDir, readJsonl) {
  const obsFile = path.join(memDir, 'observations.jsonl');
  const observations = readJsonl(obsFile);
  let graph = empty();

  for (let i = 0; i < observations.length; i++) {
    const current = observations[i];
    const prev    = i > 0 ? observations[i - 1] : null;
    const curId   = obsId(current);
    if (!curId) continue;

    const kind = current.type === 'file_edit' ? 'file' : 'command';
    touchNode(graph, curId, obsLabel(current), kind, current.ts);

    if (prev && prev.session_id === current.session_id && sameWindow(prev.ts, current.ts)) {
      const prevId = obsId(prev);
      if (prevId && prevId !== curId) {
        if (prev.type === 'file_edit' && current.type === 'file_edit') {
          touchEdge(graph, prevId, curId, 'co_edited', current.ts);
          touchEdge(graph, curId, prevId, 'co_edited', current.ts);
        } else if (prev.type === 'file_edit' && current.type === 'bash') {
          touchEdge(graph, prevId, curId, 'followed_by', current.ts);
        } else if (prev.type === 'bash' && current.type === 'file_edit') {
          touchEdge(graph, prevId, curId, 'then_edited', current.ts);
        }
      }
    }
  }

  saveGraph(memDir, graph);
  return graph;
}

module.exports = { loadGraph, updateGraph, getContextLines, rebuildGraph };
