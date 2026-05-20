// Token de sessão do humano logado (JWT Ed25519 emitido pelo backend após
// login/criação). Guardado em sessionStorage — vale enquanto a aba viver.
// É enviado como `Authorization: Bearer` nas rotas autenticadas (handoff).

const KEY = 'pmem.human.token';

export function setHumanToken(token: string | null): void {
  try {
    if (token) sessionStorage.setItem(KEY, token);
    else sessionStorage.removeItem(KEY);
  } catch {
    /* sessionStorage indisponível (modo restrito) — segue sem persistir */
  }
}

export function getHumanToken(): string | null {
  try {
    return sessionStorage.getItem(KEY);
  } catch {
    return null;
  }
}
