'use strict';

// Fired async on PostToolUse for Write|Edit|MultiEdit|Bash.
// Records what Claude did into .claude-project-memory/observations.jsonl

const path = require('path');
const {
  getMemDir, ensureMemDir,
  readLastJsonlLine, appendJsonl,
  incrementStats,
  readStdin, getCwd, findProjectRoot
} = require('./lib.cjs');
const { updateGraph } = require('./graph.cjs');

const SKIP_CMDS = /^(ls|ll|la|pwd|echo\s|cat\s|head\s|tail\s|which\s|true|false|clear|history|sleep|printf)\b/;

const input = readStdin();
const { session_id, tool_name, tool_input, tool_response } = input;

if (!tool_name) process.exit(0);

const relevant = new Set(['Write', 'Edit', 'MultiEdit', 'Bash']);
if (!relevant.has(tool_name)) process.exit(0);

if (tool_response && (tool_response.error || tool_response.is_error)) process.exit(0);

const cwd         = getCwd(input);
const projectRoot = findProjectRoot(cwd);
const memDir      = path.join(projectRoot, '.claude-project-memory');
ensureMemDir(memDir);

const ts  = new Date().toISOString();
let obs   = null;

if (tool_name === 'Write' || tool_name === 'Edit' || tool_name === 'MultiEdit') {
  const filePath = tool_input?.file_path || tool_input?.notebook_path || '';
  if (!filePath) process.exit(0);
  const relFile = filePath.startsWith(projectRoot + '/')
    ? filePath.slice(projectRoot.length + 1)
    : filePath;
  obs = { type: 'file_edit', session_id, file: relFile, ts };

} else if (tool_name === 'Bash') {
  const cmd = (tool_input?.command || '').trim();
  if (!cmd || SKIP_CMDS.test(cmd)) process.exit(0);
  const exit = tool_response?.exit_code ?? tool_response?.exitCode ?? tool_response?.returnCode;
  obs = { type: 'bash', session_id, cmd: cmd.slice(0, 200), exit, ts };
}

if (obs) {
  const obsFile = path.join(memDir, 'observations.jsonl');
  const prevObs = readLastJsonlLine(obsFile); // O(tail) instead of O(n)
  appendJsonl(obsFile, obs);
  incrementStats(memDir, 'observations');
  try { updateGraph(memDir, obs, prevObs); } catch {}
}

process.exit(0);
