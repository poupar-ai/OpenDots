# Concorrência Assíncrona — Princípios

> Aplica-se a agentes que escrevem código async (Python FastAPI, workers, filas).

## Regras de ouro
1. **Nunca** bloqueie o event loop — sem `time.sleep`, sem `requests.get`, sem I/O síncrono.
2. **Sempre** use `async def` para rotas FastAPI e funções que fazem I/O.
3. **Paralelismo** com `asyncio.gather`, não `for` sequencial.
4. **Timeout explícito** em toda chamada externa (`asyncio.wait_for`).
5. **Idempotency key** obrigatória em operação financeira ou com side effect.
6. **Backoff exponencial** em retry — sem retry em loop apertado.

## Padrões
```python
# BOM — paraleliza
results = await asyncio.gather(fetch_a(), fetch_b(), fetch_c())

# RUIM — sequencial disfarçado
a = await fetch_a()
b = await fetch_b()
c = await fetch_c()
```

```python
# BOM — trabalho CPU-bound num thread
result = await asyncio.to_thread(heavy_cpu_work, data)

# RUIM — bloqueia o event loop
result = heavy_cpu_work(data)
```

## Filas (SQS)
- Sempre `VisibilityTimeout` >= tempo máximo de processamento.
- Sempre DLQ configurada.
- Mensagem tem `trace_id` + `idempotency_key`.
- Consumer é idempotente — deduplica por `idempotency_key`.

## Anti-padrões
- `async def` que não faz `await` em nada — é função síncrona mascarada.
- Compartilhar cursor/session async entre tasks — cada task abre o seu.
- `await` em loop sem `gather` quando N chamadas são independentes.
- Swallowing de `asyncio.CancelledError` — sempre re-raise.
