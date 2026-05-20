// Registro de MCP do projeto — cadastro/edição/remoção. As chamadas são
// mutáveis (POST/DELETE) → o wrapper de fetch anexa o Bearer do humano e o
// backend exige auth (igual agents/humans). storage escolhe vault×banco.

export type McpTransport = 'stdio' | 'http' | 'sse';
export type McpStorage = 'db' | 'vault';

export interface RegisteredMcp {
  name: string;
  transport: McpTransport;
  description?: string;
  /** stdio */
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  /** http/sse */
  url?: string;
  headers?: Record<string, string>;
  storage: McpStorage;
}

export async function registerMcp(mcp: RegisteredMcp): Promise<RegisteredMcp> {
  const r = await fetch('/api/mcps', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(mcp),
  });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(j?.error || `HTTP ${r.status}`);
  return j.mcp as RegisteredMcp;
}

/** Def completa (resolve vault). Pode falhar se storage=vault e cofre trancado. */
export async function getMcp(name: string): Promise<RegisteredMcp> {
  const r = await fetch(`/api/mcps/${encodeURIComponent(name)}`);
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(j?.error || `HTTP ${r.status}`);
  return j.mcp as RegisteredMcp;
}

export async function deleteMcp(name: string): Promise<void> {
  const r = await fetch(`/api/mcps/${encodeURIComponent(name)}`, { method: 'DELETE' });
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j?.error || `HTTP ${r.status}`);
  }
}
