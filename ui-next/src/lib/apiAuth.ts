import { getHumanToken } from './session';

// Intercepta window.fetch e anexa `Authorization: Bearer <token-do-humano>`
// em toda chamada `/api/*` quando há token de sessão. Centraliza o auth sem
// precisar tocar cada `fetch` espalhado pelos hooks/libs. Não sobrescreve um
// Authorization já definido (ex.: handoff que monta o header manualmente).
let installed = false;

export function installApiAuth(): void {
  if (installed || typeof window === 'undefined') return;
  installed = true;
  const orig = window.fetch.bind(window);

  window.fetch = (input: RequestInfo | URL, init?: RequestInit) => {
    const url =
      typeof input === 'string'
        ? input
        : input instanceof URL
          ? input.pathname
          : (input as Request).url;

    // Só requests da nossa API; e não mexe em objetos Request prontos.
    if (typeof url === 'string' && url.includes('/api/') && !(input instanceof Request)) {
      const token = getHumanToken();
      if (token) {
        const headers = new Headers(init?.headers || {});
        if (!headers.has('Authorization')) {
          headers.set('Authorization', `Bearer ${token}`);
          init = { ...init, headers };
        }
      }
    }
    return orig(input as RequestInfo | URL, init);
  };
}
