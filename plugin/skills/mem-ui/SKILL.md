# /mem-ui — Interface web do project-man

Sobe a interface web local para visualizar a memória do projeto atual.

## 1. Verifique se já está rodando

```bash
curl -sf http://localhost:37778/api/data > /dev/null 2>&1 && echo "rodando" || echo "parado"
```

Se já estiver rodando, informe o usuário e abra `http://localhost:37778`. Não suba um segundo servidor.

## 2. Encontre o script do servidor

```bash
_R="${CLAUDE_PLUGIN_ROOT:-/home/tiago/pessoal/project-mem/plugin}"
echo "$_R/ui/server.cjs"
```

## 3. Suba o servidor

Execute em background a partir da raiz do projeto:

```bash
node "$_R/ui/server.cjs" &
```

Aguarde 2 segundos e verifique se subiu:

```bash
curl -sf http://localhost:37778/api/data > /dev/null 2>&1 && echo "ok" || echo "falhou"
```

**Se a porta 37778 estiver ocupada:**
```bash
PROJECT_MEM_PORT=37779 node "$_R/ui/server.cjs" &
```

## 4. Informe o usuário

Diga:
- URL onde a UI está disponível
- O que cada aba mostra:
  - **PROJECT.md** — contexto permanente do projeto
  - **Sessões** — histórico com datas e resumos
  - **Observações** — todas as edições e comandos com busca
- Que a UI atualiza automaticamente a cada 10 segundos
- Para parar: feche o terminal ou rode `pkill -f "server.cjs"`
