# Code Review — Princípios

> Aplica-se a qualquer agente que revise PRs ou código gerado por outro agente.

## Checklist de revisão
1. **Escopo** — o PR faz só o que propõe? Flagre refactors fora de escopo.
2. **Nomeação** — variáveis e funções dizem o que fazem sem comentário explicativo.
3. **Testes** — cobertura do caminho feliz + 1 edge case. Sem testes que só chamam o mock.
4. **Segurança** — SQL injection, XSS, SSRF, auth bypass, secrets em log.
5. **Concorrência** — locks, race conditions, idempotência em operações financeiras.
6. **Observabilidade** — log estruturado com `trace_id`, métricas nos pontos críticos.

## Anti-padrões comuns
- `except Exception: pass` — engole bug.
- `if not user: return None` sem log — erro silencioso.
- Mutação de argumento (`list.append` em param compartilhado).
- `await` dentro de loop síncrono (`for x in ...: await ...` sem `gather`).
- Transação financeira sem `idempotency_key`.

## Tom no review
- **Aponte fato, sugira alternativa** — "isso bloqueia o event loop; troca por `asyncio.to_thread`" > "está errado".
- **Separe crítico de preferencial** — use `[MUST]`, `[SHOULD]`, `[NIT]`.
- **Nunca** aprove sem rodar os testes mentalmente.

## Quando pedir mudanças
- Qualquer [MUST] aberto.
- Mais de 3 [SHOULD] — vira débito técnico rápido.
- Zero teste em código de domínio novo.
