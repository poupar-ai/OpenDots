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
  source?: 'catalog' | 'local';
}

export interface CreateBestPracticeInput {
  id: string;
  name: string;
  scope?: string;
  tags?: string[];
  when_to_use?: string;
  not_for?: string;
  content: string;
  overwrite?: boolean;
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

export async function createBestPractice(input: CreateBestPracticeInput): Promise<{ ok: boolean; id: string }> {
  const r = await fetch('/api/best-practices/create', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
  return j;
}

export interface UpdateBestPracticeInput {
  name?: string;
  scope?: string;
  tags?: string[];
  when_to_use?: string;
  not_for?: string;
  content?: string;
}

export async function updateBestPractice(id: string, input: UpdateBestPracticeInput) {
  const r = await fetch(`/api/best-practices/${encodeURIComponent(id)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
  return j as { ok: true; id: string };
}

export async function deleteBestPractice(id: string) {
  const r = await fetch(`/api/best-practices/${encodeURIComponent(id)}`, { method: 'DELETE' });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
  return j as { ok: true; id: string; removed: true };
}
