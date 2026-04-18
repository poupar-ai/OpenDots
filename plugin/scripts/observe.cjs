'use strict';

// Fired async on PostToolUse for Write|Edit|MultiEdit|Bash.
// Records what Claude did into .claude-project-memory/observations.jsonl

const path = require('path');
const fs   = require('fs');
const { execFileSync } = require('child_process');
const {
  getMemDir, ensureMemDir,
  readLastJsonlLine, appendJsonl,
  incrementStats,
  readStdin, getCwd, findProjectRoot
} = require('./lib.cjs');
const { updateGraph } = require('./graph.cjs');

const SKIP_CMDS = /^(ls|ll|la|pwd|echo\s|cat\s|head\s|tail\s|which\s|true|false|clear|history|sleep|printf)\b/;

// ── Diff capture ─────────────────────────────────────────────────────────────
// Enriquece file_edit com sinal do que mudou (insertions/deletions + preview do
// diff). Busca via git contra HEAD — captura o estado cumulativo do arquivo
// nesta sessão. Silencioso se git não está disponível ou o arquivo é binário.

const DIFF_PREVIEW_MAX = 240;

function captureDiff(projectRoot, relFile) {
  try {
    const numstat = execFileSync('git', ['diff', 'HEAD', '--numstat', '--', relFile], {
      cwd: projectRoot, timeout: 1500, stdio: ['pipe', 'pipe', 'pipe']
    }).toString().trim();

    if (numstat) {
      const parts = numstat.split(/\s+/);
      const ins = parseInt(parts[0], 10);
      const del = parseInt(parts[1], 10);
      if (Number.isNaN(ins) || Number.isNaN(del)) {
        return { tracked: true, binary: true };
      }
      const preview = diffPreview(projectRoot, relFile);
      return { tracked: true, ins, del, preview };
    }

    // Sem diff contra HEAD → ou arquivo untracked ou idêntico ao HEAD.
    const abs = path.join(projectRoot, relFile);
    if (fs.existsSync(abs)) {
      const size = fs.statSync(abs).size;
      const lines = size > 0 ? fs.readFileSync(abs, 'utf8').split('\n').length : 0;
      return { tracked: false, ins: lines, del: 0 };
    }
    return null;
  } catch {
    return null;
  }
}

// ── Bash output parser (#9) ──────────────────────────────────────────────────
// Extrai resumo útil do stdout/stderr para comandos de teste/lint/typecheck.
// Mantém ~60 chars como `output_summary` na observação.

function summarizeOutput(cmd, tool_response) {
  if (!tool_response) return null;
  const raw = [
    tool_response.stdout, tool_response.output, tool_response.content,
    tool_response.stderr, tool_response.result
  ].filter(Boolean).join('\n');
  if (!raw) return null;
  const out = String(raw).slice(-4000);
  const c = cmd.toLowerCase();

  if (/\bpytest\b/.test(c)) {
    const m = out.match(/(\d+)\s+passed(?:,\s*(\d+)\s+failed)?(?:,\s*(\d+)\s+error)?/);
    if (m) return `pytest: ${m[1]} passed${m[2] ? `, ${m[2]} failed` : ''}${m[3] ? `, ${m[3]} errors` : ''}`;
  }
  if (/(npm|yarn|pnpm|bun)\s+(test|run\s+test)|\bjest\b|\bvitest\b/.test(c)) {
    const m = out.match(/Tests?:?\s+(?:(\d+)\s+failed,\s*)?(\d+)\s+passed/i);
    if (m) return `tests: ${m[2]} passed${m[1] ? `, ${m[1]} failed` : ''}`;
  }
  if (/\bcargo\s+(test|nextest)\b/.test(c)) {
    const m = out.match(/(\d+)\s+passed[;,]\s*(\d+)\s+failed/i);
    if (m) return `cargo: ${m[1]} passed, ${m[2]} failed`;
  }
  if (/\bruff\b/.test(c)) {
    if (/all checks passed/i.test(out)) return 'ruff: ok';
    const m = out.match(/found\s+(\d+)\s+error/i);
    if (m) return `ruff: ${m[1]} errors`;
  }
  if (/\bmypy\b/.test(c)) {
    if (/success:\s*no issues/i.test(out)) return 'mypy: ok';
    const m = out.match(/found\s+(\d+)\s+error/i);
    if (m) return `mypy: ${m[1]} errors`;
  }
  if (/\btsc\b|\btypecheck\b|type-check/.test(c)) {
    const errors = (out.match(/error\s+TS\d+/g) || []).length;
    if (errors > 0) return `tsc: ${errors} errors`;
    if (/\b0\s+errors?\b|no errors? found/i.test(out)) return 'tsc: ok';
  }
  if (/\beslint\b/.test(c)) {
    const m = out.match(/(\d+)\s+problems?\s*\((\d+)\s+errors?,\s*(\d+)\s+warnings?\)/i);
    if (m) return `eslint: ${m[2]} err, ${m[3]} warn`;
    if (/^\s*$/.test(out)) return 'eslint: ok';
  }

  // Fallback: última linha não vazia, truncada
  const lastLine = out.split('\n').map(l => l.trim()).reverse().find(l => l);
  if (lastLine && lastLine.length < 180) return lastLine;
  return null;
}

function diffPreview(projectRoot, relFile) {
  try {
    const diff = execFileSync('git', ['diff', 'HEAD', '--unified=0', '--', relFile], {
      cwd: projectRoot, timeout: 1500, stdio: ['pipe', 'pipe', 'pipe'],
      maxBuffer: 256 * 1024
    }).toString();
    if (!diff) return null;
    // Mantém só linhas +/-, ignorando headers de hunk e metadata.
    const body = diff.split('\n')
      .filter(l => (l.startsWith('+') || l.startsWith('-')) && !l.startsWith('+++') && !l.startsWith('---'))
      .join('\n');
    const sliced = body.slice(0, DIFF_PREVIEW_MAX);
    return sliced.length < body.length ? sliced + '…' : sliced;
  } catch {
    return null;
  }
}

const input = readStdin();
const { session_id, tool_name, tool_input, tool_response } = input;

if (!tool_name) process.exit(0);

const relevant = new Set(['Write', 'Edit', 'MultiEdit', 'Bash', 'Task', 'Agent']);
if (!relevant.has(tool_name)) process.exit(0);

if (tool_response && (tool_response.error || tool_response.is_error)) process.exit(0);

const cwd         = getCwd(input);
const projectRoot = findProjectRoot(cwd);
const memDir      = path.join(projectRoot, '.claude-project-memory');
ensureMemDir(memDir);

const ts  = new Date().toISOString();
const agentId = process.env.CLAUDE_PROJECT_MEM_AGENT || null;
let obs   = null;

if (tool_name === 'Write' || tool_name === 'Edit' || tool_name === 'MultiEdit') {
  const filePath = tool_input?.file_path || tool_input?.notebook_path || '';
  if (!filePath) process.exit(0);
  const relFile = filePath.startsWith(projectRoot + '/')
    ? filePath.slice(projectRoot.length + 1)
    : filePath;
  obs = { type: 'file_edit', session_id, file: relFile, ts };
  if (agentId) obs.agent_id = agentId;
  const diff = captureDiff(projectRoot, relFile);
  if (diff) obs.diff = diff;

} else if (tool_name === 'Bash') {
  const cmd = (tool_input?.command || '').trim();
  if (!cmd || SKIP_CMDS.test(cmd)) process.exit(0);
  const exit = tool_response?.exit_code ?? tool_response?.exitCode ?? tool_response?.returnCode;
  obs = { type: 'bash', session_id, cmd: cmd.slice(0, 200), exit, ts };
  if (agentId) obs.agent_id = agentId;
  const summary = summarizeOutput(cmd, tool_response);
  if (summary) obs.output_summary = summary;

} else if (tool_name === 'Task' || tool_name === 'Agent') {
  const agent = (tool_input?.subagent_type || 'general-purpose').toLowerCase();
  const desc  = (tool_input?.description || '').slice(0, 120);
  const prompt = (tool_input?.prompt || '').slice(0, 200);
  obs = { type: 'agent_call', session_id, agent, description: desc, prompt, ts };
  if (agentId) obs.agent_id = agentId;
}

if (obs) {
  const obsFile = path.join(memDir, 'observations.jsonl');
  const prevObs = readLastJsonlLine(obsFile); // O(tail) instead of O(n)
  appendJsonl(obsFile, obs);
  incrementStats(memDir, 'observations');
  try { updateGraph(memDir, obs, prevObs); } catch {}
}

process.exit(0);
