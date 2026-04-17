export type SupportedIde = 'claude-code' | 'cursor' | 'vscode-copilot' | 'codex';

export const IDE_META: Record<SupportedIde, { label: string; hint: string }> = {
  'claude-code':    { label: 'Claude Code',    hint: '.claude/agents/<name>.md' },
  'cursor':         { label: 'Cursor',         hint: '.cursor/rules/<name>.mdc' },
  'vscode-copilot': { label: 'VS Code Copilot',hint: '.github/instructions/<name>.instructions.md' },
  'codex':          { label: 'Codex',          hint: '.codex/instructions/<name>.md' },
};

export async function listIdes(): Promise<{ supported: SupportedIde[] }> {
  const r = await fetch('/api/ides');
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function applyIde(ide: SupportedIde): Promise<{ ok: boolean; written: string[] }> {
  const r = await fetch('/api/ides/apply', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ide }),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
  return j;
}
