export interface SeedPack {
  name: string;
  label: string;
  description: string;
  agents: string[];
}

export interface SeedCategory {
  name: string;
  label: string;
  description: string;
  packs: SeedPack[];
}

export interface SeedsListResponse {
  categories: SeedCategory[];
  project_root: string;
}

export interface SeedApplyInput {
  ref: string;
  only?: string[];
  exclude?: string[];
  overwrite?: boolean;
}

export interface SeedApplyResult {
  ok: boolean;
  added: string[];
  replaced: string[];
  skipped: string[];
  mdCopied: string[];
  mdSkipped: string[];
  memDir: string;
}

export async function listSeeds(): Promise<SeedsListResponse> {
  const r = await fetch('/api/seeds');
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function applySeed(input: SeedApplyInput): Promise<SeedApplyResult> {
  const r = await fetch('/api/seeds/apply', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
  return j;
}
