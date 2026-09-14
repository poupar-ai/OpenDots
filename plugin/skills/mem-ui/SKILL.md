---
name: mem-ui
description: Sobe a interface web local pra visualizar a memória do projeto.
---

# /mem-ui — Interface web do open-poupar

Sobe a interface web local para visualizar a memória do projeto atual.

## 1. Verifique se já está rodando

```bash
curl -sf http://localhost:37778/api/data > /dev/null 2>&1 && echo "rodando" || echo "parado"
```

Se já estiver rodando, informe o usuário e abra `http://localhost:37778`. Não suba um segundo servidor.

## 2. Encontre o servidor

O servidor é o binário `poupar-http`. Ele mora em lugares diferentes conforme
a instalação (tarball vs. clone), então resolva pelo helper:

```bash
_R="${CLAUDE_PLUGIN_ROOT}"
_SRV="$(bash "$_R/bin-path.sh" poupar-http)" && echo "$_SRV"
```

Se o comando falhar, a instalação não tem o binário — foi um clone sem
build. Peça pro usuário rodar:

```bash
cargo build --release --manifest-path v2/Cargo.toml -p api-http
cd ui-next && npm install && npm run build
```

## 3. Suba o servidor

Execute em background a partir da raiz do projeto:

```bash
"$_SRV" &
```

Aguarde 2 segundos e verifique se subiu:

```bash
curl -sf http://localhost:37778/api/data > /dev/null 2>&1 && echo "ok" || echo "falhou"
```

**Se a porta 37778 estiver ocupada:**
```bash
POUPAR_PORT=37779 "$_SRV" &
```

## 4. Informe o usuário

Diga:
- URL onde a UI está disponível
- O que cada aba mostra:
  - **PROJECT.md** — contexto permanente do projeto
  - **Sessões** — histórico com datas e resumos
  - **Observações** — todas as edições e comandos com busca
- Que a UI atualiza automaticamente a cada 10 segundos
- Para parar: feche o terminal ou rode `pkill -f poupar-http`
