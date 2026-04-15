'use strict';

// Fired on SessionEnd.
// Generates a summary of the session and appends it to sessions.jsonl.

const path = require('path');
const fs   = require('fs');
const {
  getMemDir, readJsonl, appendJsonl,
  incrementStats, getTmpDir,
  readStdin, getCwd
} = require('./lib.cjs');

const input     = readStdin();
const sessionId = input.session_id;
const cwd       = getCwd(input);
const memDir    = getMemDir(cwd);

const obsFile = path.join(memDir, 'observations.jsonl');
if (!fs.existsSync(obsFile)) process.exit(0);

const allObs     = readJsonl(obsFile);
const sessionObs = sessionId ? allObs.filter(o => o.session_id === sessionId) : [];

if (sessionObs.length === 0) process.exit(0);

// Deduplicated edited files
const editedFiles = [...new Set(
  sessionObs
    .filter(o => o.type === 'file_edit')
    .map(o => o.file.split('/').pop())
)].slice(0, 8);

// Meaningful bash commands (first 4 words, deduplicated)
const cmds = [...new Set(
  sessionObs
    .filter(o => o.type === 'bash')
    .map(o => o.cmd.split(/\s+/).slice(0, 4).join(' '))
)].slice(0, 5);

const notes = sessionObs
  .filter(o => o.type === 'note')
  .map(o => o.content)
  .slice(0, 3);

const parts = [];
if (editedFiles.length > 0) parts.push(`Editado: ${editedFiles.join(', ')}`);
if (cmds.length > 0)        parts.push(`Rodou: ${cmds.join('; ')}`);
if (notes.length > 0)       parts.push(`Notas: ${notes.join('; ')}`);

const summary = parts.length > 0
  ? parts.join('. ')
  : `${sessionObs.length} operações`;

appendJsonl(path.join(memDir, 'sessions.jsonl'), {
  type: 'end',
  session_id: sessionId,
  ended_at: new Date().toISOString(),
  obs_count: sessionObs.length,
  summary
});

incrementStats(memDir, 'sessions');

// Clean up session flag file from tmp/
if (sessionId) {
  const flagFile = path.join(getTmpDir(memDir), `.init-${sessionId}`);
  try { if (fs.existsSync(flagFile)) fs.unlinkSync(flagFile); } catch {}
}

process.exit(0);
