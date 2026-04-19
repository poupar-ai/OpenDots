# AGENTS.md — project-mem v2 (Rust port)

> Guia de onboarding para qualquer IA que entrar frio neste diretório.
> Leia antes de editar qualquer arquivo em `v2/`.

## O que é este diretório

`v2/` é a **migração em andamento** do plugin comercial
**project-mem** de Node.js CJS para Rust, seguindo arquitetura
hexagonal. O código em `../plugin/` (Node) continua sendo a versão
de produção até a paridade chegar a 100% e o cutover ser feito.

**Nada aqui deve impactar a v1 em produção.**

project-mem é produto comercial closed-source vendido como binário
assinado com licença Firebase Ed25519 em 4 tiers (Free/Basic/Pro/
Business com limites de instalação 1/1/3/10).

## Regra de ouro — Paridade por fase

A cada fase da migração entregamos **três coisas, lado a lado**:

1. **Código portado** (Rust).
2. **Unit tests** cobrindo o código portado.
3. **Parity tests cross-language** — `tests/parity_*.rs` spawn
   `node` executando o `.cjs` original e compara o output ao da
   implementação Rust byte-a-byte (stdout, JSON, árvore de arquivos
   gerada, headers HTTP, etc.).

Fase só é dada por concluída quando **os dois conjuntos de testes
estão 100% verdes**. Se um parity test detecta divergência, é
bug do Rust até prova em contrário — o `.cjs` é a verdade.

Esse compromisso está gravado em `feedback_port_rust_paridade.md`
da memória persistente do Claude Code.

## Plano macro

O plano completo está em [`../docs/migrato_to_rust.md`](../docs/migrato_to_rust.md).
Resumo das fases:

| Fase | Escopo                                                        | Estado       |
|------|---------------------------------------------------------------|--------------|
| 0    | Scaffolding do workspace Rust + branding                       | ✅ pronto    |
| 1    | `core-ports`: 9 traits (contratos) sem lógica                  | ✅ pronto    |
| 2    | `core-paths`: port de `lib.cjs` (paths, JSONL, stats, config)  | 🔄 em curso  |
| 3    | Adapters de fs (observações, sessões, grafo)                   | pendente     |
| 4    | `domain-graph`, `domain-observations`                          | pendente     |
| 5    | Cofre (PMV1/PMV2/PMVEXP1)                                      | pendente     |
| 6    | Bus de agentes + sigilo + sandbox                              | pendente     |
| 7    | HTTP/SSE server (Axum) + MCP stdio                             | pendente     |
| 8    | Licença + update + telemetria                                  | pendente     |
| 9    | CLI + instalador                                               | pendente     |
| 10   | Cutover                                                        | pendente     |

A Fase 2 anda **um "ponto" por vez** — não porta o `lib.cjs` inteiro
de uma vez. Cada ponto é uma função ou trio coeso, entregue com
unit + parity tests antes do próximo.

## Estrutura do workspace

```
v2/
├── Cargo.toml               # workspace root (resolver = "2")
├── rust-toolchain.toml      # pin 1.85 (1.83 quebra em hashbrown 0.17)
├── settings.toml            # branding carregado em runtime
├── README.md                # overview humano
├── AGENTS.md                # este arquivo (onboarding IA)
└── crates/
    ├── core-branding/       # identidade do produto (TOML → structs)
    ├── core-ports/          # 9 traits + tipos compartilhados
    │   ├── src/
    │   │   ├── clock.rs         # Clock + SystemClock
    │   │   ├── observation.rs   # JSONL append-only logs
    │   │   ├── session.rs       # flag files de sessão
    │   │   ├── vault.rs         # cofre (~20 métodos)
    │   │   ├── graph.rs         # grafo de projeto
    │   │   ├── license.rs       # Ed25519 + tiers
    │   │   ├── update.rs        # manifest assinado
    │   │   ├── telemetry.rs     # opt-in sink
    │   │   └── notifier.rs      # bus SSE/agente
    │   └── tests/
    │       └── parity_*.rs      # spawn node, compara byte-a-byte
    └── core-paths/          # Fase 2 — port de lib.cjs
        ├── src/lib.rs           # find_project_root, ensure_* , ...
        └── tests/parity_*.rs
```

### Arquitetura hexagonal (convenção dos prefixos)

- **`core-*`** — infraestrutura reutilizável sem lógica de negócio
  (ports, branding, paths).
- **`domain-*`** — regras de negócio que consomem ports. Testadas
  com fakes. **Não importam adapters.**
- **`adapter-*`** — implementações concretas dos ports (disco, HTTP,
  crypto). Podem ser trocadas sem afetar domínio.
- **`api-*`** — pontos de entrada (CLI, HTTP server, MCP stdio).

## Como compilar e testar

```bash
cd v2

# build inteiro
cargo build --workspace

# testa tudo (inclui parity tests — precisa `node` no PATH)
cargo test --workspace

# lint
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check

# crate específico
cargo test -p core-paths
```

**Parity tests exigem `node` instalado.** Sem node, testam skippam
com `eprintln!` (não falham). Sempre rodar num ambiente com node
antes de fechar fase.

## Convenções de código deste port

### Naming
- Funções/métodos em `snake_case` mapeando 1:1 com o `.cjs` em
  `camelCase`. `findProjectRoot` → `find_project_root`,
  `ensureTmpDir` → `ensure_tmp_dir`.
- Nomes de **campos serializados** preservam o case do `.cjs` (via
  `#[serde(rename_all=...)]` quando precisa). Testes de paridade
  validam isso.

### Erros
- Uma enum `*Error` por crate/módulo usando `thiserror`.
- Erros que o `.cjs` engolia silenciosamente (ex.: git falhou →
  fallback no cwd) **permanecem infalíveis** no Rust também.
  Paridade de comportamento importa mais que "idiomático".

### Comentários
- Doc comment (`///`) em tudo que for público, explicando:
  a) o que faz, b) qual a função `.cjs` equivalente (se houver),
  c) edge cases (fallback, idempotência, timeout).
- Inline (`//`) só para decisões não óbvias ou por-ques específicos
  de paridade. Evitar "comentário-narrador" que repete o código.

### Dependências
- `async_trait` só onde a trait faz I/O de rede (license, update,
  telemetry). Traits locais são síncronas.
- Evitar crates pesados em `core-*`. Se um adapter precisa de
  `reqwest`, o crate `adapter-license-firebase` carrega —
  `core-ports` fica leve.

## Regras de commit e escopo

- Um ponto da fase = uma unidade de commit. Não misturar fase 2 com
  refactor fora do ponto.
- Nunca editar nada em `../plugin/` (v1) sem conversar antes. A v1
  é verdade congelada até o cutover.
- Nunca "melhorar" o `.cjs` durante a migração — se algo parece
  errado lá, é um bug a investigar, não a corrigir unilateralmente.

## Armadilhas conhecidas

- **Rust toolchain 1.83 quebra** em `hashbrown 0.17` que precisa
  edition2024. `rust-toolchain.toml` fixa em 1.85.
- **Parity tests no macOS** podem falhar por symlink `/private/tmp`
  — sempre `canonicalize()` antes de comparar paths.
- **Terminal quebra** em `child.kill()` se o processo já morreu;
  `.ok()` é intencional.
- **`plugin/ui/index.html` NÃO migra.** Vai ser embutido no binário
  Rust via `include_str!` no Fase 7. Só o core muda de linguagem.
- **`PathBuf::join` não normaliza** `./`, `//`, `..` — diverge de
  `path.join` do Node. `core-paths` resolve via `normalize_posix` +
  `path_join`. Nunca usar `PathBuf::join` direto em paths que o `.cjs`
  equivalente produziria normalizados.
- **`serde_json` precisa de `preserve_order`** para bater ordem de
  chaves com `JSON.stringify` do Node. Feature habilitada no
  `core-paths/Cargo.toml`; propagada pelo workspace por unification.
- **Inteiros `> 2^53` divergem por design.** Node perde precisão
  (double), Rust preserva (i64). Decidido não corrigir — nenhum
  caller de produção do project-mem grava ints tão grandes. Parity
  test `audit_append_big_number_matches_v1` marcado `#[ignore]`.

## Ao detectar divergência durante o port

**Nem toda divergência é bug do Rust.** O `.cjs` pode ter bug latente que
a migração descobriu por acaso. Fluxo:

1. Caracterizar o que cada lado devolve (empírico — spawn node,
   comparar byte-a-byte).
2. Decidir a origem:
   - Rust errado → corrigir Rust, paridade restaurada.
   - `.cjs` errado (bug latente) → **não alterar o `.cjs`**; Rust
     replica a divergência por paridade; registrar em
     `../docs/migration_findings.md` para tratamento pós-cutover.
   - Ambos certos, mas output diverge (ex.: precisão numérica) →
     decidir com Tiago; documentar escolha.
3. Em qualquer caso que envolva achado, registrar em
   `../docs/migration_findings.md` com severidade e recomendação.

## Referências cruzadas

- Plano macro: `../docs/migrato_to_rust.md`
- Achados da migração: `../docs/migration_findings.md`
- Código-fonte v1 (verdade): `../plugin/scripts/*.cjs`
- UI v1 (será embutida, não portada): `../plugin/ui/index.html`
- Repo público do produto comercial: github.com/schedylabs/claude-project-memory
- Memória persistente relevante: `feedback_port_rust_paridade.md`,
  `reference_project_mem_repo.md`.
