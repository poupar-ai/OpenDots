# ADR-001 — Multi-modelo (decisão de adiar + plano de migração)

**Status**: Aceito (adiar implementação) · 2026-05
**Decisores**: Tiago (PO), Claude (arquitetura)
**Substitui**: —
**Substituído por**: —

## Contexto

Hoje o plugin assume **Claude CLI** como único runtime de modelo. Spawn
de subprocess via `TokioCommand::new("claude")` em 2 hot paths:

- `v2/crates/api-http/src/chat_send_routes.rs::chat_send_handler`
- `v2/crates/api-http/src/agent_runner.rs::run_headless_turn`

O nome do binário, args (`-p`, `--output-format`, `--model`,
`--allowed-tools`, `--disallowed-tools`), env vars (`CLAUDE_PROJECT_MEM_*`),
e formato de output (stdout = resposta plain) são todos amarrados ao
Claude CLI.

Projetos como o [Hermes Agent](https://hermes-agent.nousresearch.com/)
suportam 200+ modelos via OpenRouter + providers próprios (Nous Portal,
Novita AI, NVIDIA NIM, etc.) — usuário troca com `hermes model`.

## Decisão

**Manter Claude CLI hardcoded por enquanto.** Não introduzir abstração
de provider agora. Razões:

1. **Identidade do projeto**: somos "claude-project-memory" — plugin
   pro Claude Code. Lock-in com Claude é coerente com o nome e o
   contexto de uso. Usuário que escolhe nosso plugin já decidiu usar
   Claude.

2. **Custo de abstração agora**: trait `LlmRunner` + adapters requer
   ~200 linhas de refactor em 2 arquivos hot. Cada adapter novo (codex
   CLI, OpenAI API, Anthropic API direta) é mais 100-200 linhas. Sem
   demanda concreta, vira código especulativo.

3. **Spawn vs API**: nosso modelo de execução é "spawn subprocess por
   turn". Pra suportar modelos via API HTTP direto (sem CLI), seria
   uma arquitetura diferente — não basta trocar `claude` por outro
   binário. Mudança maior do que parece.

4. **Foco vs amplitude**: Hermes é categoria "plataforma". Nosso é
   categoria "feature de plugin". Adicionar multi-modelo nos move pra
   "plataforma" sem foco claro. Vide README — nosso valor é "memória
   por projeto", não "broker de LLMs".

## Quando reverter essa decisão

Sinais concretos que justificariam implementar:
- Usuário com demanda específica por modelo não-Claude (não hipótese).
- Plano de comercialização que requer multi-provider.
- Custos Claude inviabilizando uso (raro hoje pra projetos pessoais).
- Decisão estratégica de virar agnóstico (mudança de identidade do projeto).

## Plano de migração (quando vier)

### Fase 1 — Extrair `LlmRunner` trait

```rust
// v2/crates/core-llm/src/lib.rs (crate novo)
pub struct LlmTurnRequest {
    pub model: String,
    pub system_prompt: Option<String>,
    pub user_message: String,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: PathBuf,
}

pub struct LlmTurnResponse {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
}

#[async_trait]
pub trait LlmRunner: Send + Sync {
    async fn run_turn(&self, req: &LlmTurnRequest) -> LlmTurnResponse;
}
```

### Fase 2 — Impl `ClaudeCliRunner` (preserva comportamento atual)

```rust
// v2/crates/adapter-llm-claude-cli/src/lib.rs
pub struct ClaudeCliRunner;

impl LlmRunner for ClaudeCliRunner {
    async fn run_turn(&self, req: &LlmTurnRequest) -> LlmTurnResponse {
        // Move pra cá o spawn de TokioCommand::new("claude") que hoje
        // mora em chat_send_routes + agent_runner.
        // ...
    }
}
```

### Fase 3 — Refactor callers

`chat_send_routes` + `agent_runner` deixam de chamar `TokioCommand`
direto e passam a chamar `state.llm.run_turn(&req).await`. AppState
ganha `pub llm: Arc<dyn LlmRunner>`. Construção em `AppState::new` faz
default = `Arc::new(ClaudeCliRunner)`.

### Fase 4 — Adapters adicionais

Cada novo provider é um crate `adapter-llm-<nome>/` espelhando o
trait. Config (qual adapter ativar) via `<mem_dir>/llm.json`:

```json
{
  "provider": "claude-cli",
  "model": "sonnet-4.6"
}
```

Endpoint `PUT /api/llm/config` pra trocar via UI/seed.

### Fase 5 — Adapters relevantes (prioridade quando vier)

1. `adapter-llm-anthropic-api` — Claude direto via API HTTP (sem CLI).
   Útil pra ambientes sem CLI instalado (servidor remoto).
2. `adapter-llm-openrouter` — proxy pra 200+ modelos. Maior alavanca.
3. `adapter-llm-openai` — GPT direto.

## Onde está hardcoded HOJE (pra grep futuro)

Procurar por `TokioCommand::new("claude")` no codebase:

- `v2/crates/api-http/src/chat_send_routes.rs:~851`
- `v2/crates/api-http/src/agent_runner.rs:~159`

Em cada um, há um comentário inline apontando pra este ADR.

## Consequências

**Positivas**:
- Foco preservado. Projeto continua sendo "plugin de memória pro Claude
  Code" sem ambiguidade.
- Zero dívida técnica imediata. Não pagamos custo de abstração agora.

**Negativas**:
- Quando vier multi-modelo, refactor não é trivial (~200 linhas em hot
  paths). Mas o plano acima reduz risco — cada fase é commit isolado.
- Não atrai usuários de outros modelos. Aceitável dado o nome do
  projeto.

**Neutras**:
- Hermes Agent não é competidor direto (categoria diferente —
  plataforma vs plugin). Comparações de scope são apenas referência.
