'use strict';

// Pipelines — fluxos reproduzíveis com checkpoints.
//
// Definições vivem em plugin/pipelines/<id>/pipeline.yaml (templates) e são
// copiadas pra memDir/pipelines/ quando instaladas. Runtime em
// memDir/pipeline-state.json (uma run ativa por projeto por enquanto).
//
// DNA original project-mem: o canvas é o runner. Quando um step fica
// ativo, o agente pisca na tela; checkpoint abre modal no humano owner.

const fs   = require('fs');
const path = require('path');

const PIPE_DIR = path.join(__dirname, '..', 'pipelines');

// ── YAML mínimo (pipeline.yaml) ──────────────────────────────────────────────
// Parser tolerante ao nosso schema específico. Para inline arrays e blocos |.

function parsePipelineYaml(text) {
  const lines = text.split('\n');
  const out = { steps: [] };
  let currentStep = null;
  let block = null;        // { targetKey, indent, buf, owner: out|step }
  let inSteps = false;

  const flushBlock = () => {
    if (!block) return;
    const value = block.buf.join('\n').trim();
    if (block.owner === 'out') out[block.targetKey] = value;
    else if (block.owner === 'step' && currentStep) currentStep[block.targetKey] = value;
    block = null;
  };

  for (const raw of lines) {
    const line = raw.replace(/\r$/, '');
    if (!line.trim()) { if (block) block.buf.push(''); continue; }
    if (line.trim().startsWith('#')) continue;

    if (block) {
      const indent = line.match(/^(\s*)/)[0].length;
      if (indent >= block.indent) {
        block.buf.push(line.slice(block.indent));
        continue;
      }
      flushBlock();
    }

    const stepMatch = line.match(/^\s*-\s+id:\s*(.+)$/);
    if (stepMatch && inSteps) {
      if (currentStep) out.steps.push(currentStep);
      currentStep = { id: stepMatch[1].trim() };
      continue;
    }

    if (/^steps:\s*$/.test(line)) { inSteps = true; continue; }

    const kv = line.match(/^(\s*)(\w+):\s*(.*)$/);
    if (!kv) continue;
    const indent = kv[1].length;
    const key    = kv[2];
    let   val    = kv[3];

    const isStepField = inSteps && currentStep && indent >= 4;
    const target     = isStepField ? currentStep : out;
    const ownerTag   = isStepField ? 'step' : 'out';

    if (val === '|') {
      block = { targetKey: key, indent: indent + 2, buf: [], owner: ownerTag };
      continue;
    }
    const arr = val.match(/^\[(.*)\]$/);
    if (arr) {
      target[key] = arr[1].split(',').map(s => s.trim()).filter(Boolean);
      continue;
    }
    if (/^".*"$/.test(val) || /^'.*'$/.test(val)) val = val.slice(1, -1);
    target[key] = val;
  }
  flushBlock();
  if (currentStep) out.steps.push(currentStep);
  return out;
}

// ── Discovery ────────────────────────────────────────────────────────────────

function listTemplates() {
  if (!fs.existsSync(PIPE_DIR)) return [];
  return fs.readdirSync(PIPE_DIR)
    .filter((d) => {
      const p = path.join(PIPE_DIR, d);
      return fs.statSync(p).isDirectory() && fs.existsSync(path.join(p, 'pipeline.yaml'));
    })
    .map((name) => {
      const file = path.join(PIPE_DIR, name, 'pipeline.yaml');
      try {
        const def = parsePipelineYaml(fs.readFileSync(file, 'utf8'));
        return {
          id: def.id || name,
          label: def.label || name,
          description: def.description || '',
          tags: def.tags || [],
          steps: def.steps || [],
          stepCount: (def.steps || []).length,
          source: 'template',
        };
      } catch {
        return null;
      }
    })
    .filter(Boolean);
}

function listInstalled(memDir) {
  const dir = path.join(memDir, 'pipelines');
  if (!fs.existsSync(dir)) return [];
  return fs.readdirSync(dir)
    .filter((d) => {
      const p = path.join(dir, d);
      return fs.statSync(p).isDirectory() && fs.existsSync(path.join(p, 'pipeline.yaml'));
    })
    .map((name) => {
      const file = path.join(dir, name, 'pipeline.yaml');
      try {
        const def = parsePipelineYaml(fs.readFileSync(file, 'utf8'));
        return {
          id: def.id || name,
          label: def.label || name,
          description: def.description || '',
          tags: def.tags || [],
          steps: def.steps || [],
          stepCount: (def.steps || []).length,
          source: 'installed',
        };
      } catch {
        return null;
      }
    })
    .filter(Boolean);
}

function list(memDir) {
  // installed overrides template com mesmo id
  const templates = listTemplates();
  const installed = listInstalled(memDir);
  const installedIds = new Set(installed.map(p => p.id));
  return [
    ...installed,
    ...templates.filter(t => !installedIds.has(t.id)),
  ];
}

function loadDef(memDir, id) {
  const installedFile = path.join(memDir, 'pipelines', id, 'pipeline.yaml');
  const tplFile       = path.join(PIPE_DIR, id, 'pipeline.yaml');
  const file = fs.existsSync(installedFile) ? installedFile : tplFile;
  if (!fs.existsSync(file)) return null;
  try { return parsePipelineYaml(fs.readFileSync(file, 'utf8')); } catch { return null; }
}

// ── Install ──────────────────────────────────────────────────────────────────

function install({ id, memDir, overwrite }) {
  const src = path.join(PIPE_DIR, id, 'pipeline.yaml');
  if (!fs.existsSync(src)) throw new Error(`pipeline "${id}" não existe em templates`);
  const dstDir = path.join(memDir, 'pipelines', id);
  if (!fs.existsSync(dstDir)) fs.mkdirSync(dstDir, { recursive: true });
  const dst = path.join(dstDir, 'pipeline.yaml');
  if (fs.existsSync(dst) && !overwrite) return { installed: false, reason: 'exists' };
  fs.copyFileSync(src, dst);
  return { installed: true };
}

// ── Create (via UI) ──────────────────────────────────────────────────────────

const ID_RE = /^[a-z][a-z0-9-]{1,48}[a-z0-9]$/;

function yamlEscape(s) {
  if (s === null || s === undefined) return '';
  const str = String(s);
  // Bloco | se tem quebra de linha; senão aspas se tem : [ ] # ou começa com -
  if (str.includes('\n')) return null; // caller handles block form
  if (/[:\[\]#]|^-/.test(str)) return JSON.stringify(str);
  return str;
}

function serializePipeline(def) {
  const lines = [];
  lines.push(`id: ${def.id}`);
  if (def.label) lines.push(`label: ${yamlEscape(def.label) ?? JSON.stringify(def.label)}`);
  if (def.description) {
    if (def.description.includes('\n')) {
      lines.push('description: |');
      for (const l of def.description.split('\n')) lines.push(`  ${l}`);
    } else {
      const esc = yamlEscape(def.description);
      lines.push(`description: ${esc !== null ? esc : JSON.stringify(def.description)}`);
    }
  }
  if (def.tags && def.tags.length) {
    lines.push(`tags: [${def.tags.map(t => t.trim()).filter(Boolean).join(', ')}]`);
  }
  lines.push('steps:');
  for (const s of def.steps) {
    lines.push(`  - id: ${s.id}`);
    if (s.type && s.type !== 'agent') lines.push(`    type: ${s.type}`);
    if (s.agent) lines.push(`    agent: ${s.agent}`);
    if (s.label) {
      const esc = yamlEscape(s.label);
      lines.push(`    label: ${esc !== null ? esc : JSON.stringify(s.label)}`);
    }
    if (s.description) {
      if (s.description.includes('\n')) {
        lines.push('    description: |');
        for (const l of s.description.split('\n')) lines.push(`      ${l}`);
      } else {
        const esc = yamlEscape(s.description);
        lines.push(`    description: ${esc !== null ? esc : JSON.stringify(s.description)}`);
      }
    }
    if (s.owner) lines.push(`    owner: ${s.owner}`);
    if (s.best_practices && s.best_practices.length) {
      lines.push(`    best_practices: [${s.best_practices.map(t => t.trim()).filter(Boolean).join(', ')}]`);
    }
  }
  return lines.join('\n') + '\n';
}

function validatePipelineInput(input) {
  if (!input || typeof input !== 'object') throw new Error('payload inválido');
  const { id, label, steps } = input;
  if (typeof id !== 'string' || !ID_RE.test(id)) {
    throw new Error('id deve ser slug a-z/0-9/- (3-50 chars, começa com letra)');
  }
  if (!label || typeof label !== 'string' || label.length > 120) {
    throw new Error('label é obrigatório (máx 120 chars)');
  }
  if (!Array.isArray(steps) || steps.length === 0) {
    throw new Error('pipeline precisa de pelo menos 1 step');
  }
  const stepIds = new Set();
  for (const s of steps) {
    if (!s || typeof s !== 'object') throw new Error('step inválido');
    if (typeof s.id !== 'string' || !ID_RE.test(s.id)) {
      throw new Error(`step.id "${s.id}" inválido (use slug a-z/0-9/-)`);
    }
    if (stepIds.has(s.id)) throw new Error(`step.id "${s.id}" duplicado`);
    stepIds.add(s.id);
    const type = s.type || 'agent';
    if (!['agent', 'checkpoint'].includes(type)) {
      throw new Error(`step.type inválido em "${s.id}" (use agent ou checkpoint)`);
    }
    if (type === 'agent' && !s.agent) {
      throw new Error(`step "${s.id}" do tipo agent precisa de agent`);
    }
  }
}

function create({ memDir, scope, input, overwrite }) {
  validatePipelineInput(input);
  const scopeName = scope === 'template' ? 'template' : 'project';
  const baseDir = scopeName === 'template'
    ? PIPE_DIR
    : path.join(memDir, 'pipelines');
  const dstDir = path.join(baseDir, input.id);
  const dst = path.join(dstDir, 'pipeline.yaml');
  if (fs.existsSync(dst) && !overwrite) {
    throw new Error(`pipeline "${input.id}" já existe em ${scopeName} — use overwrite`);
  }
  if (!fs.existsSync(dstDir)) fs.mkdirSync(dstDir, { recursive: true });
  fs.writeFileSync(dst, serializePipeline(input));
  return { id: input.id, scope: scopeName, path: dst };
}

// ── State (run) ──────────────────────────────────────────────────────────────

function statePath(memDir) {
  return path.join(memDir, 'pipeline-state.json');
}

function runsPath(memDir) {
  return path.join(memDir, 'pipeline-runs.jsonl');
}

function summarizeRun(state) {
  const doneCount = state.steps.filter(s => s.status === 'done').length;
  const rejectedCount = state.steps.filter(s => s.status === 'rejected').length;
  const startedMs = Date.parse(state.started_at);
  const endedMs = Date.parse(state.updated_at);
  const duration_ms = Number.isFinite(startedMs) && Number.isFinite(endedMs)
    ? Math.max(0, endedMs - startedMs)
    : null;
  return {
    run_id: state.run_id,
    pipeline_id: state.pipeline_id,
    title: state.title,
    status: state.status,
    started_at: state.started_at,
    ended_at: state.updated_at,
    started_by: state.started_by || null,
    total_steps: state.total_steps,
    steps_done: doneCount,
    steps_rejected: rejectedCount,
    duration_ms,
    cancel_reason: state.cancel_reason || null,
  };
}

function appendRun(memDir, state) {
  if (!state || !state.run_id) return;
  if (!fs.existsSync(memDir)) fs.mkdirSync(memDir, { recursive: true });
  const entry = summarizeRun(state);
  fs.appendFileSync(runsPath(memDir), JSON.stringify(entry) + '\n');
}

function listRuns(memDir, limit = 50) {
  const f = runsPath(memDir);
  if (!fs.existsSync(f)) return [];
  const lines = fs.readFileSync(f, 'utf8').split('\n').filter(Boolean);
  const runs = [];
  for (const line of lines) {
    try { runs.push(JSON.parse(line)); } catch { /* skip malformed */ }
  }
  return runs.reverse().slice(0, Math.max(1, Math.min(limit, 500)));
}

function loadState(memDir) {
  const f = statePath(memDir);
  if (!fs.existsSync(f)) return null;
  try { return JSON.parse(fs.readFileSync(f, 'utf8')); } catch { return null; }
}

function saveState(memDir, state) {
  if (!fs.existsSync(memDir)) fs.mkdirSync(memDir, { recursive: true });
  fs.writeFileSync(statePath(memDir), JSON.stringify(state, null, 2) + '\n');
}

function start({ memDir, id, title, startedBy }) {
  const def = loadDef(memDir, id);
  if (!def) throw new Error(`pipeline "${id}" não encontrada`);
  const existing = loadState(memDir);
  if (existing && existing.status === 'running') {
    throw new Error(`já existe uma pipeline rodando: ${existing.pipeline_id}`);
  }
  const now = new Date().toISOString();
  const runId = `${id}-${now.replace(/[:.]/g, '-').slice(0, 19)}`;
  const state = {
    run_id: runId,
    pipeline_id: id,
    title: title || def.label || id,
    status: 'running',
    started_at: now,
    started_by: startedBy || null,
    updated_at: now,
    current_step: 0,
    total_steps: def.steps.length,
    steps: def.steps.map((s, i) => ({
      index: i,
      id: s.id,
      label: s.label || s.id,
      description: s.description || '',
      agent: s.agent || null,
      type: s.type || 'agent',
      owner: s.owner || null,
      best_practices: s.best_practices || [],
      status: i === 0 ? 'active' : 'pending',
      started_at: i === 0 ? now : null,
      finished_at: null,
      note: null,
      decided_by: null,
    })),
  };
  saveState(memDir, state);
  return state;
}

function advance({ memDir, stepId, note, decidedBy, decision }) {
  const state = loadState(memDir);
  if (!state) throw new Error('sem pipeline rodando');
  if (state.status !== 'running') throw new Error(`pipeline não está rodando (status: ${state.status})`);

  const idx = state.steps.findIndex(s => s.id === stepId);
  if (idx === -1) throw new Error(`step "${stepId}" não existe nessa pipeline`);
  if (idx !== state.current_step) {
    throw new Error(`step "${stepId}" não é o atual (atual: ${state.steps[state.current_step].id})`);
  }

  const now = new Date().toISOString();
  const step = state.steps[idx];

  if (decision === 'reject') {
    step.status = 'rejected';
    step.finished_at = now;
    step.note = note || null;
    step.decided_by = decidedBy || null;
    state.status = 'aborted';
    state.updated_at = now;
    saveState(memDir, state);
    appendRun(memDir, state);
    return state;
  }

  step.status = 'done';
  step.finished_at = now;
  step.note = note || null;
  step.decided_by = decidedBy || null;

  const nextIdx = idx + 1;
  if (nextIdx >= state.steps.length) {
    state.status = 'done';
  } else {
    state.current_step = nextIdx;
    state.steps[nextIdx].status = 'active';
    state.steps[nextIdx].started_at = now;
  }
  state.updated_at = now;
  saveState(memDir, state);
  if (state.status === 'done') appendRun(memDir, state);
  return state;
}

function cancel({ memDir, reason }) {
  const state = loadState(memDir);
  if (!state) return null;
  const wasRunning = state.status === 'running';
  state.status = 'cancelled';
  state.updated_at = new Date().toISOString();
  if (reason) state.cancel_reason = reason;
  saveState(memDir, state);
  if (wasRunning) appendRun(memDir, state);
  return state;
}

function clear({ memDir }) {
  const f = statePath(memDir);
  if (fs.existsSync(f)) fs.unlinkSync(f);
}

module.exports = {
  PIPE_DIR,
  listTemplates,
  listInstalled,
  list,
  loadDef,
  install,
  create,
  loadState,
  start,
  advance,
  cancel,
  clear,
  listRuns,
};
