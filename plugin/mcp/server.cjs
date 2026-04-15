'use strict';

// MCP server for claude-project-memory.
// Exposes project memory as tools callable by Claude during a conversation.
// Transport: JSON-RPC 2.0 over stdio (newline-delimited).
//
// Tools:
//   mem_context       — PROJECT.md + recent sessions + observations + graph patterns
//   mem_read_project  — PROJECT.md only
//   mem_search        — full-text search over observations and sessions
//   mem_add_note      — append a note to observations.jsonl

const readline = require('readline');
const fs       = require('fs');
const path     = require('path');

// Resolve lib and graph relative to this file so the server works
// regardless of the CWD it is invoked from.
const SCRIPTS = path.join(__dirname, '..', 'scripts');
const { findProjectRoot, readJsonl, appendJsonl } = require(path.join(SCRIPTS, 'lib.cjs'));
const { loadGraph, getContextLines }              = require(path.join(SCRIPTS, 'graph.cjs'));

// ─── Tool definitions ────────────────────────────────────────────────────────

const TOOLS = [
  {
    name: 'mem_context',
    description: 'Returns the full memory context for the current project: PROJECT.md, recent session summaries, recent observations, and detected patterns (graph). Use this at the start of any task to load project context.',
    inputSchema: {
      type: 'object',
      properties: {},
      required: []
    }
  },
  {
    name: 'mem_read_project',
    description: 'Returns the content of .claude-mem/PROJECT.md — permanent project documentation including stack, architecture, conventions, and known decisions.',
    inputSchema: {
      type: 'object',
      properties: {},
      required: []
    }
  },
  {
    name: 'mem_search',
    description: 'Full-text search over observations (file edits, bash commands) and session summaries. Returns matching entries sorted by recency.',
    inputSchema: {
      type: 'object',
      properties: {
        query: {
          type: 'string',
          description: 'Text to search for'
        },
        type: {
          type: 'string',
          enum: ['file', 'bash', 'session'],
          description: 'Restrict search to a specific entry type (optional)'
        },
        limit: {
          type: 'number',
          description: 'Maximum number of results (default: 30)'
        }
      },
      required: ['query']
    }
  },
  {
    name: 'mem_add_note',
    description: 'Appends a free-text note to the project memory. Use to save important decisions, context, or observations that should persist across sessions.',
    inputSchema: {
      type: 'object',
      properties: {
        content: {
          type: 'string',
          description: 'Note content to save'
        }
      },
      required: ['content']
    }
  }
];

// ─── Helpers ─────────────────────────────────────────────────────────────────

function getMemDir() {
  const root = findProjectRoot(process.cwd());
  return path.join(root, '.claude-project-memory');
}

function toolContext() {
  const memDir = getMemDir();
  const lines  = [];

  // PROJECT.md
  const projectMd = path.join(memDir, 'PROJECT.md');
  if (fs.existsSync(projectMd)) {
    const content = fs.readFileSync(projectMd, 'utf8').trim();
    if (content) {
      lines.push('## PROJECT.md');
      lines.push(content);
      lines.push('');
    }
  } else {
    lines.push('## PROJECT.md');
    lines.push('_Not found. Run /mem-setup to create it._');
    lines.push('');
  }

  // Last 5 session summaries
  const sessFile = path.join(memDir, 'sessions.jsonl');
  if (fs.existsSync(sessFile)) {
    const summaries = readJsonl(sessFile)
      .filter(s => s.type === 'end' && s.summary)
      .slice(-5)
      .reverse();
    if (summaries.length > 0) {
      lines.push('## Recent sessions');
      for (const s of summaries) {
        const date = (s.ended_at || '').slice(0, 10);
        lines.push(`- **${date}**: ${s.summary}`);
      }
      lines.push('');
    }
  }

  // Last 30 observations
  const obsFile = path.join(memDir, 'observations.jsonl');
  if (fs.existsSync(obsFile)) {
    const obs = readJsonl(obsFile).slice(-30);
    if (obs.length > 0) {
      lines.push('## Recent observations');
      for (const o of obs) {
        const ts = (o.ts || '').slice(0, 16).replace('T', ' ');
        if (o.type === 'file_edit') lines.push(`- \`${ts}\` EDIT \`${o.file}\``);
        else if (o.type === 'bash')  lines.push(`- \`${ts}\` BASH \`${(o.cmd || '').slice(0, 120)}\``);
        else if (o.type === 'note')  lines.push(`- \`${ts}\` NOTE: ${o.content}`);
      }
      lines.push('');
    }
  }

  // Graph patterns
  try {
    const graph     = loadGraph(memDir);
    const graphLines = getContextLines(graph);
    if (graphLines.length > 0) {
      lines.push('## Detected patterns');
      lines.push(...graphLines);
    }
  } catch {}

  return lines.join('\n') || '_No memory found for this project._';
}

function toolReadProject() {
  const projectMd = path.join(getMemDir(), 'PROJECT.md');
  if (!fs.existsSync(projectMd)) return '_PROJECT.md not found. Run /mem-setup to create it._';
  return fs.readFileSync(projectMd, 'utf8').trim() || '_PROJECT.md is empty._';
}

function toolSearch(args) {
  const query = (args.query || '').toLowerCase();
  const type  = args.type || null;
  const limit = args.limit || 30;

  if (!query) return '_No query provided._';

  const memDir  = getMemDir();
  const results = [];

  // Search sessions
  if (!type || type === 'session') {
    const sessFile = path.join(memDir, 'sessions.jsonl');
    if (fs.existsSync(sessFile)) {
      const sessions = readJsonl(sessFile).filter(s => s.type === 'end' && s.summary);
      for (const s of sessions) {
        if ((s.summary || '').toLowerCase().includes(query)) {
          const date = (s.ended_at || '').slice(0, 10);
          results.push({ ts: s.ended_at, line: `[session ${date}] ${s.summary}` });
        }
      }
    }
  }

  // Search observations
  if (!type || type === 'file' || type === 'bash') {
    const obsFile = path.join(memDir, 'observations.jsonl');
    if (fs.existsSync(obsFile)) {
      const obs = readJsonl(obsFile);
      for (const o of obs) {
        if (type && ((type === 'file' && o.type !== 'file_edit') || (type === 'bash' && o.type !== 'bash'))) continue;
        const hay = `${o.file || ''} ${o.cmd || ''} ${o.content || ''}`.toLowerCase();
        if (hay.includes(query)) {
          const ts = (o.ts || '').slice(0, 16).replace('T', ' ');
          if (o.type === 'file_edit') results.push({ ts: o.ts, line: `[${ts}] EDIT \`${o.file}\`` });
          else if (o.type === 'bash') results.push({ ts: o.ts, line: `[${ts}] BASH \`${(o.cmd || '').slice(0, 120)}\`` });
          else if (o.type === 'note') results.push({ ts: o.ts, line: `[${ts}] NOTE: ${o.content}` });
        }
      }
    }
  }

  if (results.length === 0) return `_No results for "${args.query}"._`;

  results.sort((a, b) => (b.ts || '').localeCompare(a.ts || ''));
  const slice = results.slice(0, limit);
  return `Found ${results.length} result(s) (showing ${slice.length}):\n\n` + slice.map(r => r.line).join('\n');
}

function toolAddNote(args) {
  const content = (args.content || '').trim();
  if (!content) return '_Note content is empty._';

  const memDir  = getMemDir();
  fs.mkdirSync(memDir, { recursive: true });
  appendJsonl(path.join(memDir, 'observations.jsonl'), {
    type: 'note',
    content,
    ts: new Date().toISOString()
  });
  return `Note saved: "${content}"`;
}

// ─── JSON-RPC dispatch ────────────────────────────────────────────────────────

function respond(id, result) {
  process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id, result }) + '\n');
}

function error(id, code, message) {
  process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id, error: { code, message } }) + '\n');
}

function handle(msg) {
  const { id, method, params } = msg;

  if (method === 'initialize') {
    return respond(id, {
      protocolVersion: '2024-11-05',
      capabilities: { tools: {} },
      serverInfo: { name: 'claude-project-memory', version: '1.0.0' }
    });
  }

  if (method === 'notifications/initialized') return; // no response needed

  if (method === 'tools/list') {
    return respond(id, { tools: TOOLS });
  }

  if (method === 'tools/call') {
    const name = params?.name;
    const args = params?.arguments || {};
    let text;
    try {
      if (name === 'mem_context')      text = toolContext();
      else if (name === 'mem_read_project') text = toolReadProject();
      else if (name === 'mem_search')  text = toolSearch(args);
      else if (name === 'mem_add_note') text = toolAddNote(args);
      else return error(id, -32601, `Unknown tool: ${name}`);
    } catch (e) {
      text = `Error: ${e.message}`;
    }
    return respond(id, { content: [{ type: 'text', text }] });
  }

  // Unknown method — return error only if it has an id (requests, not notifications)
  if (id !== undefined && id !== null) {
    error(id, -32601, `Method not found: ${method}`);
  }
}

// ─── Stdio loop ───────────────────────────────────────────────────────────────

const rl = readline.createInterface({ input: process.stdin, terminal: false });

rl.on('line', line => {
  const trimmed = line.trim();
  if (!trimmed) return;
  let msg;
  try { msg = JSON.parse(trimmed); } catch { return; }
  handle(msg);
});

rl.on('close', () => process.exit(0));
