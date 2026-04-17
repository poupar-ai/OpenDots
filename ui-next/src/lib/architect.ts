export interface ArchitectAgentSpec {
  name: string;
  label?: string;
  icon?: string;
  category?: string;
  description?: string;
  prompt?: string;
  best_practices?: string[];
}

export interface ArchitectSeedSpec {
  slug: string;
  category: string;
  label?: string;
  description?: string;
  category_label?: string;
  category_description?: string;
  best_practices?: string[];
  agents: ArchitectAgentSpec[];
}

export interface ArchitectGenerateResult {
  ok: boolean;
  ref: string;
  dir: string;
  agent_count: number;
}

export async function ensureArchitectInstalled(): Promise<{ ok: boolean }> {
  const r = await fetch('/api/architect/ensure', { method: 'POST' });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
  return j;
}

export async function generateArchitectSeed(spec: ArchitectSeedSpec): Promise<ArchitectGenerateResult> {
  const r = await fetch('/api/architect/generate', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(spec),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
  return j;
}
