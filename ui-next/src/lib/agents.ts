import type { Autonomy, AutonomyLevel } from '@/lib/types';

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
