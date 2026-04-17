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
  skills?: boolean | string[];
  ide?: 'claude-code' | 'cursor' | 'vscode-copilot' | 'codex' | null;
}

export interface SeedApplyResult {
  ok: boolean;
  added: string[];
  replaced: string[];
  skipped: string[];
  mdCopied: string[];
  mdSkipped: string[];
  memDir: string;
  practices?: { copied: string[]; preserved: string[]; missing: string[] };
  skills?: { copied: string[]; preserved: string[]; mcp_added: string[]; mcp_preserved: string[] };
  ide?: { written: string[] } | null;
}

export interface SkillSummary {
  name: string;
  description: string;
  scope: string;
  version: string;
  requires_mcp: boolean;
  env: string[];
  has_mcp: boolean;
}

export async function listSkillsInPack(category: string, pack: string): Promise<{ skills: SkillSummary[] }> {
  const r = await fetch(`/api/skills?category=${encodeURIComponent(category)}&pack=${encodeURIComponent(pack)}`);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
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
