# Achados da Migração Node → Rust

> Problemas, bugs latentes e oportunidades de melhoria detectados
> durante o port de `plugin/scripts/*.cjs` para `v2/crates/`.
>
> **Não corrigir o `.cjs` em produção sem revisar com Tiago.** O
> compromisso da migração é paridade byte-a-byte até o cutover —
> "corrigir" um bug no `.cjs` agora quebra a paridade e pode surpreender
> callers que dependem do comportamento atual.
>
> Esta lista existe para, após o cutover, decidir quais itens o Rust
> deve passar a "corrigir" explicitamente (com feature flag ou major
> bump de formato) e quais continuam replicados por compatibilidade.

## Convenção

- **[FX-NN]** = Fase X, achado NN.
- **Severidade:**
  - 🔴 **bug** — comportamento errado observável hoje em caller real.
  - 🟡 **latente** — bug silencioso só se o input cair num corner case
    que a produção atual não produz.
  - 🟢 **melhoria** — sem bug, mas vale reforçar no Rust quando houver
    luz verde para divergir.
- **Paridade v2:** se o Rust já replica, diverge, ou ficou a decidir.

---

## Fase 2 — Port de `lib.cjs`

### [F2-01] 🟡 `findProjectRoot` cai em fallback silencioso no timeout/erro
- **Onde:** `plugin/scripts/lib.cjs:13-23`.
- **Comportamento:** se `git rev-parse --show-toplevel` falha (git ausente,
  não é repo) **ou passa de 3s** (network FS lento, repo gigante),
  retorna o `startCwd` sem nenhum log/warn.
- **Impacto:** memória pode acabar gravada em `$HOME/.claude-project-memory`
  em vez de no projeto, dependendo de onde o Claude Code foi invocado.
  Usuário só percebe quando algo que devia estar lá não está.
- **Recomendação pós-cutover:** emitir warning estruturado (stderr JSON)
  quando cair no fallback. Manter fallback — derrubar aqui quebra Claude
  Code inteiro em ambiente sem git.
- **Paridade v2:** ✅ replica literalmente em `core_paths::find_project_root`.

### [F2-02] 🟡 `ensureMemDir` não valida que o path é diretório
- **Onde:** `plugin/scripts/lib.cjs:35-37`.
- **Comportamento:** `if (!fs.existsSync(memDir)) fs.mkdirSync(...)`. Se
  `memDir` existe mas é arquivo (usuário criou `.claude-project-memory`
  como arquivo por engano), a função é no-op. Próxima chamada
  downstream (`appendJsonl`, `fs.writeFileSync`) falha de forma
  confusa.
- **Impacto:** diagnóstico ruim. Erro "ENOTDIR" longe do lugar onde a
  causa real está.
- **Recomendação pós-cutover:** checar `stat.isDirectory()` e lançar
  erro claro se for arquivo.
- **Paridade v2:** ✅ replica literalmente em `core_paths::ensure_mem_dir`.

### [F2-03] 🔴 `appendJsonl` em arquivo sem `\n` final concatena grude
- **Onde:** `plugin/scripts/lib.cjs:56-58`.
- **Comportamento:** `fs.appendFileSync(file, JSON.stringify(obj) + '\n')`.
  Se o arquivo anterior terminou sem `\n` (por crash parcial, edição
  manual, ou arquivo criado por outra ferramenta), o append gera bytes
  do tipo `{"a":1}{"b":2}\n`. Linha composta **não é parseável** —
  `readJsonl` dropa silenciosamente.
- **Impacto:** perda silenciosa da observação anterior + da nova.
- **Recomendação pós-cutover:** ler último byte antes do append; se
  não é `\n`, prefixar `\n` no write. Custo: 1 syscall extra.
- **Paridade v2:** ✅ replica literalmente (bug preservado por design).
  Ver `jsonl::append_jsonl` + parity test
  `audit_append_without_trailing_newline_produces_glued_bytes`.

### [F2-04] 🟡 `readJsonl` dropa valores falsy legítimos
- **Onde:** `plugin/scripts/lib.cjs:47-54`.
- **Comportamento:** `.filter(Boolean)` aplicado pós-parse elimina
  qualquer linha que deserialize para `null`, `false`, `0`, `""`.
- **Impacto:** se algum caller gravar um desses valores como linha JSON
  legítima, `read_jsonl` nunca o devolve. Os callers atuais do
  project-mem sempre gravam objetos, então o bug é latente.
- **Recomendação pós-cutover:** aplicar filter só em valores parse-falha
  (já retornamos `null`), distinguindo de JSON literal `null`.
  Atualmente os dois se confundem.
- **Paridade v2:** ✅ replicado em `core_paths::jsonl::is_truthy`.

### [F2-05] 🔴 `readLastJsonlLine` retorna `null` quando a última linha
corrompe e o arquivo cabe num chunk
- **Onde:** `plugin/scripts/lib.cjs:62-90`.
- **Comportamento:** lê backward em chunks de 2KB. Se a última linha
  falha parse, o loop só continua se ainda houver chunk acima para
  ler. Arquivo pequeno com tail corrompido devolve `null` mesmo
  tendo N linhas válidas parseáveis atrás.
- **Impacto:** depois de crash parcial de gravação (disco cheio,
  SIGKILL durante append), o estado útil mais recente some da leitura
  — caller acha que não há nada.
- **Recomendação pós-cutover:** quando parse da última linha falha,
  tentar `lines[len-2]`, `[len-3]`, etc. **antes** de ir pro próximo
  chunk. Custo desprezível, muda semântica importante.
- **Paridade v2:** ✅ replica exatamente em `jsonl::read_last_jsonl_line`.
  Confirmado empiricamente contra `.cjs` em
  `read_last_corrupted_tail_single_chunk_matches_v1`.

### [F2-06] 🟢 Inteiros `> 2^53` perdem precisão no Node, não em Rust
- **Onde:** qualquer `JSON.parse` / `JSON.stringify` do Node.
- **Comportamento:** Node só tem double — `9007199254740993` vira
  `9007199254740992` silenciosamente. Rust preserva i64 até
  `i64::MAX`.
- **Impacto:** hoje zero (project-mem não grava ints tão grandes — IDs
  são UUID string, timestamps ms cabem em i64 até o ano 294.276).
  Mas um caller futuro que gravasse um hash numérico grande perderia
  info no `.cjs` sem saber.
- **Recomendação pós-cutover:** manter comportamento Rust (mais
  preciso). Nenhum ajuste necessário.
- **Paridade v2:** ⚠️ **diverge por design**. Test
  `audit_append_big_number_matches_v1` marcado `#[ignore]` com razão
  documentada. Registrado em `AGENTS.md`.

### [F2-07] 🟡 BOM UTF-8 quebra leitura silenciosamente
- **Onde:** `readJsonl`, `readLastJsonlLine`.
- **Comportamento:** arquivo aberto com BOM `EF BB BF` faz a primeira
  linha ter o caractere `\uFEFF` colado no `{` inicial → `JSON.parse`
  falha → filter drop → arquivo parece vazio.
- **Impacto:** usuário editou o arquivo em notepad Windows, abriu
  salvou, virou vazio na leitura. Silente.
- **Recomendação pós-cutover:** strip do BOM no início do `read_to_string`.
- **Paridade v2:** ✅ replicado (Rust tem o mesmo problema porque não
  strippa BOM).

### [F2-08] 🟡 `PathBuf::join` (Rust) não normaliza — lição para outros ports
- **Onde:** não é bug do `.cjs`. É armadilha do Rust.
- **Comportamento:** `PathBuf::join` preserva literais `./`, `//`, `..`.
  `path.join` do Node normaliza sempre.
- **Impacto no port:** qualquer função futura que construa paths a
  partir de inputs não sanitizados vai divergir do `.cjs` em shape de
  string.
- **Recomendação:** usar `core_paths::path_join` em vez de
  `PathBuf::join` sempre que estivermos portando `path.join`. Incluído
  como regra no `AGENTS.md`.
- **Paridade v2:** ✅ corrigido no Ponto 2.

### [F2-09] 🟡 `serde_json` default ordena chaves alfabeticamente
- **Onde:** não é bug do `.cjs`. Armadilha do Rust.
- **Comportamento:** sem `preserve_order`, `Value::Object` é BTreeMap
  e serialização diverge de `JSON.stringify` do Node (que preserva
  inserção).
- **Impacto no port:** diffs de arquivo antes/depois do cutover fariam
  ruído visual enorme (chaves reordenadas em cada observação histórica).
- **Recomendação:** manter `preserve_order` ligado permanentemente.
- **Paridade v2:** ✅ feature ligada em `core-paths/Cargo.toml`,
  propaga por unification para todo o workspace.

---

## Fase 3+ (pendente)

Nada registrado ainda.
