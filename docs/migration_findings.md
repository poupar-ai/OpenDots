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
- **Impacto:** memória pode acabar gravada em `$HOME/.poupar`
  em vez de no projeto, dependendo de onde o Claude Code foi invocado.
  Usuário só percebe quando algo que devia estar lá não está.
- **Recomendação pós-cutover:** emitir warning estruturado (stderr JSON)
  quando cair no fallback. Manter fallback — derrubar aqui quebra Claude
  Code inteiro em ambiente sem git.
- **Paridade v2:** ✅ replica literalmente em `core_paths::find_project_root`.

### [F2-02] 🟡 `ensureMemDir` não valida que o path é diretório
- **Onde:** `plugin/scripts/lib.cjs:35-37`.
- **Comportamento:** `if (!fs.existsSync(memDir)) fs.mkdirSync(...)`. Se
  `memDir` existe mas é arquivo (usuário criou `.poupar`
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
  open-poupar sempre gravam objetos, então o bug é latente.
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
- **Impacto:** hoje zero (open-poupar não grava ints tão grandes — IDs
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

### [F2-10] 🟡 `estimateTokens` retorna `NaN` silenciosamente para não-string
- **Onde:** `plugin/scripts/lib.cjs:97-100`.
- **Comportamento:** `if (!str) return 0; return Math.ceil(str.length / 3.5);`.
  Para argumentos não-string, JS não falha:
  - `estimateTokens(42)` → `NaN` (number não tem `.length`, `undefined/3.5=NaN`).
  - `estimateTokens(true)` → `NaN`.
  - `estimateTokens({a:1})` → `NaN`.
  - `estimateTokens(-5)` → `NaN`.
  - `estimateTokens([1,2,3])` → `ceil(3/3.5)` = 1 (arrays têm `.length`, silenciosamente tratados como strings).
- **Impacto:** caller atual é único (`session-init.cjs:140`) e passa `text.trim()`
  (sempre string), então bug é latente. Se um caller futuro passar número
  ou undefined member access, grava `NaN` em `blocks[i].tokens`, que
  `JSON.stringify` converte para `null` — corrompendo arquivo de estatística
  ou orçamento de contexto sem erro visível.
- **Recomendação pós-cutover:** validar `typeof str === 'string'` e lançar
  ou retornar 0 explicitamente. No Rust isso já é prevenido pelo tipo
  (`&str`) — não é possível passar não-string.
- **Paridade v2:** ✅ **Rust mais forte por design** (prevenção em compile
  time via `&str`). Parity tests cobrem só inputs string, que é o que o
  caller real passa. Sem teste `#[ignore]` necessário — divergência só
  aparece em caller incorreto.

### [F2-11] 🟡 `incrementStats` faz coerção exótica quando campo não é número
- **Onde:** `plugin/scripts/lib.cjs:118` — `stats[field] = (stats[field] || 0) + 1`.
- **Comportamento empírico (confirmado com node `-e`):**
  - `stats[field] = "5"` → `(stats[field] || 0) + 1` = `"5" + 1` = `"51"` (concat string).
  - `stats[field] = [1,2,3]` → `"1,2,3" + 1` = `"1,2,31"`.
  - `stats[field] = "foo"` → `"foo1"`.
  - `stats[field] = true` → `2` (coerce bool→int, depois +1).
- **Impacto:** zero hoje — callers reais passam só `'sessions'` e
  `'observations'`, ambos inteiros. Um caller futuro que passar campo
  heterogêneo (ex.: `'last_injection_at'` por typo) corrompe stats.
- **Recomendação pós-cutover:** validar `typeof stats[field] === 'number'`
  antes de incrementar; se não for, fazer overwrite para 1 ou logar erro.
- **Paridade v2:** ⚠️ **diverge por design** — `increment_js_semantics` em
  `stats::increment_stats` trata non-numeric truthy como "falsy" (retorna
  `1`) em vez de replicar a coerção. Rust não propaga o bug. Parity tests
  atuais cobrem só os casos realistas (`null`, int 0, int positivo,
  missing). Caso um dia precisemos replicar bit-a-bit, trivial adicionar
  flag.

### [F2-12] 🔴 `loadStats` parse-fail faz `incrementStats` **sobrescrever** stats válido
- **Onde:** `plugin/scripts/lib.cjs:107` (swallow parse) + `lib.cjs:117`
  (fallback default).
- **Comportamento (confirmado empiricamente):**
  ```
  stats.json inicial: "\uFEFF{\"sessions\":9999,\"observations\":42,\"critical_metric\":\"keep-me\"}"
  # (BOM UTF-8 na primeira linha — parse falha)
  incrementStats(memDir, "sessions")
  → stats.json final: {"sessions":1,"observations":0}
  # sessions=9999 virou 1; critical_metric desapareceu
  ```
- **Impacto:** **perda silenciosa de dados**. Qualquer coisa que rode o
  JSON parse em falha — BOM (F2-07), truncagem por crash, edição manual
  com caractere inválido — faz a próxima chamada de `incrementStats`
  zerar o arquivo inteiro.
- **Recomendação pós-cutover:** se parse falhar, fazer backup do arquivo
  atual (`stats.json.corrupt-<timestamp>`) antes de sobrescrever, OU não
  tocar no arquivo e apenas logar warning. Atualização atômica via
  `tmp + rename`.
- **Paridade v2:** ✅ replica literalmente (`load_stats` → `None` →
  `default_stats()` → `save_stats` sobrescreve). Parity test
  `parity_increment_over_corrupt_file` confirma comportamento idêntico.

### [F2-13] 🟡 `initProjectConfig(memDir, undefined)` grava config sem chave `project`
- **Onde:** `plugin/scripts/lib.cjs:135-145`.
- **Comportamento (confirmado com node `-e`):**
  ```
  initProjectConfig(memDir, undefined)
  → {"version":"1","created_at":"2026-..."}
  # (chave `project` sumiu — JSON.stringify elimina valores undefined)
  ```
- **Impacto:** caller único (`session-init.cjs:85`) sempre passa
  `projectName` derivado de git ou fallback — não deveria passar
  undefined. Mas se uma refatoração quebrar essa derivação silenciosamente,
  config fica sem `project` e próxima leitura espera a chave.
- **Recomendação pós-cutover:** validar `typeof projectName === 'string'
  && projectName.length > 0` e lançar erro claro.
- **Paridade v2:** ✅ **Rust mais forte por design** — `project_name: &str`
  previne `undefined/null` em compile time. `""` passa (Rust não valida
  vazio), mas `None` é impossível. Mesma família de "tipo forte previne
  bug" vista em F2-10.

### [F2-14] 🟡 `initProjectConfig` preserva config não-objeto (string/array/obj vazio)
- **Onde:** `plugin/scripts/lib.cjs:136-137` — `if (existing) return existing`.
- **Comportamento (confirmado empiricamente):**
  - `config.json = "\"hello\""` → retorna string `"hello"`, **não** inicializa.
  - `config.json = "{}"` → retorna objeto vazio, **não** inicializa.
  - `config.json = "[]"` → retorna array vazio, **não** inicializa.
  - `config.json = "42"` → retorna número 42, **não** inicializa.
- **Impacto:** se arquivo for corrompido para um valor JSON truthy não-objeto
  (ex.: alguém editou manualmente, ou algum processo gravou tipo errado),
  `initProjectConfig` dá no-op. Próximos acessos a `.project` / `.version`
  falham em tempo de execução downstream.
- **Recomendação pós-cutover:** validar
  `typeof existing === 'object' && existing !== null && !Array.isArray(existing)`
  antes de tratar como config válido.
- **Paridade v2:** ✅ replica literalmente via `is_js_truthy(&existing)`.
  Bug preservado por design. Parity tests
  `parity_init_preserves_when_existing_is_{truthy_string, empty_object}`
  confirmam byte-a-byte.

### [F2-15] ⚠️ Armadilha do port: `Option::Some` ≠ JS truthy
- **Onde:** armadilha do Rust descoberta durante o Ponto 6.
- **Comportamento:** primeira tentativa de `init_project_config`
  usava `if let Some(existing) = load_project_config(...) { return Ok(existing); }`.
  Divergia do `.cjs` quando `config.json` continha `"null"` literal:
  JS `if (null)` → falsy → overwrite; Rust `Option::Some(Value::Null)`
  → match → preserva.
- **Impacto no port:** qualquer função portada que replique `if (existing)`
  do `.cjs` usando `Option` precisa também rodar um check semântico de
  truthiness, senão diverge em cenários com `null`/`0`/`false`/`""` na
  entrada.
- **Recomendação:** sempre que portar um `if (x)` JS onde `x` pode ser
  qualquer `Value`, usar `is_js_truthy(&x)` (helper em `config.rs`) ou
  equivalente. Considerar extrair para módulo `core_paths::js` se o
  padrão aparecer em mais lugares.
- **Paridade v2:** ✅ corrigido no Ponto 6. Unit + parity tests
  `init_overwrites_when_existing_is_{json_null, zero, false, empty_string}`
  ancoram a regressão.

### [F2-16] 🔴 `syncClaudeMd` corrompe arquivo se conteúdo contém `CLAUDE_MD_END` literal
- **Onde:** `plugin/scripts/lib.cjs:155-185` — path de replace usa
  `existing.indexOf(CLAUDE_MD_END)` que retorna a **primeira** ocorrência.
- **Comportamento observado:** se `projectMdContent` incluir a string
  `<!-- open-poupar.ai:end -->` literal (ex.: PROJECT.md
  documentando o próprio marker), a primeira invocação grava corretamente
  (path "arquivo ausente" — só concatena). A **segunda** invocação (path
  de replace) faz `indexOf(END)` bater no END **dentro** do bloco velho,
  não no terminador real. Resultado: tail do bloco vira órfão fora dos
  markers, e o novo bloco fica seguido de lixo.
  ```
  pass 1:  START\nbefore END after\nEND\n        ← ok
  pass 2:  START\nNEW\nEND after\nEND\n           ← corrompido
  ```
- **Impacto:** real se alguém colocar o marker literal no PROJECT.md
  (docs, troubleshooting, exemplos). Hoje a chance é baixa — o conteúdo
  vem de `renderProjectMd` do plugin, que é controlado — mas qualquer
  extensão futura que inclua docs explicando o mecanismo dispara.
- **Recomendação pós-cutover:** usar `lastIndexOf(END)` OU fazer
  `rfind` a partir do `startIdx` conhecido. No Rust, trocar
  `existing.find(END)` por `existing[start_idx..].find(END)` resolve
  sem mudar semântica na maioria dos casos realistas.
- **Paridade v2:** ✅ replica o bug byte-a-byte (Rust usa `str::find`
  que também é first-match). Registrado; não consertar antes do cutover.

### [F2-17] 🟡 `syncClaudeMd` com múltiplos blocos só atualiza o primeiro
- **Onde:** mesma função, path de replace.
- **Comportamento:** se `existing` contém dois pares `START..END`
  (estado que não deveria existir, mas pode surgir após F2-18 ou edição
  manual), apenas o **primeiro** bloco é substituído; o segundo fica
  intacto. Não há log nem erro.
- **Impacto:** baixo em produção (como o arquivo sempre é escrito via
  `syncClaudeMd`, estado de múltiplos blocos só aparece após edição
  humana). Importante documentar para debug futuro.
- **Recomendação pós-cutover:** ou (a) detectar múltiplos blocos e
  emitir warning, ou (b) fazer replace global (remove todos os blocos
  e reinjeta um). Qualquer das duas vira divergência com o `.cjs`.
- **Paridade v2:** ✅ replica.

### [F2-18] 🟡 `syncClaudeMd` com markers em ordem invertida (END antes de START) produz lixo
- **Onde:** mesma função. `.includes(START)` é true; `indexOf(END)`
  retorna a posição do END **que aparece antes** do START no arquivo.
- **Comportamento observado:** com `existing = "pre\nEND\nSTART\nmid\n"`,
  `startIdx = 20, endIdx = 4`. `before = existing.slice(0, 20)` (inclui
  o END fora de ordem), `after = existing.slice(4 + END.length)` (inclui
  o START e o `mid` como texto literal solto). Resultado:
  ```
  pre
  END                      ← órfão no início
  START
  NEW
  END
  START                    ← duplicado como texto
  mid
  ```
- **Impacto:** só importa se um arquivo for editado à mão até ficar com
  ordem invertida. Muito raro.
- **Recomendação pós-cutover:** checar `endIdx > startIdx`, senão tratar
  como malformed (append fresh).
- **Paridade v2:** ✅ replica (`str::find` tem mesma semântica).

### [F2-19] 🟢 `syncClaudeMd(undefined | null | number)` crasha — Rust previne no compilador
- **Onde:** `projectMdContent.trim()` no `.cjs:159`.
- **Comportamento:** invocação com `undefined`/`null` → `TypeError: Cannot
  read properties of X (reading 'trim')`. Com number → `trim is not a function`.
- **Impacto:** nenhum na prática — todos os callers (`session-init.cjs:130`,
  etc.) envolvem em `try/catch`. Mas o crash silencioso **descarta a
  invocação inteira**, então um typo num caller deixa o CLAUDE.md
  desatualizado sem warning.
- **Paridade v2:** 🟢 **melhoria grátis** — assinatura Rust
  `fn sync_claude_md(_, &str)` impossibilita o erro no compilador.
  Qualquer caller Rust que tente passar `None`/`null`/non-string não
  compila.

### [F2-20] 🟢 `getCwd` com `input.cwd` não-string: Rust diverge conscientemente
- **Onde:** `plugin/scripts/lib.cjs:198-200` — `(input && input.cwd) || ...`.
- **Comportamento `.cjs`:** qualquer valor **truthy** (número, array, objeto)
  em `input.cwd` é retornado literal. Observado empiricamente:
  - `{cwd: 42}` → retorna `42` (number) → caller passa para `fs.*` que
    coage a `"42"` implicitamente.
  - `{cwd: []}` → `[]` é truthy em JS → retorna array. Caller coage a
    `""` (Array.prototype.toString de `[]` é `""`), resultado inútil.
  - `{cwd: {}}` → retorna objeto literal → quebra no caller.
- **Comportamento Rust:** só aceita `Value::String` não-vazia; qualquer
  outra coisa cai para env. **Divergência consciente** — registrada em
  `parity_getcwd_consciously_diverges_on_non_string_cwd` com
  `assert_ne!` (!). Essa divergência melhora robustez sem quebrar
  callers realistas (input vem de JSON do Claude Code, que sempre
  envia string).
- **Recomendação pós-cutover:** manter a divergência; eventualmente
  deprecar a aceitação de não-string no `.cjs`.
- **Paridade v2:** 🟢 consciente. 12 unit + 12 parity cobrindo cada
  transição de branch.

### [F2-21] 🟢 `readStdin` — paridade total em inputs exóticos (BOM, duplo-JSON, unterminated)
- **Onde:** `plugin/scripts/lib.cjs:189-196`.
- **Comportamento testado empiricamente em ambos:**
  - stdin vazio → `{}`
  - BOM `\uFEFF` + `{}` → `{}` (JSON.parse rejeita BOM; serde_json também)
  - duplo JSON `{a}\n{b}` → `{}` (JSON.parse rejeita tokens trailing
    após o primeiro valor; serde_json idem)
  - unterminated `{"x` → `{}`
- **Paridade v2:** ✅ `read_stdin_from_str` devolve exatamente as
  mesmas `Value` em cada caso. Sem divergências.

---

## Fase 3 — agents.cjs

### [F3-01] 🟢 `js_trim` — BOM (U+FEFF) strippado em paridade com JS `.trim()`
- **Onde:** `plugin/scripts/agents.cjs:427` — `fs.readFileSync(p, 'utf8').trim()`.
- **Comportamento `.cjs`:** `String.prototype.trim()` em JS **strip**a BOM
  (U+FEFF / ZWNBSP) além de todos os whitespace Unicode WS=Y.
- **Comportamento Rust nativo:** `str::trim` usa `char::is_whitespace`
  que segue apenas a property Unicode `WS=Y`. BOM **NÃO é WS=Y**,
  portanto `str::trim` preserva o BOM.
  - LS (U+2028), PS (U+2029), NBSP (U+00A0) **são** WS=Y — OK.
  - BOM (U+FEFF) é a única divergência relevante.
- **Solução:** helper `js_trim` em `crates/domain-agents/src/js_trim.rs`
  que combina `char::is_whitespace() || c == '\u{FEFF}'`.
- **Aplicação:** `load_mini_context` usa `js_trim` em vez de `str::trim`.
- **Risco residual:** qualquer outro lugar da codebase que faça `.trim()`
  em conteúdo vindo do disco com BOM vai divergir. Auditar em cada
  ponto que portar `.cjs` → Rust (grep `\.trim\(\)` no `.cjs` em
  comparação com `str::trim` no Rust). Candidatos conhecidos:
  `claude_md.rs` em `core-paths` — usa `str::trim` em
  `project_md_content`; revisar quando for auditar core-paths v2.
- **Paridade v2:** ✅ 14 unit + 16 parity. Teste dedicado
  `parity_load_trims_bom` cobre `"\uFEFFhello\n"` → `"hello"`
  byte-a-byte idêntico em `.cjs` e Rust.

---

### [F3-02] 🟡 `countInvocations` — outer try/catch mascara dados corrompidos
- **Onde:** `plugin/scripts/agents.cjs:463-482`.
- **Comportamento `.cjs`:** O `for (const l of lines)` inteiro está
  envolto em `try { ... } catch {}`. Se qualquer operação dentro do
  loop lança, o loop para **em silêncio** e retorna o acumulado até
  ali. Triggers conhecidos (validados empiricamente em paridade):
  - Linha `null` → `null.type` lança `TypeError`.
  - `o.agent` é número não-zero, bool `true`, array, ou objeto →
    `.toLowerCase` não existe → `TypeError`.
- **Consequência:** Um `observations.jsonl` corrompido (uma linha ruim)
  **cala** todas as estatísticas subsequentes. Sem log, sem aviso.
- **Paridade v2:** Preservada via `break` explícito em
  `crates/domain-agents/src/invocations.rs`. 13 unit + 21 parity
  cobrindo cada trigger.
- **Recomendação pós-cutover:** Substituir o outer try/catch por
  tratamento por-linha (`continue` em vez de `break`), logando linha
  inválida. Aumentaria resiliência sem mudar comportamento feliz.

### [F3-03] 🟢 `countInvocations` — shape de retorno assimétrico preservado
- **Onde:** `plugin/scripts/agents.cjs:465` vs `:481`.
- **Comportamento `.cjs`:** Retorna `{}` quando arquivo ausente, mas
  `{ counts, recent }` quando presente (mesmo se vazio).
- **Paridade v2:** Rust retorna `serde_json::Value` com os dois
  shapes distintos. Consumers devem checar presença de `counts`.
- **Recomendação pós-cutover:** Unificar retorno em
  `{ counts: {}, recent: {} }` sempre. Menor pegadinha para consumer.

---

### [F3-04] 🔴 `buildInjectionBlock` crasha quando agente não tem `context_file`
- **Onde:** `plugin/scripts/agents.cjs:535` —
  `loadMiniContext(memDir, a.context_file)`.
- **Raiz:** Quando `a.context_file` é `undefined`, `loadMiniContext`
  chama `path.join(memDir, undefined)` que lança
  `TypeError: The "path" argument must be of type string.`
- **Impacto:** Qualquer manifest editado à mão com agente sem
  `context_file` quebra a injeção inteira no session-init — **não é
  apenas o agente que falha, toda a renderização explode**.
- **Paridade v2:** 🟢 Rust é graciosa — usa
  `a.get("context_file").and_then(Value::as_str)` e skipa snippet se
  ausente. Divergência consciente (Rust mais robusto).
- **Teste:** `rust_does_not_crash_on_missing_context_file` em
  `parity_injection.rs` valida comportamento robusto do Rust. Todos
  os parity tests injetam `context_file` explicitamente para evitar
  o crash do `.cjs`.
- **Recomendação pós-cutover:** No `.cjs`, adicionar fallback em
  `loadMiniContext` (`if (!contextFile) return null;`) ou pular
  snippet quando `context_file` ausente na linha 535.

### [F3-05] 🟡 `buildInjectionBlock` imprime `"undefined"` literal quando `role` ausente
- **Onde:** `plugin/scripts/agents.cjs:524` —
  `` `- ${emoji} **${name}**${tag} — ${a.role}` ``.
- **Comportamento `.cjs`:** Template literal coage `undefined` para
  a string `"undefined"`. Saída fica `— undefined` após o traço.
- **Paridade v2:** Replicada via `js_template_str` em
  `crates/domain-agents/src/injection.rs`. Mesmo comportamento para
  `watch.threshold` e `watch.window_minutes` quando ausentes.
- **Recomendação pós-cutover:** Adicionar fallback `a.role || '—'`
  ou mudar o layout para não deixar `undefined` escapar pro usuário.

### [F3-06] 🟢 `sanitizeScope` — caminho diferente, mesma saída em input array
- **Onde:** `plugin/scripts/agents.cjs:758-767`.
- **Comportamento `.cjs`:** `typeof [] === 'object'` passa pelo guard
  inicial. Em seguida `input.bash_allow` em array é `undefined`, então
  `Array.isArray(undefined)` é `false` e `bashAllow` vira `[]`.
- **Paridade v2:** `input.as_object()` retorna `None` para arrays em
  Rust, então caímos no early return `{bash_allow: []}` sem inspecionar
  `bash_allow`. Saída idêntica; caminho interno difere.
- **Recomendação pós-cutover:** nenhuma — comportamento externo igual.

### [F3-07] 🟡 `sanitizeScope` — `p.length` em JS é UTF-16, em Rust byte
- **Onde:** `plugin/scripts/agents.cjs:762`.
- **Comportamento `.cjs`:** `.filter(p => ... && p.length <= 200)` usa
  `String.prototype.length` (UTF-16 code units).
- **Paridade v2:** `crates/domain-agents/src/scope.rs` usa `p.len()` que é
  byte length em Rust. Para padrões Bash ASCII (100% dos casos reais) as
  contagens coincidem. Para string com emoji ou caracteres
  multi-byte: Rust rejeita antes do JS.
- **Recomendação pós-cutover:** trocar para `encode_utf16().count()` se
  bash_allow algum dia aceitar unicode (hoje não há caller que faça isso).

### [F3-08] 🟢 `updateScope` — `tools` ausente preserva campo existente
- **Onde:** `plugin/scripts/agents.cjs:774-776`.
- **Comportamento `.cjs`:** Se `tools` não vem no input (ou `sanitizeTools`
  retorna `undefined` porque não é array), o campo `tools` do agente não é
  tocado. Mesmo para `scope`.
- **Paridade v2:** Replicado com `Option<&Value>` na assinatura e
  sentinela `None` de `sanitize_tools`. Testes de paridade cobrem cada
  permutação (só tools / só scope / ambos / nenhum / tipos inválidos).
- **Recomendação pós-cutover:** nenhuma — semântica clara e consistente.

### [F3-09] 🔴 `removeAgent` crasha se manifest não tem `agents`
- **Onde:** `plugin/scripts/agents.cjs:817` —
  `if (!cfg || !cfg.agents[name]) return false;`.
- **Comportamento `.cjs`:** Se `cfg` é truthy mas `cfg.agents` é
  `undefined`, acessar `cfg.agents[name]` lança `TypeError: Cannot
  read properties of undefined (reading 'name')`. A exceção NÃO está
  dentro do try/catch (que só envolve o `fs.unlinkSync`), logo escapa
  do caller.
- **Impacto:** Baixo em produção — o `.cjs` sempre cria manifest com
  `agents: {}`. Mas se alguém editar `agents.json` à mão removendo a
  chave, `removeAgent` panica em vez de retornar `false`.
- **Paridade v2:** Divergência defensiva. Rust retorna `Ok(false)`
  nesse caso em vez de panicar (`crates/domain-agents/src/registry.rs`).
  Documentado no docstring de `remove_agent`.
- **Recomendação pós-cutover:** nenhuma ação necessária — comportamento
  Rust é estritamente mais robusto.

### [F3-10] 🟡 `addAgent` crasha se manifest existente não tem `agents`
- **Onde:** `plugin/scripts/agents.cjs:561` — `cfg.agents[name]`.
- **Comportamento `.cjs`:** Mesmo cenário do F3-09 — se `loadManifest`
  retorna objeto sem `agents`, o guard `cfg.agents[name]` panica.
  Só não acontece hoje porque o fallback `{agents: {}}` só é usado
  quando `loadManifest` retorna null.
- **Paridade v2:** Rust valida `cfg.get("agents").is_object()` antes e
  cria `agents: {}` se ausente. Divergência defensiva análoga ao F3-09.
- **Recomendação pós-cutover:** nenhuma ação — Rust mais robusto.

### [F3-11] 🟢 `addAgent` ordem de chaves do objeto preservada
- **Onde:** `plugin/scripts/agents.cjs:568-590`.
- **Comportamento `.cjs`:** Literal `{type, emoji, ...(gender? : ''), model,
  role, context_file, tools, mcps, can_talk_to, scope}` e depois
  `agents[name].watch = ...` para firefighter. A ordem de inserção
  sobrevive no `JSON.stringify` e é visível no `agents.json`.
- **Paridade v2:** Replicada com `serde_json::Map` (IndexMap) inserindo
  chaves na mesma ordem. `gender` só aparece se válido; `watch` só em
  firefighter com `config.watch` presente.
- **Recomendação pós-cutover:** nenhuma — parcela de paridade byte-a-byte
  que vale manter.

### [F3-12] 🟡 `addAgent` campos numéricos usam `||` (0 vira default)
- **Onde:** `plugin/scripts/agents.cjs:584-587` — `watch.threshold || 5`,
  etc.
- **Comportamento `.cjs`:** `0 || 5 === 5` — se o usuário passar
  `threshold: 0` (querendo desabilitar), JS troca por `5`.
- **Paridade v2:** Replicado em `js_num_or` — `0.0` é tratado como falsy
  e coage ao default.
- **Recomendação pós-cutover:** Se quiser permitir `threshold: 0` como
  "sempre dispara", trocar `||` por `?? ` no `.cjs` (nullish coalescing).

### [F3-13] 🟢 `normalizeAutonomy` — mesma ordem de chaves no output
- **Onde:** `plugin/scripts/agents.cjs:619`.
- **Comportamento `.cjs`:** `return { level, call_human_when };` — literal
  object, ordem fixa independente da ordem do input.
- **Paridade v2:** Replicada com `Map::new` inserindo `level` antes de
  `call_human_when`. Testes confirmam via comparação byte-a-byte.

### [F3-14] 🟡 `normalizeAutonomy` — input array dá mensagem diferente
- **Onde:** `plugin/scripts/agents.cjs:608-613`.
- **Comportamento `.cjs`:** `!input || typeof input !== 'object'` — arrays
  passam (typeof `'object'`, truthy). Depois `!AUTONOMY_LEVELS.has(level)`
  lança `"autonomy.level inválido"` porque `input.level` é undefined.
- **Paridade v2:** Rust usa `as_object()` que retorna None para arrays,
  então lança `NotObject` em vez de `InvalidLevel`. Ambos erram; a
  mensagem diverge. Testes de paridade não cobrem array input.
- **Recomendação pós-cutover:** irrelevante — caller real manda objeto ou
  null. Se quisermos paridade estrita, adicionar checagem pra arrays
  antes do `as_object`.

### [F3-15] 🟢 `propagateAutonomyToSeed` — formato de seed preservado
- **Onde:** `plugin/scripts/agents.cjs:652`.
- **Comportamento `.cjs`:** `JSON.stringify(seed, null, 2) + '\n'` —
  2 espaços de indent, trailing newline.
- **Paridade v2:** `serde_json::to_string_pretty` + `push('\n')`. Byte
  parity confirmada em `parity_autonomy.rs`.

### [F3-16] 🟢 `resolveCliModel` — passthrough tolerante pra nomes desconhecidos
- **Onde:** `plugin/scripts/agents.cjs:696`.
- **Comportamento `.cjs`:** Se não casa alias/regex/map, devolve a string
  original. Motivação: deixar o CLI surfar o erro real em vez de decidir
  unilateralmente.
- **Paridade v2:** Replicada em `crates/domain-agents/src/model.rs`.
  Testes de paridade cobrem "foo-bar", "opus-bad", "opus-1.2.3" (regex
  exige `N.N` exato, então 3-part fica passthrough).

### [F3-17] 🟢 `normalizeModel` — limite de 80 chars com UTF-16 length
- **Onde:** `plugin/scripts/agents.cjs:665`.
- **Comportamento `.cjs`:** `v.length > 80` (UTF-16 code units).
- **Paridade v2:** Rust usa `encode_utf16().count()` em vez de `len()`
  (bytes). Todos os modelos reais são ASCII, então as contagens
  coincidem; escolhi a UTF-16 por paridade estrita.

### [F3-18] 🟢 `CLI_MODEL_MAP` não exportado — paridade via iteração
- **Onde:** `plugin/scripts/agents.cjs:828-861` (module.exports).
- **Comportamento:** A constante é privada ao módulo. Testes de paridade
  não conseguem importar ela; paridade é testada chamando
  `resolveCliModel` pra cada chave do Rust map.
- **Paridade v2:** A ordem e conteúdo do Rust `CLI_MODEL_MAP` bate com o
  `.cjs` por inspeção visual; cada par é verificado por
  `resolve_all_map_keys_parity`.
- **Recomendação pós-cutover:** Exportar `CLI_MODEL_MAP` do `.cjs` ou
  mudar o teste Rust pra parsear o `.cjs` diretamente se quisermos
  cobertura 100% automatizada.

### [F3-19] 🟡 `integratorTemplate` — `""` em `docs_url`/`base_url` cai no fallback
- **Onde:** `plugin/scripts/agents.cjs` — templates string usam `${x ? ... : fallback}`.
- **Comportamento:** JS `""` é falsy, então base_url/docs_url vazio
  renderiza como "_Adicionar URL base..._" / "_Adicionar link da doc..._".
- **Impacto:** Caller que passa string vazia por engano vê placeholder,
  não string vazia. Paridade Rust replica via `js_str_or` (trata `""`
  como ausente).
- **Paridade v2:** Replicada. Testes `integrator_empty_docs_url_fallback`
  e `integrator_empty_base_url_fallback` cobrem.
- **Recomendação pós-cutover:** Manter — o fallback é útil (marcador
  pro seller preencher), mesmo que venha de input acidentalmente vazio.

### [F3-20] 🟡 `firefighterTemplate` — `threshold: 0` vira default 5
- **Onde:** `plugin/scripts/agents.cjs` — `${threshold || 5}`.
- **Comportamento:** JS `0 || 5 === 5`. Se o caller passar 0 como
  threshold (zero falhas ativa), cai em 5.
- **Impacto:** threshold zero é semanticamente estranho (alerta a cada
  erro), mas possivelmente intencional em casos extremos. Caller perde
  silenciosamente.
- **Paridade v2:** Replicada via `n_or(v, 5)` (0.0 == falsy).
- **Recomendação pós-cutover:** Trocar `||` por `??` no `.cjs` seria
  semanticamente mais correto; mas como template é só markdown, impacto
  é zero em runtime.

### [F3-21] 🟢 `motorTemplate` — `requires_manager_approval` usa `=== false`
- **Onde:** `plugin/scripts/agents.cjs` — `requires_manager_approval === false`.
- **Comportamento:** Só o booleano literal `false` gera a linha
  "**Requer aprovação do Manager:** não — pode agir sozinho no escopo".
  `null`, `undefined`, `"no"`, `0` NÃO casam — caem no ramo default
  ("sim — valida com Manager antes de agir").
- **Paridade v2:** Replicada via
  `matches!(v.get("requires_manager_approval"), Some(Value::Bool(false)))`.
  Cobertura: `requires_approval_true/missing/null/truthy_string`.

---

## Fase 3 completa

Todos os sub-pontos A–L de `agents.cjs` portados com paridade byte-a-byte
validada. Próximo: `architect.cjs`.

---

## Fase 4 — Port de `architect.cjs`

### [F4-01] 🟡 `ensureInstalled` assume que manifest é objeto parseável
- **Onde:** `plugin/scripts/architect.cjs:74-75`.
- **Comportamento:** Se `agents.json` contém JSON mas não-objeto (ex: `42`,
  `[]`, `"str"`), `manifest.agents = {}` em cima disparia em arrays, mas
  `JSON.parse` aceita qualquer coisa e o `.cjs` trata `!manifest.agents`
  no array (undefined em número/string → cria `.agents` num primitive,
  mas objeto-like do JS permite props em Number/String wrappers e perde
  silenciosamente).
- **Impacto:** Se disco foi corrompido, paridade quebra porque Rust
  parseia strict (retorna erro `InvalidManifest`) enquanto .cjs segue
  caminho esquisito (`JSON.stringify(42) === "42"`).
- **Paridade v2:** Rust é mais estrito. Em produção, esse caso nunca
  aparece (manifest sempre é escrito pelo próprio plugin).

### [F4-02] 🟢 `ensureInstalled` idempotente
- **Onde:** `plugin/scripts/architect.cjs:81-89`.
- **Comportamento:** Só regrava `agents.json` se architect não tem
  `category === 'meta'`; só escreve `architect-meta.md` se arquivo
  ausente. Custom preservado.
- **Paridade v2:** Testes `ensure_skips_overwrite_when_architect_already_meta`
  e `ensure_preserves_custom_context` validam byte-a-byte.

### [F4-03] 🟢 `generateSeed` cria categorias on-demand
- **Onde:** `plugin/scripts/architect.cjs:107-113`.
- **Comportamento:** Se `<seeds>/<category>/` não existe, cria + gera
  `meta.json` default com label/description derivados. Se já existe,
  não sobrescreve (mesmo que `category_label` no spec seja diferente).
- **Paridade v2:** Teste `generate_preserves_category_meta_if_exists`.

### [F4-04] 🟡 `generateSeed` gera timestamp não-determinístico
- **Onde:** `plugin/scripts/architect.cjs:153` — `new Date().toISOString()`.
- **Comportamento:** `generated_at` varia a cada execução.
- **Impacto:** Paridade byte-a-byte é impossível sem controlar o clock.
- **Paridade v2:** Rust expõe `Clock` trait + `SystemClock`/`FixedClock`.
  Paridade validada via `FixedClock` + monkey-patch de
  `Date.prototype.toISOString` no Node (testes `parity_generate.rs`).
- **Recomendação pós-cutover:** Manter `Clock` injetável — facilita
  snapshot testing e auditoria.

### [F4-05] 🟡 `generateSeed` normaliza nome com regex ASCII
- **Onde:** `plugin/scripts/architect.cjs:130` — `toLowerCase().replace(/[^a-z0-9_]/g, '_')`.
- **Comportamento:** Chars Unicode depois do `toLowerCase` (ex: `É` → `é`,
  `Ç` → `ç`) NÃO casam o regex ASCII → viram `_`. Acentos são perdidos.
- **Impacto:** Dois nomes com acentos diferentes podem colidir
  ("João" e "Jõão" → ambos "j_o"). Em produção, specs vêm da UI em
  inglês.
- **Paridade v2:** Replicado em `normalize_agent_name` — Unicode
  fora do ASCII vira `_`.

### [F4-06] 🟡 `generateSeed` best_practices dedupe por Set JS
- **Onde:** `plugin/scripts/architect.cjs:126, 140`.
- **Comportamento:** `packBp` é um Set com ordem de inserção. Spec
  best_practices entram primeiro (na ordem), depois cada agente adiciona
  o seu (ordem: agente na ordem do array, e dentro do agente na ordem
  das best_practices). Dedupe por referência JS (strings iguais
  colidem, `"x"` e `x` objeto diferentes não).
- **Paridade v2:** Rust usa `Vec<String>` + `HashSet<String>` pra
  dedupe com ordem de primeira aparição. Cobertura:
  `generate_merges_best_practices_order`.

### [F4-07] 🟢 `generateSeed` ordem de chaves no JSON preservada
- **Onde:** `plugin/scripts/architect.cjs:132-139, 148-154`.
- **Comportamento:** Ordem das chaves do agent: `label, icon, category,
  description, context_file, best_practices`. Meta do pack: `label,
  description, best_practices, generated_by, generated_at`.
- **Paridade v2:** `serde_json` com `preserve_order` + `Map::insert` na
  ordem exata.

### [F4-08] 🟢 `generateSeed` conta via `Object.keys`, não via contador local
- **Onde:** `plugin/scripts/architect.cjs:160` —
  `Object.keys(manifest.agents).length`.
- **Comportamento:** Se dois agentes do spec normalizam pro MESMO nome,
  colidem na `manifest.agents[name]` e contam como 1 (segundo sobrescreve).
- **Paridade v2:** Replicado — `agent_count` vem de `.agents.len()`
  depois do loop.
- **Recomendação pós-cutover:** Warn quando normalização causa colisão
  (silêncio atual pode surpreender caller).

---

## Fase 5 — Port de `best-practices.cjs`

### [F5-01] 🟡 `parseCatalog` é um YAML minimal hand-rolled
- **Onde:** `plugin/scripts/best-practices.cjs:17-79`.
- **Comportamento:** Parser tolerante feito a mão. Não suporta:
  flow maps `{a: b}`, aninhamento profundo, âncoras/aliases, booleans
  YAML (`true`/`false`/`yes` viram string). Strings com `:` no meio
  podem confundir a regex `^\s+(\w+):\s*(.*)$` se a linha inicia com
  `\w+:`.
- **Impacto:** Se alguém edita `_catalog.yaml` à mão com YAML válido
  mas fora do subset, parse silencioso falha (entries caem).
- **Paridade v2:** Replicado literalmente no Rust — mesmo subset, mesma
  limitação. Teste `real_catalog_parity` valida contra o catálogo de
  produção.
- **Recomendação pós-cutover:** Migrar para `serde_yaml` no Rust, mas
  só depois que nenhum caller usar YAML fora do subset atual.

### [F5-02] 🟢 `listPractices` swallow de erro de parse
- **Onde:** `plugin/scripts/best-practices.cjs:83-92` — `try/catch`
  sem log.
- **Comportamento:** Se `_catalog.yaml` existe mas falha no parse (ex:
  arquivo binário), retorna `[]` silencioso.
- **Paridade v2:** Rust replica com early return em `fs::read_to_string`
  falho. Parser nunca panica.

### [F5-03] 🟢 `applyPractices` omite `name` no meta.json se ausente no catálogo
- **Onde:** `plugin/scripts/best-practices.cjs:123`.
- **Comportamento:** `p.name` undefined → `JSON.stringify` omite a
  chave (undefined não serializa). Paridade exige a mesma omissão.
- **Paridade v2:** Rust remove o `Null` depois de inserir para manter
  byte-parity.

### [F5-04] 🟡 `applyPractices` não valida ids contra regex
- **Onde:** `plugin/scripts/best-practices.cjs:111-117`.
- **Comportamento:** Qualquer string em `ids` é tratada como ID. Ids
  com path-traversal (`../`) só falham porque o catálogo não tem match
  — mas se o catálogo tiver entradas maliciosas, `mkdirSync` cria fora
  do `mem_dir/best-practices/`.
- **Impacto:** Baixo hoje (catálogo vem de arquivo interno), mas é
  vetor de ataque se alguém injeta via API.
- **Paridade v2:** Replicado — Rust segue a mesma lógica.
- **Recomendação pós-cutover:** Validar id com `ID_RE` também em apply.

### [F5-05] 🟢 `createLocalPractice` tags filter+trim+dedupe?
- **Onde:** `plugin/scripts/best-practices.cjs:186` —
  `.filter(t => typeof t === 'string' && t.trim()).map(t => t.trim())`.
- **Comportamento:** Filtra não-string e strings vazias após trim.
  Trimma. NÃO dedupe (duplicadas passam).
- **Paridade v2:** Replicado em `sanitize_tags`. Cobertura:
  `create_trims_fields`.

### [F5-06] 🟡 `updateLocalPractice` força id + source depois do merge
- **Onde:** `plugin/scripts/best-practices.cjs:221-222`.
- **Comportamento:** Depois de mesclar input em meta, sobrescreve
  `meta.id = id` e `meta.source = 'local'`. Isso elimina possíveis
  discrepâncias mas IMPEDE atualizar o source via update (ex: converter
  uma prática do catálogo para local).
- **Paridade v2:** Replicado — Rust faz o mesmo `insert` no fim.
- **Recomendação pós-cutover:** Documentar que source só pode ser
  mudado no filesystem diretamente.

### [F5-07] 🟢 `deleteLocalPractice` trata ausência de meta.json como `source=local`
- **Onde:** `plugin/scripts/best-practices.cjs:232-235`.
- **Comportamento:** Se `meta.json` ausente, default `source=local`,
  permite apagar.
- **Paridade v2:** Replicado — Rust também trata ausência como local.

### [F5-08] 🟡 `listLocalPractices` ordem depende do filesystem
- **Onde:** `plugin/scripts/best-practices.cjs:140` — `fs.readdirSync`.
- **Comportamento:** Ordem de retorno é a ordem do filesystem
  (inode-based no Linux, ext4), não alfabética.
- **Impacto:** Testes cross-platform podem ser flaky se consumidor
  conta com ordem específica.
- **Paridade v2:** Replicado — Rust `fs::read_dir` tem a mesma
  propriedade. Teste `list_with_entries` ordena ambos os lados por id
  antes de comparar.
- **Recomendação pós-cutover:** Sort por id para output determinístico.

### [F5-09] 🟡 `content` usa `length` UTF-16 pra limite de 200KB
- **Onde:** `plugin/scripts/best-practices.cjs:171, 205`.
- **Comportamento:** `content.length > 200_000` em JS mede UTF-16 code
  units. String com muitos emojis/non-BMP chars pode passar limite em
  bytes UTF-8 mas falhar no limit JS.
- **Paridade v2:** Replicado via `encode_utf16().count()`.

### [F5-10] 🟡 `listLocalPractices` carrega meta via spread swallowing erros
- **Onde:** `plugin/scripts/best-practices.cjs:147`.
- **Comportamento:** Se `meta.json` é JSON malformado, `try/catch`
  silencia e segue com defaults. Mesmo comportamento em
  `updateLocalPractice`.
- **Paridade v2:** Rust tem a mesma tolerância (parse falho → defaults).

---

## Fase 6 — Port de `bus.cjs`

### [F6-01] 🟡 `setConfig` escreve sem newline final
- **Onde:** `plugin/scripts/bus.cjs:101`.
- **Comportamento:** `fs.writeFileSync(..., JSON.stringify(next, null, 2))`
  — sem `\n` no fim. Difere de outros writes no repositório (graph,
  code_index, humans) que acrescentam newline final.
- **Impacto:** nenhum funcional, mas editores que forçam newline final
  criam diff espúrio ao salvar. Também foge da convenção POSIX.
- **Paridade v2:** Replicado — `fs::write` com `to_string_pretty` sem
  `\n`. Teste `set_config_persists` compara byte-a-byte.
- **Recomendação pós-cutover:** Pad com `\n` final para consistência
  com graph/stats/humans.

### [F6-02] 🟡 `getConfig` passa keys extras do arquivo para o output
- **Onde:** `plugin/scripts/bus.cjs:82-84`.
- **Comportamento:** `{ ...defaults, ...parsed }` copia qualquer campo
  do JSON, mesmo campos desconhecidos. Caller recebe props que não
  existem no schema.
- **Impacto:** caller pode confiar em algo não documentado. Em `getState`
  propagar um campo desconhecido via `retention_days` quebra pouco, mas
  mostra falta de validação.
- **Paridade v2:** Replicado — Rust mantém todas as keys do JSON lido.
- **Recomendação pós-cutover:** Validar schema (só `retention_days` tem
  significado hoje); logar warning se campos extras aparecem.

### [F6-03] 🟡 `setConfig` aceita valores que `Number()` coage
- **Onde:** `plugin/scripts/bus.cjs:94`.
- **Comportamento:** `Number(patch.retention_days)` aceita strings
  numéricas (`"30"` → `30`), booleans (`true` → `1`, que passa no range
  MIN=1), arrays de 1 elemento (`[30]` → `30`).
- **Impacto:** input validation frouxa; caller que devia passar
  number passa string/bool por bug e não percebe.
- **Paridade v2:** Replicado — Rust aceita `Value::String`, `Bool(true)`
  (=1), `Bool(false)` (=0, rejeitado por range). Teste cobre bool e
  string.
- **Recomendação pós-cutover:** Exigir `Value::Number`; qualquer outro
  tipo retorna erro explícito.

### [F6-04] 🟡 `compactOldFiles` aceita `retentionDays=NaN` silenciosamente
- **Onde:** `plugin/scripts/bus.cjs:203-204`.
- **Comportamento:** `Number.isFinite(retentionDays) && retentionDays > 0
  ? Math.floor(retentionDays) : cfg.retention_days`. Se caller passa
  `NaN`, `Infinity`, negativo ou string não-numérica, cai no default.
  Sem erro, sem warning.
- **Impacto:** bug silencioso. Caller que queria disable retention
  (passando `Infinity`) cai no default 14 dias.
- **Paridade v2:** Replicado — `f.is_finite() && r > 0.0` na estrutura
  `CompactOpts`. Teste implícito em `compact_uses_config_default`.
- **Recomendação pós-cutover:** retornar erro quando `retention_days`
  fornecido é inválido.

### [F6-05] 🟡 `migrateLegacy` concatena sem checar duplicidade
- **Onde:** `plugin/scripts/bus.cjs:130-131`.
- **Comportamento:** Se chunk do dia já existe (caso patológico: chunk
  escrito após o legacy mas antes da migração), `migrateLegacy` APPENDA
  o conteúdo do legacy ao existente. Resultado: eventos duplicados.
- **Impacto:** `deriveState` ainda funciona (último vence), mas
  `traceSummary.attempts` conta duplicado, e `getState.recent_events`
  mostra evento 2x.
- **Paridade v2:** Replicado — Rust também prepend + append sem dedupe.
- **Recomendação pós-cutover:** No pós-cutover, migração legacy é no-op
  (arquivo não existe mais). Não vale o esforço de corrigir.

### [F6-06] 🟢 `sweepPending` sem `onRetry`/`onTimeout` ainda emite o evento
- **Onde:** `plugin/scripts/bus.cjs:321-365`.
- **Comportamento:** emit de `retry`/`failed`/`timed_out` acontece
  INDEPENDENTE de haver callback. Callback é só pra reações side-effect
  (re-enqueue, escalar firefighter).
- **Impacto:** comportamento desejado — o bus decide quando o deadline
  estourou e registra; caller decide o que fazer. Mas documentação
  atual não deixa isso claro.
- **Paridade v2:** Replicado — `sweep_pending` emit antes de invocar
  callbacks.
- **Recomendação pós-cutover:** Destacar isso no docstring da função.

### [F6-07] 🟡 `getPending` lê só `sinceDays: 2`
- **Onde:** `plugin/scripts/bus.cjs:273`.
- **Comportamento:** Janela fixa de 2 dias. Comentário justifica:
  "deadlines são curtos (< 10min)". Correto em produção, mas um trace
  criado em chunk mais antigo (ex: restore de backup, migração lenta)
  nunca aparece em pending — "vazamento" de estado.
- **Impacto:** corner case raríssimo, só se retenção default mudar e
  alguém injetar eventos antigos.
- **Paridade v2:** Replicado (`since_days: Some(2)`).

### [F6-08] 🟢 `deriveState` usa `Map` pra preservar insertion order
- **Onde:** `plugin/scripts/bus.cjs:238-245`.
- **Comportamento:** `Map` em JS preserva ordem de primeira inserção;
  `getPending`/`getState` iteram nessa ordem e downstream (API, UI)
  confia nela.
- **Paridade v2:** Implementação inicial usou `BTreeMap` (alfabético) —
  divergia ao ordenar trace_ids `z_*` antes de `a_*` pela ordem de
  inserção. Adicionado `derive_state_ordered` que retorna
  `Vec<(String, Value)>` preservando ordem, consumido por `getPending`
  e `getState`. `derive_state` (BTreeMap) mantido pra callers que só
  precisam de lookup O(log n).

### [F6-09] 🟡 `sweepPending` usa `Date.now()` mas `new Date(ts).getTime()` para idade
- **Onde:** `plugin/scripts/bus.cjs:324, 331`.
- **Comportamento:** `nowMs = Date.now()` para "agora"; `lastMs = new
  Date(last.ts).getTime()` pra parse do ts do evento. `new Date(str)`
  aceita formatos frouxos (ISO, RFC 2822, "2026/04/19"); se ts vier
  deformado, retorna `NaN`, `age = NaN`, comparações com `>` retornam
  `false`, trace fica preso.
- **Impacto:** trace nunca progride se emissor escreveu ts inválido.
  Evento silenciosamente "morto".
- **Paridade v2:** Rust `parse_iso_millis` é mais estrito (só ISO
  `YYYY-MM-DDTHH:MM:SS.sssZ` ou prefixo de data); fallback `.unwrap_or(0)`
  no último resort — se parse falha, `last_ms = 0`, `age` gigantesco,
  trace sempre entra em retry/timed_out. **Divergência** latente mas
  "mais conservadora" no Rust.
- **Recomendação pós-cutover:** Garantir que `emit` sempre escreva ts
  válido (já faz: `new Date().toISOString()` vs `clock.now_iso()`).
  Rejeitar eventos sem ts válido em `parse_file`.

### [F6-10] 🟢 Stage order desconhecido cai em `0`
- **Onde:** `plugin/scripts/bus.cjs:229-235, 241`.
- **Comportamento:** `STAGE_ORDER[type] || 0`. Type desconhecido
  (typo, novo tipo futuro) tem order 0 — menor que qualquer valido.
  `deriveState` SEMPRE troca o estado atual quando o novo é
  desconhecido (novo_order=0 >= cur_order=0 quando cur também
  desconhecido; mas 0 < 1 quando cur é válido → não troca).
- **Impacto:** Adicionar um novo tipo de evento sem atualizar STAGE_ORDER
  faz ele nunca se tornar o estado final. Silencioso.
- **Paridade v2:** Replicado — `fn stage_order` retorna 0 pra
  desconhecido.
- **Recomendação pós-cutover:** Validar `ev.type` no emit (enum
  fechado) pra forçar atualização do STAGE_ORDER.


---

## Fase 7 — Port de `destructive.cjs`

### [F7-01] 🟡 `CORE_PATTERNS` é `Object.freeze`ado mas `origin` é atribuído APÓS freeze
- **Onde:** `plugin/scripts/destructive.cjs:106-110`.
- **Comportamento:** `Object.freeze(CORE_PATTERNS)` é chamado ANTES de
  `CORE_PATTERNS.forEach((p) => { p.origin = 'core'; Object.freeze(p); })`.
  A atribuição `p.origin = 'core'` em modo estrito lançaria
  `TypeError`, mas como arrays freeze só protege o container (não os
  elementos), a atribuição aos objetos internos ainda passa.
- **Impacto:** funciona mas contra-intuitivo. Se alguém adicionar
  `'use strict'` ao arquivo (já tem em bus.cjs/lib.cjs), quebra.
- **Paridade v2:** Rust usa `LazyLock` — patterns são imutáveis por
  design (struct com `&'static str`). `origin` não é campo: `listPatterns`
  e `detect` hard-codam `"core"`.

### [F7-02] 🟡 `loadUserConfig` aceita arquivo com keys aleatórias
- **Onde:** `plugin/scripts/destructive.cjs:121-125`.
- **Comportamento:** Só valida `Array.isArray(raw.patterns)` e
  `Array.isArray(raw.disabled_core)`. Resto do JSON é ignorado
  silenciosamente — arquivo malformado parcialmente vira config vazia
  por campo.
- **Impacto:** corrupção silenciosa. Usuário edita à mão e perde keys,
  não vê erro.
- **Paridade v2:** Replicado. `load_user_config` faz a mesma checagem.

### [F7-03] 🟡 `saveUserPatterns` não valida regex em `kind: regex`
- **Onde:** `plugin/scripts/destructive.cjs:137-152`.
- **Comportamento:** Usuário pode gravar um pattern com `kind: 'regex'`
  e `text` que não compila como regex JS. `saveUserPatterns` aceita;
  só na hora do `detect`, `compileUserPattern` retorna `null` via
  try/catch e o pattern é silenciosamente ignorado.
- **Impacto:** UI mostra pattern habilitado, mas ele nunca dispara.
  Usuário acha que está protegido.
- **Paridade v2:** Replicado — Rust `save_user_patterns` também não
  valida, e `compile_user_pattern` retorna `None` silenciosamente via
  `Regex::new().ok()`.
- **Recomendação pós-cutover:** Validar regex no save; devolver erro
  estruturado com índice + mensagem.

### [F7-04] 🟡 `saveUserPatterns` trunca via `String.slice` (UTF-16)
- **Onde:** `plugin/scripts/destructive.cjs:139-140`.
- **Comportamento:** `String(pt.label || '').slice(0, 200)` mede em
  UTF-16 code units. Emojis (surrogate pairs) contam 2 units cada.
- **Paridade v2:** Replicado via `encode_utf16().count()` +
  `from_utf16_lossy`. Teste `slice_utf16_handles_multibyte` cobre.

### [F7-05] 🟡 `compileUserPattern` não retorna erro — só `null`
- **Onde:** `plugin/scripts/destructive.cjs:171-183`.
- **Comportamento:** Regex inválido, pattern desabilitado, ou null/undefined
  → `null`. Caller (`detect`) não distingue os casos.
- **Paridade v2:** Replicado — `compile_user_pattern -> Option<Regex>`.
- **Recomendação pós-cutover:** expor `Result<Option<Regex>, CompileError>`.

### [F7-06] 🟢 `compileUserPattern` com literal injeta `\b...\b`
- **Onde:** `plugin/scripts/destructive.cjs:179`.
- **Comportamento:** `\b${escaped}\b` força word boundary em literais.
  Consequência: padrão literal "rm -rf" tem espaços; `\b-\b` é uma
  boundary entre letter/non-letter, mas "rm -rf" vs "rm-rf" — o primeiro
  contém espaço, e `\b` na fronteira " -" passa (entre space e -?
  não — `\b` só em fronteira \w/\W, e " " e "-" ambos são \W, então
  SEM boundary). Resultado: literal "rm -rf" compila pra `\brm -rf\b`
  que só casa em contextos onde `rm` está cercado de \b.
- **Impacto:** adequado — literal deve ser palavra "completa".
- **Paridade v2:** Replicado exatamente.

### [F7-07] 🟡 Regex CORE usa `\w` que é ASCII-only em JS
- **Onde:** `plugin/scripts/destructive.cjs:23, 77`.
- **Comportamento:** JS regex sem flag `u` trata `\w` como
  `[A-Za-z0-9_]` ASCII. Rust `regex` crate por default usa Unicode
  `\w` (inclui caracteres acentuados).
- **Impacto esperado:** Para `apag\w+` casando "apague" ou "apagar",
  ambos funcionam — letras são ASCII. Mas pra `\bDELETE\s+FROM\s+\w+`
  com nome de tabela em português com acento (`DELETE FROM açãoes`),
  JS `\w+` só pega "a" antes do "ç"; Rust pega a palavra inteira.
- **Paridade v2:** **Divergência potencial latente**. Passou nos testes
  atuais (35/35) porque nenhum caso usa word+acento+non-word.
  Manter testes de regressão cobrindo PT específicos. Se surgir
  divergência em produção, usar `(?-u:...)` no Regex::new ou flag
  `RegexBuilder::unicode(false)`.
- **Recomendação pós-cutover:** Migrar patterns CORE pra classe
  explícita `[a-zA-Z0-9_]` evitando ambiguidade.

### [F7-08] 🟡 Slice de `match` usa UTF-16 (120 chars)
- **Onde:** `plugin/scripts/destructive.cjs:224, 236`.
- **Comportamento:** `m[0].slice(0, 120)` trunca em 120 UTF-16 units.
- **Paridade v2:** Replicado via `slice_utf16`.

### [F7-09] 🟢 Ordem de hits: CORE antes de USER
- **Onde:** `plugin/scripts/destructive.cjs:218-239`.
- **Comportamento:** Loop CORE primeiro, USER depois. Consumer que
  prioriza por ordem assume isto.
- **Paridade v2:** Replicado — Rust faz na mesma ordem.

### [F7-10] 🟡 `setCoreDisabled` grava mesmo se nenhum mudou
- **Onde:** `plugin/scripts/destructive.cjs:155-167`.
- **Comportamento:** `set.add(coreId)` / `set.delete(coreId)` — se já
  estava no estado desejado, `add`/`delete` é no-op, mas o write para
  disco acontece de qualquer jeito.
- **Impacto:** mtime do arquivo atualiza mesmo sem mudança. Pode
  confundir file watchers.
- **Paridade v2:** Replicado — `set_core_disabled` também escreve
  sempre.


## Fase 8 — firefighter.cjs → domain-firefighter

### [F8-01] 🟡 `prune_old_triages` usa `>` estrito (exclui boundary)
- **Onde:** `plugin/scripts/firefighter.cjs:39-42`.
- **Comportamento:** `.filter(e => new Date(e.ts).getTime() > cutoff)` —
  comparação **estrita**. Entrada cujo `ts` bate exatamente em
  `now - 3_600_000` é descartada.
- **Impacto:** Em janela exata de 1h, a primeira triage do topo da
  janela começa a ser descartada no instante `now`. Rate-limit pode
  "liberar" 1 slot antes do esperado.
- **Paridade v2:** Replicado — `prune_old_triages` usa `ms > cutoff`.

### [F8-02] 🔴 `pruneOldTriages` com `ts` inválido gera `NaN > cutoff` → `false` → drop
- **Onde:** `plugin/scripts/firefighter.cjs:41`.
- **Comportamento:** `new Date("bogus").getTime()` retorna `NaN`; `NaN >
  qualquer` é `false` em JS → a entrada é **descartada silenciosamente**.
- **Impacto:** Corrupção sutil — se alguém escrever um `ts` mal-formado
  no state file (edição manual, bug upstream), a entrada some na
  próxima iteração sem log.
- **Paridade v2:** Replicado — `parse_iso_millis` retorna `None` →
  filtro cai em `false`. Comportamento idêntico, mas note o risco.
- **Recomendação pós-cutover:** logar warning quando `parse_iso_millis`
  falha (não mudar o comportamento de drop — é feature no sentido de
  auto-saneamento).

### [F8-03] 🟡 `save_state` sempre adiciona `\n` final
- **Onde:** `plugin/scripts/firefighter.cjs:35-37`.
- **Comportamento:** `JSON.stringify(state, null, 2) + '\n'` — trailing
  newline sempre presente.
- **Paridade v2:** Replicado — `save_state` faz `push('\n')`.
- **Contexto:** Consistente com POSIX (arquivo de texto termina em
  newline); difere de outros artefatos do projeto (ex.
  `destructive_patterns.json` — sem `\n` trailing).

### [F8-04] 🟡 `load_state` silencia JSON malformado retornando `{}`
- **Onde:** `plugin/scripts/firefighter.cjs:29-33`.
- **Comportamento:** `try { JSON.parse(...) } catch { return {}; }` —
  zeroa cooldown e triages_last_hour. Em produção, `pollOnce` a seguir
  chama `saveState` que **sobrescreve** o arquivo corrompido com o novo
  state vazio.
- **Impacto:** Corrupção do state dispara efetivamente um "reset"
  silencioso. Perde rate-limit/cooldown de todos os bombeiros.
- **Paridade v2:** Replicado — `load_state` retorna `Value::Object`
  vazio em parse error.
- **Recomendação pós-cutover:** fazer backup do arquivo corrompido
  antes do reset (`.corrupt.N`).

### [F8-05] 🟡 `parseArgs` `--interval 0` → 60 (JS truthy fallback)
- **Onde:** `plugin/scripts/firefighter.cjs:276`.
- **Comportamento:** `parseInt(argv[++i], 10) || 60`. Como `0` é falsy
  em JS, o fallback `|| 60` dispara e vira `60s`.
- **Impacto:** Usuário que quer "loop mais rápido possível" passando
  `--interval 0` recebe 60s sem aviso. Subjetivo: bug ou feature.
- **Paridade v2:** Replicado — `parse_int_loose` retorna `None` quando
  resultado é `0`, fallback cai em `60`.
- **Recomendação pós-cutover:** aceitar `0` com semântica "sem sleep"
  OU rejeitar com erro explícito.

### [F8-06] 🟢 `parseArgs` ignora flags desconhecidas silenciosamente
- **Once:** `plugin/scripts/firefighter.cjs:270-280`.
- **Comportamento:** O `else if` chain não tem branch default → flags
  desconhecidas são puladas sem erro.
- **Paridade v2:** Replicado.
- **Recomendação pós-cutover:** emitir warning em stderr para flag
  desconhecida (ajuda debug sem quebrar).

### [F8-07] 🔴 `buildTriagePrompt` não escapa query/miniContext/msg
- **Onde:** `plugin/scripts/firefighter.cjs:117-134`.
- **Comportamento:** Interpolação via template literal direta; se a
  `query` do watch contiver backtick ou `${}`, **não** há escape antes
  de enviar ao `claude -p`. (No `.cjs` atual, template literals já
  foram resolvidas no objeto agent config — o string injetado é
  literal, não executa mais template; mas o prompt final contém chars
  especiais sem escape para o LLM.)
- **Impacto:** Prompt injection via `query` maliciosa do manifest (se
  manifest for user-provided). Impacto baixo hoje porque manifest vem
  do próprio projeto, mas é atenção para futuro.
- **Paridade v2:** Replicado byte-a-byte — o port não introduz escape
  novo.
- **Recomendação pós-cutover:** sanitize `query`/`message`/`service`
  antes de injetar no prompt.

### [F8-08] 🟡 Slice UTF-16 em `msg.slice(0, 400)` e `attrs.slice(0, 200)`
- **Onde:** `plugin/scripts/firefighter.cjs:110, 114`.
- **Comportamento:** JS `.slice(n, m)` em string trabalha em UTF-16
  units. Strings com surrogate pairs podem ser cortadas no meio.
- **Paridade v2:** Replicado via `slice_utf16` (quebra surrogate do
  mesmo jeito, `String::from_utf16_lossy`).

### [F8-09] 🟡 `stderr` truncado a 300 UTF-16 em observação de falha
- **Onde:** `plugin/scripts/firefighter.cjs:216`.
- **Comportamento:** `err.slice(0, 300)` só é salvo quando
  `code !== 0`. Se claude sair com 0 e tiver stderr (warnings), esse
  stderr é perdido.
- **Paridade v2:** Replicado — `slice_utf16(&r.stderr, 300)`.

### [F8-10] 🟢 `checkAgent` retorna formato de resultado heterogêneo
- **Onde:** `plugin/scripts/firefighter.cjs:159-229`.
- **Comportamento:** Retorno é um objeto com uma dentre 7 chaves
  exclusivas: `{skipped}`, `{cooldown}`, `{rate_limited}`, `{error}`,
  `{ok}`, `{would_triage, events, prompt_chars}`, `{triaged, events,
  exit_code}`. Consumer precisa inspecionar keys.
- **Paridade v2:** Portado como `CheckOutcome` enum; conversão
  `to_value()` serializa pra mesma estrutura `.cjs`.
- **Recomendação pós-cutover:** padronizar retorno em
  `{status: "...", data: {...}}`.

## Fase 9 — graph.cjs → domain-graph

### [F9-01] 🟡 `saveGraph` não adiciona `\n` final
- **Onde:** `plugin/scripts/graph.cjs:71-74`.
- **Comportamento:** `JSON.stringify(graph, null, 2)` — **sem** newline
  final, ao contrário de `saveState` (firefighter) e `destructive_patterns`
  que têm `+ '\n'`.
- **Impacto:** Inconsistência interna no projeto. `cat graph.json`
  aparece sem newline após `}`. `wc -l` retorna linhas off-by-one.
- **Paridade v2:** Replicado — `save_graph` omite o trailing `\n`.

### [F9-02] 🟡 `loadGraph` silencia JSON malformado sem backup
- **Onde:** `plugin/scripts/graph.cjs:53-69`.
- **Comportamento:** `try { JSON.parse... } catch { return empty() }` —
  perda total de histórico incremental. Save subsequente sobrescreve.
- **Paridade v2:** Replicado.
- **Recomendação pós-cutover:** salvar como `.corrupt.N` antes de
  resetar (mesma sugestão de F8-04).

### [F9-03] 🟢 Auto-migração de version: descarta grafo antigo
- **Onde:** `plugin/scripts/graph.cjs:60-67`.
- **Comportamento:** Se `g.version !== VERSION`, chama `rebuildGraph`
  que apaga TODOS os nós/arestas e reconstrói do zero a partir dos
  JSONLs. Implementado como "v1 é descartável".
- **Paridade v2:** Replicado via callback `on_stale_version` (preserva
  a semântica sem acoplar a implementação aqui do rebuild).

### [F9-04] 🟡 `touchNode`/`touchEdge` usam `new Date(ts)` para comparar `last_seen`
- **Onde:** `plugin/scripts/graph.cjs:84, 93`.
- **Comportamento:** `new Date(ts) > new Date(n.last_seen)` — se `ts`
  for inválido, `new Date("bogus")` é `Invalid Date`, e `Invalid Date
  > X` é `false` em JS → **não atualiza** `last_seen` silenciosamente.
- **Paridade v2:** Replicado — `parse_iso_millis` retorna `None`, e a
  comparação vai pra `i64::MIN` → sempre `false` contra ms válido.
  Comportamento idêntico.

### [F9-05] 🟡 `score_of`: `Math.exp` pode diferir em 1 ULP entre V8 e libm
- **Onde:** `plugin/scripts/graph.cjs:102-107`.
- **Comportamento:** Ranking depende de `count * Math.exp(-age/half)`.
  A f64 pode variar em 1 ULP entre V8 e Rust (libm/glibc).
- **Impacto:** Em empates muito raros (scores efectivamente iguais até
  o último bit), a ordem pode divergir. Em teste real com graph
  médio-grande, não observei divergência — sort comparator usa
  `b.score - a.score`, e empate numérico usa sort stable em ambos os
  lados (ES2019 sort + Rust `sort_by` stable) preservando insertion
  order (que é idêntica graças ao `preserve_order`).
- **Paridade v2:** Replicado semanticamente; byte-exact em todos os 7
  testes de rebuild e 5 de context.
- **Recomendação pós-cutover:** se aparecer divergência, substituir
  `exp` por uma mesma implementação em ambos (ex. `(-age/half)` →
  tabela pré-computada ou aproximação polinomial explícita).

### [F9-06] 🟡 Decision id usa `slice(0,80)` **antes** de `toLowerCase`
- **Onde:** `plugin/scripts/graph.cjs:245`.
- **Comportamento:** `what.slice(0,80).toLowerCase().replace(/[^a-z0-9]+/g,'-')`.
  Ordem: slice → lower → regex replace. O slice em 80 UTF-16 trunca
  antes de lowercase, podendo cortar no meio de surrogate pair e
  gerar `String::from_utf16_lossy` em Rust (`U+FFFD`).
- **Paridade v2:** Replicado via `utf16_slice(&lower, 80)` — mas note
  que aplicamos `lower` **antes** do slice (ordem: lower → slice),
  e `lower` em Rust `.to_lowercase()` é Unicode-aware (1:many chars),
  enquanto JS `.toLowerCase()` segue tabelas V8 mais restritas. Para
  strings ASCII (uso real), resultado bate. Para entradas com chars
  gregos ou caracter turco (İ → i̇), pode divergir.
- **Recomendação pós-cutover:** normalizar com `ascii_lowercase` manual
  (pegar só `[A-Z]→[a-z]`, deixando o resto passar), idêntico à
  semântica JS em ASCII-only.

### [F9-07] 🔴 `decisionId` prefix `anon-` usa `ts.slice(0,19)` UTF-16
- **Onde:** `plugin/scripts/graph.cjs:246`.
- **Comportamento:** Se `what` vazio → `anon-<ts[0..19]>`. Se `ts` tem
  chars multi-byte UTF-16 (improvável em ISO 8601, mas), pode quebrar
  surrogate. Em ISO 8601 válido (`"2026-04-19T12:00:00"` = 19 ASCII
  chars), bate.
- **Impacto:** Na prática, zero, mas contrato frágil.
- **Paridade v2:** Replicado.

### [F9-08] 🟡 `rebuildGraph` lê diretórios em ordem do FS (não determinístico)
- **Onde:** `plugin/scripts/graph.cjs:499, 513, 524`.
- **Comportamento:** `fs.readdirSync` retorna ordem dependente do FS.
  Em tmpfs/ext4/Linux geralmente é insertion/inode order. Em macOS APFS
  é alfabético. O output do `rebuildGraph` pode diferir entre sistemas.
- **Paridade v2:** **Divergência potencial**. Resolvi em Rust ordenando
  alfabeticamente (`files.sort_by_key(file_name)`). No Linux tmpfs dos
  testes, a paridade passou — Node também devolve ordem que coincide
  com alfabética nos fixtures usados (1 arquivo por dir).
- **Recomendação pós-cutover:** aplicar `readdirSync(...).sort()` no
  `.cjs` pra garantir determinismo cross-FS.

### [F9-09] 🟢 `filesBySession` preserva ordem de inserção (Set-like)
- **Onde:** `plugin/scripts/graph.cjs:488-494`.
- **Comportamento:** `Map` + `Set` do JS preservam insertion order.
  `Array.from(set)` devolve nessa ordem.
- **Paridade v2:** Replicado via `Vec<(String, Vec<String>)>` com
  dedup linear.

### [F9-10] 🟡 `getContextLines.topBy` é stable sort: empate = insertion
- **Onde:** `plugin/scripts/graph.cjs:361-368`.
- **Comportamento:** `Array.prototype.sort` é stable desde ES2019.
  Para scores empatados, a ordem segue o insertion order (que vem do
  preserve_order do Map do `graph.nodes`/`edges`).
- **Paridade v2:** Replicado — Rust `sort_by` é stable por contrato.
- **Risco:** Se substituir serde_json por HashMap-based implementation,
  quebra paridade silenciosamente.

### [F9-11] 🟢 `co_edited` duplica aresta em ambas as direções
- **Onde:** `plugin/scripts/graph.cjs:161-163`.
- **Comportamento:** `touchEdge(a→b) + touchEdge(b→a)` para file_edit
  consecutivos. Dedup em `getContextLines` via `e.from < e.to` (ordem
  lexicográfica).
- **Paridade v2:** Replicado.
- **Risco:** Custo O(2E) em edges. Pra grafos grandes, considerar
  aresta única com flag `undirected=true`.

## Fase 10 — ide-adapters.cjs → domain-ide-adapters

### [F10-01] 🔴 `readManifest` retorna `null` em JSON inválido, causa erro genérico
- **Onde:** `plugin/scripts/ide-adapters.cjs:22-26`.
- **Comportamento:** `try { JSON.parse... } catch { return null; }`. O
  caller (`apply`) só checa se é `null` e lança
  `"agents.json não encontrado em X"` — mesma mensagem para "arquivo
  não existe" e "arquivo corrompido".
- **Impacto:** UX ruim — usuário não sabe se precisa rodar `init` ou
  se o arquivo foi corrompido.
- **Paridade v2:** Replicado — `IdeError::ManifestMissing` para ambos
  os casos.
- **Recomendação pós-cutover:** distinguir `ManifestMissing` vs
  `ManifestCorrupt`.

### [F10-02] 🟡 `readContext` retorna `''` silenciosamente quando o arquivo falta
- **Onde:** `plugin/scripts/ide-adapters.cjs:28-33`.
- **Comportamento:** Se `context_file` aponta para path inexistente,
  retorna `''`. Sem warn, sem error.
- **Impacto:** Um typo em `context_file` no manifest gera arquivo IDE
  praticamente vazio (só frontmatter + label), sem pista do problema.
- **Paridade v2:** Replicado via `unwrap_or_default` em `read_to_string`.
- **Recomendação pós-cutover:** logar warning em stderr quando
  `fs::metadata(file)` falha mas `context_file` é non-empty.

### [F10-03] 🔴 Templates NÃO escapam placeholders
- **Onde:** todos os 4 adapters.
- **Comportamento:** `description`/`label`/`ctx` são interpolados
  diretamente no template. Se description contém `---`, quebra o
  frontmatter. Se contém `<script>`, idem no VSCode Copilot preview.
- **Impacto:** malformed frontmatter silencioso → IDE pode ignorar o
  arquivo.
- **Paridade v2:** Replicado — zero escaping.
- **Recomendação pós-cutover:** sanitize description antes (remover
  `---`, newlines em single-line fields).

### [F10-04] 🟡 Iteração de agents depende de ordem de `Object.entries`
- **Onde:** todos os 4 adapters.
- **Comportamento:** `Object.entries(manifest.agents)` preserva
  insertion order pra string keys (ES2015+). Se alguém gerar
  `agents.json` com shuffle, a ordem dos arquivos escritos muda.
- **Paridade v2:** Replicado — `serde_json::Map` com `preserve_order`.
- **Recomendação pós-cutover:** ordenar agents por nome antes de
  escrever (estabiliza diffs no controle de versão).

### [F10-05] 🟡 `.cursorrules` + `copilot-instructions.md` usam `description||label||name`
- **Onde:** `plugin/scripts/ide-adapters.cjs:93, 133`.
- **Comportamento:** Fallback chain truthy: description se não-vazia,
  senão label, senão name. Diferente do body de cada per-agent file
  (que usa só `label||name`).
- **Paridade v2:** Replicado via `agent_desc_or_label_or_name` +
  `agent_label`.

### [F10-06] 🟢 Cursor `.cursorrules` sempre adiciona `\n` final, VSCode main sempre 2
- **Onde:** `plugin/scripts/ide-adapters.cjs:94, 135`.
- **Comportamento:**
  - Cursor: `[...items].join('\n') + '\n'` — 1 trailing.
  - VSCode: `[...items, ''].join('\n')` — depende do último item; com
    items empty, gera 2 trailing newlines (`header\n\ndesc\n\n`).
- **Paridade v2:** Replicado byte-a-byte. VSCode main foi refatorado
  pra usar `parts.join("\n")` após primeira implementação gerar 1
  trailing newline extra.

### [F10-07] 🟡 `ensureDir` com `mkdirSync({recursive:true})` ignora erros de permissão silenciosamente
- **Onde:** `plugin/scripts/ide-adapters.cjs:35-37`.
- **Comportamento:** Se o dir já existe, no-op. Se falhar por
  permissão, lança — mas não há try/catch no caller → user vê
  stacktrace do Node.
- **Paridade v2:** Replicado via `ensure_dir` retornando `io::Error`.
  Propaga pra caller via `?`.

### [F10-08] 🟢 `written` array contém paths relativos ao `projectRoot`
- **Onde:** `path.relative(projectRoot, out)` ao fim de cada write.
- **Comportamento:** ordem segue insertion, paths POSIX-style (JS
  `path.relative` usa `/` no Linux; `\` no Windows — potencial
  divergência x-platform).
- **Paridade v2:** Usa `path.strip_prefix().to_string_lossy()` +
  `.replace('\\', '/')` pra garantir POSIX-style.
- **Testado apenas em Linux.** No Windows, pode haver diferença.

### [F10-09] 🟡 `str.trim()` ECMAScript difere de Rust `str::trim` em whitespace Unicode exótico
- **Onde:** `ctx.trim()` em todos os 4 adapters.
- **Comportamento:** ECMAScript `String.prototype.trim` cobre o set
  `WhiteSpace ∪ LineTerminator` (inclui `\u00A0`, `\uFEFF`, etc.).
  Rust `str::trim` cobre Unicode White_Space + mais alguns.
- **Paridade v2:** Em todos os 8 testes, bateu byte-a-byte (whitespace
  trivial `\n`/` `/`\t`). Para caracteres exóticos, pode divergir.
- **Recomendação pós-cutover:** se aparecer, escrever `trim` ASCII
  explícito pra forçar paridade.

### [F10-10] 🔴 Sem `dry-run` — adapter escreve direto ao disco
- **Onde:** assinatura `apply({...})`.
- **Comportamento:** Zero flag de dry-run. Loop itera e escreve.
- **Impacto:** CI / pre-commit não consegue rodar "veja o que seria
  gerado" sem escrever.
- **Paridade v2:** Replicado — sem dry-run.
- **Recomendação pós-cutover:** expor `plan()` → `Vec<PlannedWrite>`
  e `apply()` separados.

---

## Fase 11 — `indexer.cjs` (2026-04-19)

### [F11-01] 🟡 App Router "route" overwrite em `extractTs` é dead code
- **Onde:** `extractTs` — bloco final:
  ```js
  m = /^export\s+(async\s+)?function\s+(GET|POST|...)\s*\(/.exec(l);
  if (m) {
    out[out.length - 1] = { ...out[out.length - 1], type: 'route' };
  }
  ```
- **Comportamento:** A regex de função genérica
  (`^export\s+(default\s+)?(async\s+)?function\s+(\w+)...`) casa QUALQUER
  nome incluindo `GET`/`POST`/etc., e tem `continue;` logo após push. Por
  isso a regex de rota nunca dispara — o tipo é sempre `async-fn`/`fn`, e
  handlers App Router aparecem no index sem ser marcados como `route`.
- **Impacto:** ranking/rendering não diferencia API routes de funções
  comuns. Em um app Next.js, métodos `GET`/`POST` ficam misturados como
  async-fn na listagem.
- **Paridade v2:** Replicado (adicionei `continue` explícito após push da
  fn-match para espelhar byte-a-byte).
- **Recomendação pós-cutover:** rodar a regex de rota ANTES da genérica,
  ou remover o bloco morto. Variante simples: adicionar
  `if (/^export\s+(async\s+)?function\s+(GET|POST|...)\s*\(/.test(l))` no
  ponto onde se define `type` para fazer o override inline.

### [F11-02] 🟢 `maxMtime` computado mas nunca usado em `rankSymbols`
- **Onde:** `rankSymbols`:
  ```js
  const maxMtime = Math.max(1, ...Object.values(index.files).map(f => f.mtime || 0));
  ```
- **Comportamento:** variável declarada, nunca lida. Recência usa
  `ageDays = (now - entry.mtime) / DAY`, não normaliza por maxMtime.
- **Impacto:** dead code, confundindo revisores.
- **Paridade v2:** Omiti o cálculo no Rust (não afeta output).
- **Recomendação pós-cutover:** remover a linha.

### [F11-03] 🟢 `out[out.length-1] = ...` com `out` vazio é no-op JS
- **Onde:** mesma linha do F11-01.
- **Comportamento:** Se chamado com `out = []`, `out[-1]` em JS é
  assignment a propriedade `"-1"` — não cresce o array, apenas anexa uma
  prop estranha. Não quebra mas é bizarro.
- **Paridade v2:** `out.last_mut()` retorna `None` → no-op. Match.
- **Recomendação pós-cutover:** `if (out.length > 0)` explícito.

### [F11-04] 🟡 Name-prefix filter exclui identifiers legítimos
- **Onde:** `countReferences`:
  ```js
  if (/^(new|get|set|is|to|of|on|in|as|at|do|up|or|if|_)/.test(s.name)) continue;
  ```
- **Comportamento:** exclui qualquer símbolo cujo nome começa com esses
  prefixos: `getConfig`, `setUser`, `isValid`, `toJson`, `newBuilder`
  (todos comuns em APIs) são silenciosamente ignorados no ref counting.
  Intenção era pular palavras curtas tipo `to`, `is`, mas o regex não
  tem `$` nem separador — qualquer nome COMEÇANDO com `is_...` é pulado.
- **Impacto:** subcontagem sistemática de refs para símbolos com esses
  prefixos, que são uma fração enorme dos getters/setters.
- **Paridade v2:** Replicado.
- **Recomendação pós-cutover:** mudar para lista exata
  (`['new', 'get', 'set', 'is', ...]`) com `== name`, ou usar `\b...\b`
  apenas para palavras curtas.

### [F11-05] 🟡 Chunks de 200 nomes — regex pode ficar enorme
- **Onde:** `countReferences`:
  ```js
  const chunkSize = 200;
  patterns.push(new RegExp(`\\b(${chunk.join('|')})\\b`, 'g'));
  ```
- **Comportamento:** para um repo com muitos símbolos exportados, cada
  arquivo é escaneado com `ceil(N/200)` regexes, cada uma com até 200
  alternations. Para N=2000, 10 passes por arquivo × tracked files. Pode
  ser O(N²) em repos grandes.
- **Paridade v2:** Replicado (mesmo chunkSize, mesmas regexes).
- **Recomendação pós-cutover:** substituir por Aho-Corasick (crate
  `aho_corasick` no Rust) — mesma saída, O(N) uma passada.

### [F11-06] 🟡 Extractors regex-only não tratam multi-line signatures
- **Onde:** Todas as extract_* functions usam line-by-line.
- **Comportamento:** `def foo(\n    x,\n    y\n):` não é capturado
  porque o close-paren está em linha diferente da open-paren. Mesmo em
  TS/Rust.
- **Paridade v2:** Replicado (bug copiado fielmente).
- **Recomendação pós-cutover:** pode-se fazer um pré-processamento que
  junta linhas entre parens (heurística), ou migrar pra tree-sitter.

### [F11-07] 🟡 `countReferences` recontabiliza self-refs em arquivos cross-idioma
- **Onde:** `countReferences`:
  ```js
  const ownSymbols = new Set((symbolsByFile[relPath] || []).map(s => s.name));
  ...
  if (ownSymbols.has(name)) continue;
  ```
- **Comportamento:** ownSymbols é baseado em `symbolsByFile[relPath]`,
  que só tem entradas para arquivos cujo lang é suportado. Se um arquivo
  Go referencia `alpha` que é uma função em `alpha.py`, e o Go é tracked
  (mas extractorless), ownSymbols está vazio para ele — OK. Mas se fosse
  `alpha` tanto em `.py` quanto em `.md`, md não é extraído, não está em
  ownSymbols, e qualquer uso de "alpha" em `.md` é contado como ref.
- **Impacto:** refs inflados por arquivos não-language (READMEs, docs).
- **Paridade v2:** Replicado.
- **Recomendação pós-cutover:** filtrar `tracked` para arquivos com lang
  no `countReferences`, OU armazenar ownSymbols até para arquivos sem
  extractor (via heurística de nome).

### [F11-08] 🟢 `extractSql` sensível apenas a SQL lowercase na sig
- **Onde:** `extractSql`:
  ```js
  out.push({ type: 'table', name: m[1], sig: `TABLE ${m[1]}`, ... })
  ```
- **Comportamento:** O nome m[1] preserva o case original (regex case-
  insensitive, mas `([A-Za-z0-9_]+)` captura literal). Tudo bem para
  nomes identifier-safe. Mas a sig `TABLE ${m[1]}` hardcoded uppercase
  TABLE — inconsistente com sig de outras linguagens que preservam o
  case do código-fonte (`class Foo`, `function foo`).
- **Paridade v2:** Replicado.
- **Recomendação pós-cutover:** manter hardcoded é OK pra SQL, só
  registrando a convenção.

### [F11-09] 🟡 `rankSymbols` usa `entry.mtime || now` — mtime 0 vira "agora"
- **Onde:** `rankSymbols`:
  ```js
  const ageDays = Math.max(0, (now - (entry.mtime || now)) / DAY);
  ```
- **Comportamento:** se `entry.mtime` é 0/null/undefined, usa `now` →
  `ageDays = 0` → `recency = 1`. Um arquivo sem mtime registrado aparece
  como "editado agora mesmo", inflando seu score.
- **Impacto:** arquivos de seed/fixtures sem mtime sobem no ranking.
- **Paridade v2:** Replicado (mesmo fallback).
- **Recomendação pós-cutover:** usar default de `ageDays = 365`
  (muito antigo) em vez de "agora" quando mtime missing.

### [F11-10] 🟢 Sem suporte a `.pyx`, `.rb`, `.php`, `.kt`, `.swift`, `.c`, `.cpp`
- **Onde:** tabela `LANGS`.
- **Comportamento:** apenas 6 linguagens cobertas. Projetos com outras
  não têm index.
- **Paridade v2:** Replicado (mesma tabela).
- **Recomendação pós-cutover:** expandir tabela conforme demanda — cada
  novo extractor é ~20 linhas de regex.

---

## Fase 13 — `sigilo.cjs` → `domain-sigilo` (2026-04-19)

### [F13-01] 🟢 `sanitize` declara `askedByHuman` mas nunca usa
- **Onde:** `plugin/scripts/sigilo.cjs:98` —
  `function sanitize({ agent, text, askedByHuman = true })`.
- **Comportamento:** `askedByHuman` é destruturado da chamada com
  default `true`, mas o corpo da função jamais referencia a variável.
- **Impacto:** nenhum — zero efeito observável. Pode ter sido planejado
  para diferenciar respostas entre humano↔agente vs agente↔agente (já
  que entre agentes seria legítimo citar infra) mas a lógica ficou
  embutida no `INTERNAL_META_AGENTS` em vez disso.
- **Paridade v2:** `domain_sigilo::sanitize(agent, text)` omite o
  parâmetro — porta só o que o `.cjs` usa de fato. Não quebra caller.
- **Recomendação pós-cutover:** ou (a) remover o parâmetro da assinatura
  para não confundir quem lê o código, ou (b) implementar a semântica
  prevista: se `askedByHuman=false`, afrouxar thresholds (dobra ou
  desliga sigilo em conversa agente↔agente).


---

## Fase 14 — `nudge.cjs` → `domain-nudge` (2026-04-19)

### [F14-01] 🟡 `renderGotchasMd` gera chave de dedup com `${g.file}` sem coerção segura
- **Onde:** `plugin/scripts/nudge.cjs:83` —
  `` const key = `${g.file}|${(g.description || ).slice(0, 60)}` ``.
- **Comportamento:** quando `g.file` é `null` vira string `"null"`;
  quando ausente, vira `"undefined"`. Assim dois gotchas com arquivo
  desconhecido mas descrição idêntica (≤60 chars) colapsam em UMA
  entrada no `GOTCHAS.md` mesmo sendo de origens distintas.
- **Impacto:** baixo em produção (mesma descrição ≤60 chars + arquivo
  nulo = provavelmente é realmente duplicata), mas surpreende se
  alguém injetar entries históricas com shape diferente.
- **Paridade v2:** replicado byte-a-byte via helper `js_template_string`
  que emula o `${…}` de JS.
- **Recomendação pós-cutover:** ou (a) usar `(g.file ?? "_unknown_")`
  explícito, ou (b) incluir `ts` no key pra garantir unicidade
  temporal.

### [F14-02] 🟢 `persistDecision` passa `[]` como `session_files` para `graph.onDecision`
- **Onde:** `plugin/scripts/nudge.cjs:176` —
  `graph.onDecision(memDir, entry, []);`.
- **Comportamento:** o `entry.session_id = "nudge:<agent>"` é sintético
  (não corresponde a uma sessão real em `sessions.jsonl`), então passar
  `session_files` vazio é correto — mas faz com que a decisão nunca
  crie edges `session→decision` no grafo, só o node da decisão e seus
  filhos diretos (`decision→file` via `files_touched`).
- **Impacto:** nenhum — nudge é por definição desacoplado de sessão
  humana.
- **Paridade v2:** `persist_decision` passa `&[]` igual.
- **Recomendação pós-cutover:** nenhuma; documentar no comentário
  inline que `session_id: "nudge:<agent>"` é marcador e não chave
  real.


---

## Fase 15 — `pipelines.cjs` → `domain-pipelines` (2026-04-19)

### [F15-01] 🟡 `advance` não valida `decision` — typo vira "done" silenciosamente
- **Onde:** `plugin/scripts/pipelines.cjs:391-418`.
- **Comportamento:** só o valor literal `"reject"` entra no branch de
  rejeição. Qualquer outro valor (`"accept"`, `"approve"`, `"rejct"`,
  `null`, `undefined`, objeto) cai no caminho feliz e marca o step como
  `"done"`.
- **Impacto:** typo de UI vira aprovação silenciosa. Se algum adapter
  novo decidir mandar `decision: "reject_soft"` esperando semântica
  distinta, o step é marcado `done` e a pipeline avança.
- **Paridade v2:** replica byte-a-byte em `domain_pipelines::advance` —
  só `Some("reject")` aborta; qualquer outro valor (incluindo `None`) é
  tratado como aprovação.
- **Recomendação pós-cutover:** enum estrito (`Decision::Accept |
  Reject`) no input, rejeitar valores desconhecidos com
  `Error::Validation`.

### [F15-02] 🟡 `run_id` gerado pelo `start` sempre termina com hífen
- **Onde:** `plugin/scripts/pipelines.cjs:346` —
  `` `${id}-${now.replace(/[:.]/g, '-').slice(0, 19)}` ``.
- **Comportamento:** `now` é ISO-8601 como `2026-04-19T12:34:56.789Z`
  (24 chars). Após replace `[:.]` → `-` fica `2026-04-19T12-34-56-789Z`.
  O `slice(0, 19)` corta exatamente no 20º caractere, que é o hífen
  introduzido entre `56` e `789` — então todo `run_id` termina com
  `-` seguido do id do pipeline, ex: `p1-2026-04-19T12-34-56-`.
- **Impacto:** estético e potencialmente surpreendente em logs/UIs
  que truncam visualmente. Funcional é estável (determinístico por
  construção).
- **Paridade v2:** replica via `run_id_suffix` com UTF-16 slice → 19
  chars, gera exatamente o mesmo hífen trailing.
- **Recomendação pós-cutover:** cortar em 18 chars (`T12-34-56`) ou
  trocar `.replace(/[:.]/g, '-')` por formato explícito
  `YYYYMMDDTHHMMSS`. Quebra de formato — exige migração do índice de
  runs.

### [F15-03] 🟢 `serializePipeline` omite `type: agent` explícito — round-trip lossy
- **Onde:** `plugin/scripts/pipelines.cjs:206` —
  `if (s.type && s.type !== 'agent') lines.push(...)`.
- **Comportamento:** se o usuário escreve explicitamente `type: agent`
  em `pipeline.yaml` e a def passa por `parse → serialize`, o campo
  desaparece. Default é `agent`, então a reparse rende o mesmo objeto
  lógico — mas o texto YAML não é byte-idêntico.
- **Impacto:** nenhum em runtime. Afeta diff-de-YAML em PRs (quem
  editou um pipeline via UI e committou o YAML pode ver o `type: agent`
  sumir sem motivo aparente).
- **Paridade v2:** replica via `if s.get("type").and_then(Value::as_str)
  .filter(|&t| t != "agent")` — mesmo comportamento lossy.
- **Recomendação pós-cutover:** ou (a) serializar `type: agent`
  explícito quando presente no input, ou (b) normalizar no parse
  (adicionar `type: "agent"` por default se ausente) — escolha (b)
  preserva paridade de round-trip com menos ruído.


---

## Fase 16 — `skills-install.cjs` → `domain-skills-install` (2026-04-19)

### [F16-01] 🟡 `.mcp.json` mal-formado é silenciosamente descartado
- **Onde:** `plugin/scripts/skills-install.cjs:80-84` —
  `try { mcpRoot = JSON.parse(...) } catch { /* keep default */ }`.
- **Comportamento:** se o `projectRoot/.mcp.json` existe mas tem JSON
  inválido (trailing comma, truncado), o parse falha e `mcpRoot` volta
  pra `{ mcpServers: {} }`. Em seguida, se algum skill contribui entry
  (`mcp_added` não-vazio), o `fs.writeFileSync` **sobrescreve** o
  arquivo com o default + entries novos. O conteúdo anterior é
  perdido, sem backup nem warn.
- **Impacto:** baixo em produção (MCP raramente é editado à mão), mas
  inesperado — o usuário que perdeu customizações manuais tem zero
  diagnóstico.
- **Paridade v2:** `apply_skills` replica a semântica: `serde_json::
  from_str` retorna `None`, cai no default vazio, sobrescreve se
  houver mcp_added. Bit-idêntico.
- **Recomendação pós-cutover:** ou (a) abortar com erro explícito se
  `.mcp.json` existe mas não é JSON válido, ou (b) salvar backup
  `.mcp.json.bak` antes de sobrescrever.

### [F16-02] 🟡 `parseFrontmatter` ignora keys com hífen
- **Onde:** `plugin/scripts/skills-install.cjs:51` —
  `line.match(/^(\w+):\s*(.*)$/)`.
- **Comportamento:** `\w+` em JS (e em Rust `regex`) = `[A-Za-z0-9_]+`.
  Keys como `skill-name:` ou `skill.version:` (comuns em YAML
  frontmatter de outras libs) simplesmente não matcham e a linha é
  silenciosamente pulada.
- **Impacto:** se alguém trouxer um SKILL.md de outro ecossistema
  (mdbook, obsidian, hugo) e colar aqui, campos hifenados somem.
- **Paridade v2:** replicado via mesmo regex `^(\w+):\s*(.*)$`.
- **Recomendação pós-cutover:** usar `[\w.-]+` para aceitar keys com
  hífen/ponto (YAML válido); exige revisar consumidores do
  frontmatter para garantir nenhum chave colide com hífen.

### [F16-03] 🟢 `readdirSync` não ordena — ordem de skills depende do FS
- **Onde:** `plugin/scripts/skills-install.cjs:24-28`.
- **Comportamento:** `listSkillsInPack` itera `readdirSync` sem sort.
  Em Linux ext4 a ordem é de inode (≈ criação); em macOS/Windows/tmpfs
  pode ser diferente. UI que apresentar a lista em ordem "natural"
  depende disso.
- **Impacto:** nenhum funcional, mas teste de UI pode flakear entre
  plataformas.
- **Paridade v2:** `domain_skills_install::list_skills_in_pack`
  **ordena por nome** (`sort_by_key`). Diverge por design para ter
  determinismo — os parity tests passam porque o `.cjs` também acaba
  retornando ordem alfabética no Linux com 2 entries criadas em
  sequência, mas não há garantia formal.
- **Recomendação pós-cutover:** explicitar `sort()` no `.cjs` para
  garantir paridade entre Node e Rust em todas as plataformas.


---

## Fase 17 — `seed.cjs` → `domain-seed` (2026-04-19)

### [F17-01] 🟡 Dedup de `best_practices` ignora ordem pack-level vs agent-level
- **Onde:** `plugin/scripts/seed.cjs:198-201` —
  `` const practiceIds = new Set([...(packMeta.best_practices || [])]); ``
  `` for (const cfg of Object.values(srcAgents)) for (const id of cfg.best_practices || []) practiceIds.add(id); ``
- **Comportamento:** o `.cjs` usa `Set` (preserva ordem de inserção no
  JS), então pack-level entra primeiro, depois cada agent adiciona os
  seus. IDs duplicados entre pack e agents aparecem só uma vez, na
  primeira posição. A porta Rust replica com `HashSet` + Vec para
  manter ordem.
- **Impacto:** nenhum funcional — só afeta ordem de apresentação em
  `practicesReport.copied`, que alguns UIs mostram. Baixo.
- **Paridade v2:** replicado (seen-set + preservação de ordem).
- **Recomendação pós-cutover:** documentar que a ordem é
  pack→agents, não "pack vence em caso de conflito".

### [F17-02] 🟡 `filterAgents` com `only="a,,b"` inclui "a" e "b" (vírgula dupla silenciosa)
- **Onde:** `plugin/scripts/seed.cjs:121` —
  `(onlyStr || '').split(',').map(s => s.trim()).filter(Boolean)`.
- **Comportamento:** o `.filter(Boolean)` remove strings vazias após
  trim, então `"a,,b"` → `["a", "b"]` (o ghost empty é filtrado).
  Similarmente `",a,"` → `["a"]`. Sem warn.
- **Impacto:** usuário digita ",a,b" esperando erro/warn, recebe
  silencioso-OK. Não tem consequência negativa — só silencia typo.
- **Paridade v2:** `split_csv` replica via
  `.filter(|s| !s.is_empty())`.
- **Recomendação pós-cutover:** nenhuma obrigatória; opcional: warn
  se o trim retirou segments vazios.

### [F17-03] 🟢 `apply` silenciosamente cria `agents/` mesmo quando pack só tem manager/oracle refs
- **Onde:** `plugin/scripts/seed.cjs:140-141` —
  `const agentsDir = path.join(memDir, 'agents'); if (!fs.existsSync(agentsDir)) fs.mkdirSync(agentsDir, { recursive: true });`
- **Comportamento:** `agents/` sempre é criado, mesmo que o pack tenha
  `agents: {}` (só manager/oracle sem definições concretas).
- **Impacto:** diretório vazio no memDir. Nenhum bug — só polui.
- **Paridade v2:** replicado — cria unconditionally.
- **Recomendação pós-cutover:** criar `agents/` **só** se houver
  algum agent com `context_file` efetivo (pelo menos 1 md a copiar).

### [F17-04] 🟢 `target.manager = src.manager` na rehydrate escolhe default literal "manager" se src não tem
- **Onde:** `plugin/scripts/seed.cjs:148` —
  `target = { ... manager: src.manager || 'manager', oracle: src.oracle || 'oracle', rule: src.rule, agents: {} }`.
- **Comportamento:** se o pack fonte não especifica `manager`, o
  target ganha literal `"manager"` (string do nome default). Depois em
  `seed.cjs:179`, `if (!target.manager && src.manager) target.manager = src.manager` — se src tem `manager`, ele substitui. Mas se src
  não tem, `target.manager` fica travado em `"manager"` literal, não
  em nenhum agent real do manifest.
- **Impacto:** baixo — packs reais sempre declaram manager/oracle.
  Se alguém criar um pack mínimo só com `agents: {...}`, vai ver o
  literal "manager" em agents.json e se perguntar de onde veio.
- **Paridade v2:** replicado byte-a-byte.
- **Recomendação pós-cutover:** ou (a) exigir `manager`/`oracle` no
  validate do pack, ou (b) deixar `null` se ausente e fazer o UI
  inferir pelo agent com `role: manager`.

---

## Fase 18 — `session-init.cjs` + `session-end.cjs` → `domain-session` (2026-04-19)

### [F18-01] 🟡 `detectGotchas` trata `c.exit` ausente como falha (não sucesso)
- **Onde:** `plugin/scripts/session-end.cjs:143` —
  `if ((c.exit ?? 1) !== 0) continue;`
- **Comportamento:** para a segunda execução `c`, se `exit` estiver
  ausente/`undefined`, o operador `??` devolve `1`, o que não-é-zero,
  entrando em `continue`. Ou seja: ausência de exit no sucesso é
  tratada como falha — nenhuma gotcha é gravada.
- **Impacto:** se o logger de bash não registrar `exit` na observação
  de sucesso (bug upstream), gotchas legítimas deixam de ser
  detectadas. Note o inverso para `a`: `a.exit ?? 0` — missing é
  tratado como sucesso, então a observação inicial sem exit NUNCA
  dispara a heurística.
- **Paridade v2:** port inicial divergiu (tratava missing em `c`
  como 0); corrigido para match JS — teste de parity acusou. Agora
  `bash_exit_zero` devolve `false` pra missing.
- **Recomendação pós-cutover:** garantir que observações de bash
  sempre gravem `exit` (até sucesso explícito `0`), e/ou alinhar a
  heurística — ambos os lados iguais (`a.exit ?? 0` vs `c.exit ?? 1`
  é confuso).

### [F18-02] 🟡 `formatObs` imprime `"undefined"` literal quando `o.agent` / `o.file` ausente
- **Onde:** `plugin/scripts/session-init.cjs:331` (file) e `:340`
  (agent) — template literals `${o.file}` / `${o.agent}` sem
  fallback.
- **Comportamento:** quando a observação é mal-formada (faltando
  `file` ou `agent`), a linha renderizada contém literalmente a
  string "undefined" no bloco injetado pro Claude.
- **Impacto:** o Claude vê `- \`ts\` EDIT \`undefined\`` e pode
  ficar confuso ou tentar abrir um arquivo inexistente. Baixo —
  observações bem-formadas nunca caem aqui.
- **Paridade v2:** replicado via helper `js_display` que devolve
  "undefined" para `None`. Teste parity confirma.
- **Recomendação pós-cutover:** filtrar observações inválidas no
  ponto de leitura (antes do formatObs), ou substituir por
  `<desconhecido>` na render.

### [F18-03] 🟡 `classifySession` precedência: `fix` no cmd vira bugfix antes de avaliar refactor
- **Onde:** `plugin/scripts/session-end.cjs:115-118` — ordem dos
  `if`: bugfix é testado ANTES de refactor.
- **Comportamento:** um commit `git mv old new # fix refactor` seria
  classificado como `bugfix` porque `/\bfix\b/` no cmd matcha antes
  da checagem de `refactor`. A intenção humana (refactor) é
  perdida.
- **Impacto:** classificação errada em sessões com commits mistos.
  Baixo — classificação é só para sumarização, não afeta lógica.
- **Paridade v2:** replicado — mesma ordem de checagens.
- **Recomendação pós-cutover:** reverter ordem
  (refactor → bugfix → feature) ou adotar uma taxonomia não-mutuamente
  exclusiva (multi-label).

### [F18-04] 🟢 `estimateTokens` usa `str.length` (UTF-16); port usa UTF-16 explícito
- **Onde:** `plugin/scripts/lib.cjs:97-100` — `Math.ceil(str.length / 3.5)`.
- **Comportamento:** JS `str.length` conta UTF-16 code units. Emojis
  não-BMP contam como 2 (surrogate pair). Port Rust usa
  `.encode_utf16().count()` para paridade exata.
- **Impacto:** nenhum em strings ASCII/BMP; divergência de 1-2 tokens
  para strings com emojis não-BMP (só). Orçamento é aproximado mesmo.
- **Paridade v2:** `estimate_tokens` replica via `encode_utf16`.
- **Recomendação pós-cutover:** nenhuma — estimador é heurístico
  intencionalmente frouxo.

### [F18-05] 🟢 `summarize_session` `.split('/').pop()` crashava com `file` ausente; port silencia
- **Onde:** `plugin/scripts/session-end.cjs:46` —
  `sessionObs.filter(...).map(o => o.file.split('/').pop())`.
- **Comportamento:** se uma observação `file_edit` não tiver `file`,
  `o.file.split` lança `TypeError` e o `.cjs` inteiro crasha (trava
  a regen de MDs, trava o fim-de-sessão). Empiricamente nunca
  acontece porque o append inicial sempre grava `file`.
- **Impacto:** latente; só explode se alguém aparecer com obs
  sintética.
- **Paridade v2:** port diverge por robustez —
  `summarize_session` usa `.filter_map(o.get("file").as_str)` e
  pula obs sem file. Diferença desejada.
- **Recomendação pós-cutover:** aceitar divergência — port é mais
  resiliente; `.cjs` pode adotar `o.file?.split(...)` no cutover.

---

## Fase 19 — `vault.cjs` → `domain-vault` (2026-04-19)

Escopo do port: **cripto pura + layout binário + validação**. I/O
(escrita atômica, cache de device-key, sessão de UI, gitignore disk-side)
fica fora — é responsabilidade do adapter futuro.

Parity tests garantem byte-a-byte:
- `build_header` ↔ `buildHeader`
- AES-256-GCM encrypt/decrypt com header como AAD (bidirecional)
- scrypt com N=2^15,r=8,p=1 (match exato com `crypto.scryptSync`)
- PBKDF2-SHA512 (match com `crypto.pbkdf2Sync`)
- `verify_password` aceitando auth block gerado em Node (scrypt + legacy PBKDF2)
- Export bundle round-trip Rust↔Node
- Tamper detection (GCM tag mismatch rejeita em ambos)

### [F19-01] 🔴 `importBundle` grava device-key ANTES do vault.enc
- **Onde:** `plugin/scripts/vault.cjs:607-609`:
  ```js
  writeFileSafe(deviceKeyPath(memDir), newDeviceKey, 0o600);
  _cachedDeviceKey = newDeviceKey;
  writeFileSafe(vaultPath(memDir), newVault, 0o600);
  ```
- **Comportamento:** se a segunda escrita (`vault.enc`) falhar —
  disco cheio, permission denied, crash — o usuário fica com:
  - `vault.enc` **antigo**, encriptado com a device-key **antiga**;
  - `.vault-devicekey` **novo**, vindo do bundle;
  - a device-key antiga está perdida para sempre.
  Resultado: cofre fica irrecuperável. O bundle de origem ainda existe,
  mas só serve se o user voltar a importar — perde tudo que foi criado
  desde o último backup.
- **Impacto:** perda de dados permanente em failure parcial. Mesmo
  problema pode surgir em `migrateLegacy` (mas ali o PMV1 original ainda
  é recuperável via senha — menos grave).
- **Recomendação pós-cutover:** ordem correta = escrever novo `vault.enc`
  primeiro (em nome temporário tipo `vault.enc.new`), depois device-key,
  depois rename atômico do vault. Ou: backup do trio
  (vault+devicekey+auth) antes de qualquer escrita, restaurar no catch.
- **Paridade v2:** N/A no domínio — I/O fica fora; adapter deve consertar.

### [F19-02] 🟡 `upgradeAuthIfNeeded` engole erro em `unlock`
- **Onde:** `plugin/scripts/vault.cjs:350`:
  ```js
  try { upgradeAuthIfNeeded(memDir, store, password); } catch {}
  ```
- **Comportamento:** se o upgrade do auth block de PBKDF2→scrypt falha
  (permissão, disco cheio, crypto error), a exceção é silenciosamente
  descartada. Usuário continua com cofre legacy PBKDF2 sem saber.
- **Impacto:** latente. O unlock em si funciona (valor de retorno é
  session expires_at), só deixa de endurecer a segurança. Após vazamento
  do password_hash PBKDF2 (que é mais fraco que scrypt), o atacante
  tem vantagem marginal.
- **Recomendação pós-cutover:** logar o erro de upgrade em stderr
  estruturado ou emitir observation `auth_upgrade_failed` no graph.
- **Paridade v2:** N/A no domínio — a função pura `build_auth_block*`
  não engole erros; o comportamento silent-swallow é decisão do caller.

### [F19-03] 🟡 `normalizeStore` aceita array em `secrets`/`envfiles`
- **Onde:** `plugin/scripts/vault.cjs:234-243`:
  ```js
  secrets: (raw.secrets && typeof raw.secrets === 'object') ? raw.secrets : {}
  ```
- **Comportamento:** `typeof [] === 'object'` em JS. Se um cofre
  corrompido contém `secrets: [1,2,3]`, .cjs aceita o array como
  "secrets" e chamadas subsequentes (`Object.keys(secrets).sort()`,
  `secrets[name] = value`) operam no array — sort ordena por stringify
  numérica, set-by-name cria propriedade no array. Comportamento bizarro
  mas silencioso.
- **Impacto:** latente. Só dispara se alguém editar manualmente o
  plaintext do cofre antes de criptografar (nunca acontece em produção).
- **Recomendação pós-cutover:** aceitar divergência — port em Rust usa
  `Value::Object` (rejeita array → retorna `{}` vazio). Mais seguro.
- **Paridade v2:** divergência desejada. Rust é mais estrito.

### [F19-04] 🟢 Cripto exposta como funções puras determinísticas
- **Onde:** `plugin/scripts/vault.cjs` — todas as funções de cripto
  são internas ao arquivo ou inlinadas em callers.
- **Comportamento no port:** `encrypt_with_key_deterministic`,
  `build_auth_block_deterministic`, `build_export_bundle_deterministic`,
  `hash_password_scrypt`, `hash_password_pbkdf2_sha512`,
  `derive_export_key`, `legacy_derive_key`, `parse_file`, `build_header`
  são todas públicas e aceitam salt/nonce como parâmetro. Isso
  permite parity tests byte-a-byte sem mock de RNG, e facilita
  property-based testing futuro.
- **Paridade v2:** padrão Rust-specific. `.cjs` mantém estilo imperativo;
  sem necessidade de replicar a API no Node.

### [F19-05] 🟢 Gitignore helper é funcional puro no Rust
- **Onde:** `plugin/scripts/vault.cjs:120-133` — `ensureGitignore` faz
  I/O direto (ler, testar missing, escrever).
- **Comportamento no port:** `ensure_gitignore_text(current: &str) -> Option<String>`
  retorna o novo conteúdo se precisar adicionar entries; `None` se já
  estiver completo. Adapter chama com conteúdo do disco, grava
  resultado se `Some`. Testável sem filesystem.
- **Paridade v2:** divergência de estilo. Adapter futuro deve
  replicar a lógica atômica (tmp + rename) da implementação de I/O do .cjs.

## Fase 20 — `adapter-fs`

### [F20-01] 🟡 `readLastJsonlLine` não retrocede quando a última linha é malformada
- **Onde:** `plugin/scripts/lib.cjs:62-90`.
- **Comportamento:** em cada iteração do scan backward, só a ÚLTIMA
  linha não-vazia do buffer é testada com `JSON.parse`. Se o parse
  falha, o loop adiciona mais um chunk de 2 KB — mas no próximo passe
  a "última linha" continua sendo a mesma (a malformada), porque o
  buffer cresce pra trás sem remover o tail já rejeitado. Quando o
  scan chega em `pos = 0`, sai do loop e retorna `null`. A penúltima
  linha (válida) nunca é inspecionada.
  ```js
  if (lines.length >= 1) {
    const last = lines[lines.length - 1];
    try { return JSON.parse(last); } catch { /* keep scanning */ }
  }
  ```
- **Impacto:** latente. `observations.jsonl`, `sessions.jsonl` e
  similares só ficam malformados na ponta se o processo for killado no
  meio de um `appendJsonl` (write parcial). Caller raramente depende
  do último item — `session-init.cjs:54` usa `readLastJsonlLine` no
  próprio `sessions.jsonl` e aceita `null` como "sem sessão anterior".
  Nenhum dano observável hoje.
- **Recomendação pós-cutover:** trocar o scan pra testar todas as
  linhas do buffer de trás pra frente (`for (const l of lines.reverse())`),
  não só a última. Fix trivial, mas quebra paridade — espera cutover.
- **Paridade v2:** replicado em `adapter-fs::ObservationLogFs::read_last`.
  Teste `parity_read_last_malformed_tail_both_null` cobre que os dois
  lados retornam `None`/`null` no mesmo input.

### [F20-02] 🟢 Adapter não faz auto-migração de `graph.json` versão antiga
- **Onde:** `plugin/scripts/graph.cjs:53-69` — `loadGraph` dispara
  `rebuildGraph(memDir, readJsonl)` se `g.version !== VERSION`.
- **Comportamento no port:** `GraphStoreFs::load` retorna só os bytes
  crus (`Some(Value)` / `None` / `Err(Serde)`). Nenhuma lógica de
  versão. O caller (`domain-graph` ou app layer) decide se rebuilda.
- **Paridade v2:** divergência de camada, não de comportamento. O
  `.cjs` mistura load + migração no mesmo helper; no Rust, port só
  lê bytes, domínio decide. Paridade observável mantida quando o
  caller do domínio chamar `rebuild_if_version_mismatch` antes de usar.

### [F20-03] 🟢 `saveGraph` muta `updated_at` no .cjs, adapter Rust não
- **Onde:** `plugin/scripts/graph.cjs:71-74`:
  ```js
  function saveGraph(memDir, graph) {
    graph.updated_at = new Date().toISOString();
    fs.writeFileSync(graphPath(memDir), JSON.stringify(graph, null, 2));
  }
  ```
- **Comportamento no port:** `GraphStoreFs::save` escreve exatamente
  o `Value` que recebe, sem mutar. Atualizar `updated_at` é
  responsabilidade do caller (domínio ou app use-case), porque
  `Clock::now()` é um port e precisa ser injetado pra permitir testes
  determinísticos.
- **Paridade v2:** divergência de API. Chamadas de `saveGraph` no
  app layer precisam fazer `graph.updated_at = clock.now().to_rfc3339()`
  antes de chamar o adapter.

## Fase 21 — `startup.cjs` → `api-hooks::startup` (2026-04-19)

### [F21-01] 🟢 Dica "👥 sem time" aparece quando `agents.json` está ausente
- **Onde:** `plugin/scripts/startup.cjs:89-101`.
- **Comportamento:** o `.cjs` começa com `agentCount = 0`, só lê o
  arquivo se `existsSync` for `true`, e o `if (agentCount <= 2)`
  **roda mesmo quando o arquivo não existe**, imprimindo a dica
  "👥 sem time — rode: node …/seed.cjs setup".
  ```js
  try {
    let agentCount = 0;
    if (fs.existsSync(agentsPath)) {
      const m = JSON.parse(fs.readFileSync(agentsPath, 'utf8'));
      agentCount = Object.keys(m.agents || {}).length;
    }
    if (agentCount <= 2) { line += ' | 👥 sem time ...'; }
  } catch {}
  ```
- **Impacto no port:** primeira versão do Rust tratava "arquivo
  ausente" como "sem dica" e "arquivo malformado" como "sem dica",
  resultado: o banner de projeto novo (sem `agents.json`) divergia
  do `.cjs`. Teste `parity_com_memdir_sem_stats` capturou.
- **Correção:** `load_agent_count` diferencia os dois casos — ausência
  devolve `Some(0)` (dica ligada), parse-error devolve `None` (dica
  desligada, replicando o catch do try antes do `line +=`).
- **Paridade v2:** confirmada via `parity_startup` (8 cenários).

### [F21-02] 🟢 Autostart da UI não é portado no binário Rust do `startup`
- **Onde:** `plugin/scripts/startup.cjs:19-62` — `ensureUiRunning`.
- **Comportamento esperado:** spawna detached `node ui/server.cjs`
  com pidfile lock em `/tmp/open-poupar-ui-<port>.pid`, idempotente
  via sinal 0 (check do PID vivo).
- **Decisão:** `poupar-hook-startup` **não** spawna nada. Durante o
  período híbrido (antes de `api-http` existir — F4-05), o `.cjs`
  continua responsável pelo autostart. O Rust só emite o banner.
  Motivo: a alternativa seria spawnar `node` do Rust, o que mantém
  o `.cjs` no hot path e não adiciona valor.
- **Paridade v2:** *divergência consciente* — documentada aqui e no
  cabeçalho de `crates/api-hooks/src/bin/startup.rs`. Teste de
  paridade desliga o autostart via `POUPAR_UI_AUTOSTART=0` para
  ambos os lados, portanto o stdout ainda casa byte-a-byte.

## Fase 4 — Port de `ui/server.cjs`

### [F4-05b-01] 🟢 `/api/vault/init` retorna shape diferente entre stacks
- **Onde:** `plugin/scripts/vault.cjs:318` vs
  `crates/adapter-vault-file/src/lib.rs:335-363`.
- **Comportamento `.cjs`:** `init()` devolve apenas
  `{ initialized: true, expires_at }` (2 campos).
- **Comportamento Rust:** adapter devolve `VaultStatus` completo
  (5 campos: `initialized`, `locked`, `expires_at`, `ttl_ms`,
  `needs_migration`).
- **Decisão:** o handler HTTP em `vault_routes.rs::init()` *filtra*
  o shape de retorno do adapter para só expor os 2 campos que o
  `.cjs` devolve, preservando paridade byte-a-byte. Adapter fica
  como está — é estritamente mais informativo. Após cutover, o Rust
  pode devolver o shape completo (UI nova aproveita `locked`/`ttl_ms`
  sem precisar chamar `/status` em seguida).
- **Paridade v2:** ✅ replicada no HTTP — `init` devolve `{ initialized,
  expires_at }` exatamente como `.cjs`.

### [F4-05b-02] 🟡 `/api/vault/import` retorna metadados no `.cjs` que o Rust descarta
- **Onde:** `plugin/scripts/vault.cjs:618-621` vs
  `crates/adapter-vault-file/src/lib.rs:623-659`.
- **Comportamento `.cjs`:** `importBundle()` devolve
  `{ imported_at, exported_at }` e o handler HTTP faz
  `sendJson(res, 200, { ok: true, ...r })`, resultando em
  `{ ok: true, imported_at, exported_at }`.
- **Comportamento Rust:** `import_bundle` → `Result<(), VaultError>`,
  handler devolve só `{ ok: true }`. A informação sobre quando o
  bundle foi exportado é perdida.
- **Impacto:** UI perde feedback "importado de bundle de DD/MM/YYYY".
- **Decisão:** registrar como divergência agora. Fixar exige mudar
  assinatura de `VaultStore::import_bundle` no `core-ports`. Adiar
  até pós-cutover ou quando UI precisar do metadado.
- **Paridade v2:** ⚠️ divergente consciente — teste de paridade
  `parity_vault.rs` **não** exercita o happy-path de import.

### [F4-05b-03] 🟢 Rust aceita `Sec-Fetch-Site` vazio onde `.cjs` também aceita
- **Onde:** `plugin/ui/server.cjs:3164-3170` vs
  `crates/api-http/src/vault_routes.rs::guard()`.
- **Comportamento:** ambos tratam header vazio como "ok" (no-op no
  `.cjs`; `lc.is_empty()` no Rust). Só bloqueiam `cross-site` /
  `cross-origin` etc.
- **Paridade v2:** ✅ replicada.


### [F4-05c-01] 🟡 `LocalPracticeError::InvalidId` único em Rust vs 2 msgs distintas no `.cjs`
- **Onde:** `plugin/scripts/best-practices.cjs:167` (create) vs `:196` (update)
  vs `:228` (delete). Rust: `LocalPracticeError::InvalidId` com msg
  longa fixa.
- **Comportamento `.cjs`:** `create` → "id inválido (slug [a-z][a-z0-9-]+[a-z0-9], 3–50 chars)";
  `update`/`delete` → "id inválido" (curto).
- **Comportamento Rust:** sempre a msg longa.
- **Decisão:** remapear no handler HTTP (`best_practices_routes.rs::map_local_err`)
  — `short_invalid_id=true` em update/delete converte pra "id inválido".
  Create preserva msg longa. Domain fica como está — o tipo Rust é
  semanticamente correto e não perde info. Apenas o layer HTTP
  ajusta o shape pra paridade.
- **Paridade v2:** ✅ replicada via remap no handler.
