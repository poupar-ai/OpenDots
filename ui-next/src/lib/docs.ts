export type DocName = 'SOUL.md' | 'PROJECT.md' | 'DECISIONS.md' | 'GOTCHAS.md' | 'GLOSSARY.md' | 'CODE_INDEX.md';

export async function saveDoc(name: DocName, content: string) {
  const r = await fetch(`/api/docs/${encodeURIComponent(name)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ content }),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j?.error || `HTTP ${r.status}`);
  return j as { ok: true; doc: string; bytes: number };
}
