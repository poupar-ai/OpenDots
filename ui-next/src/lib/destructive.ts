export type PatternSeverity = 'critical' | 'high' | 'medium';
export type PatternKind = 'literal' | 'regex';

export interface Pattern {
  id: string;
  label: string;
  text: string;
  kind: PatternKind;
  severity: PatternSeverity;
  enabled: boolean;
  origin: 'core' | 'user';
  readonly?: boolean;
  created_at?: string;
}

export interface PatternsResponse {
  core: Pattern[];
  user: Pattern[];
}

export interface DetectHit {
  id: string;
  label: string;
  origin: 'core' | 'user';
  severity: PatternSeverity;
  match: string;
}

export interface DetectResult {
  matched: boolean;
  hits: DetectHit[];
}

export async function listPatterns(): Promise<PatternsResponse> {
  const r = await fetch('/api/destructive/patterns');
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function saveUserPatterns(patterns: Pattern[]): Promise<Pattern[]> {
  const r = await fetch('/api/destructive/patterns', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ patterns }),
  });
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  const j = await r.json();
  return j.patterns || [];
}

export async function testPattern(text: string): Promise<DetectResult> {
  const r = await fetch('/api/destructive/test', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ text }),
  });
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function toggleCorePattern(id: string, disabled: boolean): Promise<string[]> {
  const r = await fetch('/api/destructive/core/toggle', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ id, disabled }),
  });
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  const j = await r.json();
  return j.disabled_core || [];
}
