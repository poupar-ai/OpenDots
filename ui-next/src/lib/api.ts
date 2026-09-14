// Helper de fetch que injeta o Bearer token salvo no localStorage,
// quando configurado. Middleware do servidor exige isso quando rodando
// com `POUPAR_REQUIRE_TOKEN=1`. Sem token, fetches passam sem
// header — compat com modo dev (default).
//
// Migração orgânica: ao tocar um fetch existente, troca pra apiFetch.
// Fetches que não usarem isso continuam funcionando enquanto enforcement
// estiver off; quando ligado, vão receber 401 até serem migrados.

const TOKEN_KEY = 'open-poupar.server_token';

export function getServerToken(): string | null {
  try {
    return localStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

export function setServerToken(token: string | null): void {
  try {
    if (token) localStorage.setItem(TOKEN_KEY, token);
    else localStorage.removeItem(TOKEN_KEY);
  } catch {
    /* localStorage indisponível — ignora */
  }
}

export async function apiFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  const token = getServerToken();
  const headers = new Headers(init?.headers || {});
  if (token && !headers.has('authorization')) {
    headers.set('Authorization', `Bearer ${token}`);
  }
  return fetch(input, { ...init, headers });
}
