export interface BestPractice {
  id: string;
  name: string;
  scope?: string;
  tags?: string[];
  when_to_use?: string;
  not_for?: string;
  dir?: string;
  available?: boolean;
  installed?: boolean;
}

export async function listBestPractices(): Promise<{ available: BestPractice[]; installed: string[] }> {
  const r = await fetch('/api/best-practices');
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function getBestPractice(id: string): Promise<{ id: string; content: string; installed: boolean }> {
  const r = await fetch(`/api/best-practices/${encodeURIComponent(id)}`);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function applyBestPractices(ids: string[], overwrite = false): Promise<{ ok: boolean; copied: string[]; preserved: string[]; missing: string[] }> {
  const r = await fetch('/api/best-practices/apply', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ids, overwrite }),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
  return j;
}
