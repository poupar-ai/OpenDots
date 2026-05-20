import type { Autonomy, AutonomyLevel, AgentType, Gender } from '@/lib/types';
import type { RegisteredMcp } from '@/lib/mcps';

export interface UpdateAutonomyPayload {
  level: AutonomyLevel;
  call_human_when: string[];
  propagate_seed?: boolean;
  seed_category?: string;
  seed_slug?: string;
}

export interface UpdateAutonomyResult {
  ok: boolean;
  agent: string;
  updated: { autonomy?: Autonomy };
  propagated:
    | null
    | { category: string; slug: string; seedPath: string }
    | { skipped: true; reason: string };
}

export async function updateAutonomy(
  agent: string,
  payload: UpdateAutonomyPayload,
): Promise<UpdateAutonomyResult> {
  const r = await fetch(`/api/agents/${encodeURIComponent(agent)}/autonomy`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j?.error || `HTTP ${r.status}`);
  return j;
}

export interface UpdateModelPayload {
  model: string;
  propagate_seed?: boolean;
  seed_category?: string;
  seed_slug?: string;
}

export async function updateModel(agent: string, payload: UpdateModelPayload) {
  const r = await fetch(`/api/agents/${encodeURIComponent(agent)}/model`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j?.error || `HTTP ${r.status}`);
  return j;
}

export async function updateAgentContext(agent: string, content: string) {
  const r = await fetch(`/api/agents/${encodeURIComponent(agent)}/context`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ content }),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j?.error || `HTTP ${r.status}`);
  return j as { ok: true; agent: string; context_file: string; bytes: number };
}

export interface CreateAgentPayload {
  name: string;
  type: AgentType;
  emoji?: string;
  gender?: Gender;
  model?: string;
  role?: string;
  tools?: string[];
  mcps?: string[];
  can_talk_to?: string[];
  context?: string;
}

export async function createAgent(payload: CreateAgentPayload) {
  const r = await fetch('/api/agents', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j?.error || `HTTP ${r.status}`);
  return j as { ok: true; agent: string; created: unknown };
}

export async function deleteAgent(agent: string) {
  const r = await fetch(`/api/agents/${encodeURIComponent(agent)}`, { method: 'DELETE' });
  const j = await r.json();
  if (!r.ok) throw new Error(j?.error || `HTTP ${r.status}`);
  return j as { ok: true; agent: string };
}

export interface AgentScope {
  bash_allow: string[];
}

export interface UpdateScopePayload {
  tools?: string[];
  scope?: AgentScope;
}

export async function updateAgentScope(agent: string, payload: UpdateScopePayload) {
  const r = await fetch(`/api/agents/${encodeURIComponent(agent)}/scope`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j?.error || `HTTP ${r.status}`);
  return j as { ok: true; agent: string; updated: { tools?: string[]; scope?: AgentScope } };
}

export async function updateAgentVault(agent: string, secrets: string[]) {
  const r = await fetch(`/api/agents/${encodeURIComponent(agent)}/vault`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ secrets }),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j?.error || `HTTP ${r.status}`);
  return j as { ok: true; agent: string; secrets: string[] };
}

export async function updateAgentMcps(agent: string, mcps: string[]) {
  const r = await fetch(`/api/agents/${encodeURIComponent(agent)}/mcps`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ mcps }),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j?.error || `HTTP ${r.status}`);
  return j as { ok: true; agent: string; mcps: string[] };
}

export interface McpsList {
  global: string[];
  project: string[];
  referenced: string[];
  /** MCPs cadastrados no projeto (catálogo próprio, com def + storage). */
  registered: RegisteredMcp[];
  all: string[];
}

export async function listMcps(): Promise<McpsList> {
  const r = await fetch('/api/mcps');
  const j = await r.json();
  if (!r.ok) throw new Error(j?.error || `HTTP ${r.status}`);
  return j as McpsList;
}
