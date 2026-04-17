export type PipelineStepType = 'agent' | 'checkpoint';
export type PipelineStepStatus = 'pending' | 'active' | 'done' | 'rejected';
export type PipelineStatus = 'running' | 'done' | 'aborted' | 'cancelled';

export interface PipelineStepDef {
  id: string;
  label?: string;
  description?: string;
  agent?: string;
  type?: PipelineStepType;
  owner?: string;
  best_practices?: string[];
}

export interface PipelineDef {
  id: string;
  label: string;
  description?: string;
  tags?: string[];
  steps: PipelineStepDef[];
  stepCount: number;
  source: 'template' | 'installed';
}

export interface PipelineStepState {
  index: number;
  id: string;
  label: string;
  description: string;
  agent: string | null;
  type: PipelineStepType;
  owner: string | null;
  best_practices: string[];
  status: PipelineStepStatus;
  started_at: string | null;
  finished_at: string | null;
  note: string | null;
  decided_by: string | null;
}

export interface PipelineRunState {
  run_id: string;
  pipeline_id: string;
  title: string;
  status: PipelineStatus;
  started_at: string;
  started_by: string | null;
  updated_at: string;
  current_step: number;
  total_steps: number;
  steps: PipelineStepState[];
  cancel_reason?: string;
}

export async function listPipelines(): Promise<{ pipelines: PipelineDef[] }> {
  const r = await fetch('/api/pipelines');
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function installPipeline(id: string, overwrite = false): Promise<{ ok: boolean; installed: boolean; reason?: string }> {
  const r = await fetch('/api/pipelines/install', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ id, overwrite }),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
  return j;
}

export async function getPipelineState(): Promise<{ state: PipelineRunState | null }> {
  const r = await fetch('/api/pipelines/state');
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function startPipeline(input: { id: string; title?: string; started_by?: string | null }): Promise<{ state: PipelineRunState }> {
  const r = await fetch('/api/pipelines/start', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
  return j;
}

export async function advancePipeline(input: { step_id: string; note?: string; decided_by?: string | null; decision?: 'approve' | 'reject' }): Promise<{ state: PipelineRunState }> {
  const r = await fetch('/api/pipelines/advance', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
  return j;
}

export async function cancelPipeline(reason?: string): Promise<{ state: PipelineRunState | null }> {
  const r = await fetch('/api/pipelines/cancel', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ reason }),
  });
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function clearPipeline(): Promise<void> {
  await fetch('/api/pipelines/clear', { method: 'POST' });
}

export interface PipelineRunSummary {
  run_id: string;
  pipeline_id: string;
  title: string;
  status: PipelineStatus;
  started_at: string;
  ended_at: string;
  started_by: string | null;
  total_steps: number;
  steps_done: number;
  steps_rejected: number;
  duration_ms: number | null;
  cancel_reason: string | null;
}

export async function listPipelineRuns(limit = 50): Promise<{ runs: PipelineRunSummary[] }> {
  const r = await fetch(`/api/pipelines/runs?limit=${encodeURIComponent(limit)}`);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export interface PipelineCreateInput {
  id: string;
  label: string;
  description?: string;
  tags?: string[];
  steps: PipelineStepDef[];
  scope?: 'template' | 'project';
  overwrite?: boolean;
}

export async function createPipeline(input: PipelineCreateInput): Promise<{ ok: boolean; id: string; scope: string; path: string }> {
  const r = await fetch('/api/pipelines/create', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
  return j;
}
