# Code Index
> Gerado em: 2026-09-14T02:02:55.333Z
> 215 arquivos · 1368 símbolos · linguagens: rust (1092), typescript (276)

## Top símbolos (por importância)

- **`mem_dir`** (fn) — `v2/crates/core-paths/src/lib.rs:237` · 1089 refs
  `fn mem_dir(cwd: &Path)`
- **`export`** (fn) — `v2/crates/domain-training-export/src/lib.rs:90` · 417 refs
  `fn export(mem_dir: &Path, opts: &ExportOptions)`
- **`export`** (async-fn) — `v2/crates/api-http/src/vault_routes.rs:435` · 417 refs
  `fn export(State(st)`
- **`Button`** (const) — `ui-next/src/components/ui/button.tsx:35` · 261 refs
  `const Button`
- **`Error`** (enum) — `v2/crates/domain-vault/src/lib.rs:66` · 206 refs
  `enum Error`
- **`Error`** (enum) — `v2/crates/domain-seed/src/lib.rs:26` · 206 refs
  `enum Error`
- **`Error`** (enum) — `v2/crates/api-cli/src/lib.rs:33` · 206 refs
  `enum Error`
- **`AppState`** (struct) — `v2/crates/api-http/src/lib.rs:172` · 180 refs
  `struct AppState`
- **`Error`** (enum) — `v2/crates/domain-skills-install/src/lib.rs:15` · 206 refs
  `enum Error`
- **`Error`** (enum) — `v2/crates/domain-session/src/lib.rs:19` · 206 refs
  `enum Error`
- **`Error`** (enum) — `v2/crates/domain-pipelines/src/lib.rs:28` · 206 refs
  `enum Error`
- **`Error`** (enum) — `v2/crates/domain-humans/src/lib.rs:43` · 206 refs
  `enum Error`
- **`parse`** (fn) — `v2/crates/domain-cron/src/parser.rs:48` · 196 refs
  `fn parse(expr: &str)`
- **`Text`** (fn) — `ui-next/src/ui/adapter-mantine/typography.tsx:19` · 196 refs
  `function Text({ size = 'sm', fw, muted, color, style, component, childr...)`
- **`Group`** (fn) — `ui-next/src/ui/adapter-mantine/Group.tsx:14` · 133 refs
  `function Group({ gap = 'sm', justify, align, wrap, children }: GroupProps)`
- **`Code`** (fn) — `ui-next/src/ui/adapter-mantine/typography.tsx:51` · 117 refs
  `function Code({ children, style }: CodeProps)`
- **`Box`** (fn) — `ui-next/src/ui/adapter-mantine/Box.tsx:26` · 109 refs
  `function Box(props: BoxProps)`
- **`list`** (async-fn) — `v2/crates/api-http/src/best_practices_routes.rs:43` · 108 refs
  `fn list(State(st)`
- **`list`** (fn) — `v2/crates/domain-pipelines/src/lib.rs:303` · 108 refs
  `fn list(templates_dir: &Path, mem_dir: &Path)`
- **`spawn`** (fn) — `v2/crates/api-http/src/cron_scheduler.rs:24` · 104 refs
  `fn spawn(state: AppState)`
- **`Badge`** (fn) — `ui-next/src/ui/adapter-mantine/typography.tsx:64` · 84 refs
  `function Badge({ color = 'gray', variant = 'light', size = 'sm', childre...)`
- **`Badge`** (fn) — `ui-next/src/components/ui/badge.tsx:26` · 84 refs
  `function Badge({ className, variant, ...props }: BadgeProps)`
- **`VaultError`** (enum) — `v2/crates/core-ports/src/vault.rs:27` · 76 refs
  `enum VaultError`
- **`Clock`** (trait) — `v2/crates/domain-architect/src/lib.rs:192` · 56 refs
  `trait Clock`
- **`Stack`** (fn) — `ui-next/src/ui/adapter-mantine/Stack.tsx:12` · 63 refs
  `function Stack({ gap = 'md', align, children }: StackProps)`
- **`clear`** (fn) — `v2/crates/domain-pipelines/src/lib.rs:1010` · 60 refs
  `fn clear(mem_dir: &Path)`
- **`snapshot`** (fn) — `v2/crates/domain-integrity/src/lib.rs:96` · 60 refs
  `fn snapshot(plugin_root: &Path, mem_dir: &Path)`
- **`Clock`** (trait) — `v2/crates/domain-indexer/src/lib.rs:33` · 56 refs
  `trait Clock`
- **`Clock`** (trait) — `v2/crates/domain-humans/src/lib.rs:56` · 56 refs
  `trait Clock`
- **`Clock`** (trait) — `v2/crates/domain-graph/src/lib.rs:33` · 56 refs
  `trait Clock`
- **`Clock`** (trait) — `v2/crates/domain-firefighter/src/lib.rs:45` · 56 refs
  `trait Clock`
- **`Clock`** (trait) — `v2/crates/domain-bus/src/lib.rs:62` · 56 refs
  `trait Clock`
- **`Clock`** (trait) — `v2/crates/core-queue/src/lib.rs:114` · 56 refs
  `trait Clock`
- **`Clock`** (trait) — `v2/crates/core-ports/src/clock.rs:14` · 56 refs
  `trait Clock`
- **`TenantContext`** (struct) — `v2/crates/api-http/src/tenant.rs:18` · 40 refs
  `struct TenantContext`
- **`save_manifest`** (fn) — `v2/crates/domain-agents/src/manifest.rs:59` · 46 refs
  `fn save_manifest(mem_dir: &Path, cfg: &Value)`
- **`SystemClock`** (struct) — `v2/crates/domain-architect/src/lib.rs:202` · 37 refs
  `struct SystemClock`
- **`load_manifest`** (fn) — `v2/crates/domain-agents/src/manifest.rs:44` · 42 refs
  `fn load_manifest(mem_dir: &Path)`
- **`Modal`** (fn) — `ui-next/src/ui/adapter-mantine/Modal.tsx:16` · 41 refs
  `function Modal({ opened, onClose, title, size = 'md', fullScreen, withCl...)`
- **`plugin_dir`** (fn) — `v2/crates/core-paths/src/install.rs:114` · 32 refs
  `fn plugin_dir()`

## Arquivos mais densos

### `v2/crates/domain-humans/src/lib.rs` _(44 símbolos)_
- `enum Error` — L43
- `trait Clock` — L56
- `struct SystemClock` — L62
- `struct FixedClock` — L85
- `fn format_iso_millis(ms: i64)` — L96
- `fn load_humans(mem_dir: &Path)` — L344
- `fn human_summary(h: &Value, now_ms: i64)` — L405
- `fn create_human(mem_dir: &Path, input: &CreateInput, clock: &dyn Clock)` — L530
- _... mais 36_

### `v2/crates/domain-bus/src/lib.rs` _(41 símbolos)_
- `trait Clock` — L62
- `struct SystemClock` — L72
- `struct FixedClock` — L89
- `fn emit(mem_dir: &Path, ev: &Value, clock: &dyn Clock)` — L445
- `fn legacy_file(mem_dir: &Path)` — L137
- `fn format_iso_millis(ms_since_epoch: i64)` — L103
- `fn read_events(mem_dir: &Path, opts: &ReadEventsOpts, clock: &dyn Clock)` — L399
- `struct CompactOpts` — L503
- _... mais 33_

### `v2/crates/domain-graph/src/lib.rs` _(40 símbolos)_
- `trait Clock` — L33
- `struct SystemClock` — L41
- `fn empty()` — L124
- `fn read_jsonl(file: &Path)` — L641
- `struct FixedClock` — L54
- `fn format_iso_millis(ms_since_epoch: i64)` — L62
- `fn save_graph(mem_dir: &Path, graph: &mut Value, clock: &dyn Clock)` — L170
- `fn score_of(entity: &Value, now_ms: i64)` — L291
- _... mais 32_

### `v2/crates/adapter-vault-file/tests/vault_store.rs` _(34 símbolos)_
- `fn store()` — L13
- `fn mem(t: &TempDir)` — L18
- `fn status_reports_uninitialized_on_empty_dir()` — L25
- `fn init_creates_vault_devicekey_and_gitignore()` — L36
- `fn init_returns_unlocked_status()` — L48
- `fn init_rejects_second_init()` — L58
- `fn init_rejects_short_password()` — L66
- `fn init_sets_0600_on_sensitive_files()` — L74
- _... mais 26_

### `v2/crates/domain-pipelines/src/lib.rs` _(33 símbolos)_
- `enum Error` — L28
- `fn list(templates_dir: &Path, mem_dir: &Path)` — L303
- `fn clear(mem_dir: &Path)` — L1010
- `fn cancel(mem_dir: &Path, reason: Option<&str>, clock: &dyn Clock)` — L991
- `struct Block` — L181
- `fn load_state(mem_dir: &Path)` — L755
- `enum InstallResult` — L342
- `fn slice_utf16(s: &str, n: usize)` — L782
- _... mais 25_

### `v2/crates/domain-session/src/lib.rs` _(31 símbolos)_
- `enum Error` — L19
- `fn estimate_tokens(s: &str)` — L154
- `struct Block` — L163
- `fn summarize_session(session_obs: &[Value])` — L464
- `fn apply_token_budget(mut blocks: Vec<Block>, budget: usize)` — L191
- `fn classify_session(obs: &[Value])` — L272
- `fn detect_gotchas(obs: &[Value], session_id: Option<&str>, now: &str)` — L390
- `fn correction_markers()` — L26
- _... mais 23_

### `v2/crates/domain-indexer/src/lib.rs` _(27 símbolos)_
- `trait Clock` — L33
- `struct SystemClock` — L41
- `fn extract(lang: &str, content: &str)` — L555
- `struct FixedClock` — L53
- `fn format_iso_millis(ms_since_epoch: i64)` — L61
- `fn write_index_md(mem_dir: &Path, index: &Value)` — L922
- `enum IndexerError` — L24
- `fn detect_lang(file: &str)` — L87
- _... mais 19_

### `v2/crates/domain-vault/src/lib.rs` _(26 símbolos)_
- `enum Error` — L66
- `fn verify_password(password: &[u8], auth: &AuthBlock)` — L304
- `struct Store` — L336
- `fn build_header(magic: &[u8], salt: &[u8])` — L133
- `fn parse_file(buf: &[u8])` — L142
- `fn decrypt_with_key(file: &[u8], key: &[u8])` — L223
- `fn normalize_store(raw: &Value)` — L344
- `fn serialize_store(store: &Store)` — L371
- _... mais 18_

### `ui-next/src/lib/humans.ts` _(26 símbolos)_
- `interface HumanProfile` — L25
- `type HumanGender` — L4
- `const HUMAN_LOCATIONS` — L60
- `function listHumans()` — L107
- `interface HumanSkin` — L8
- `type HumanLocation` — L3
- `function randomSkin()` — L89
- `function heartbeatHuman(id: string)` — L162
- _... mais 18_

### `v2/crates/api-http/src/lib.rs` _(25 símbolos)_
- `struct AppState` — L172
- `fn build_router(state: AppState)` — L332
- `struct NetworkConfig` — L75
- `struct LanIp` — L142
- `fn list_lan_ips()` — L149
- `fn socket_addr_for(host: &str, port: u16)` — L677
- `fn save_network_config(mem_dir: &Path, cfg: &NetworkConfig)` — L121
- `fn network_cfg_path(mem_dir: &Path)` — L90
- _... mais 17_

### `v2/crates/domain-firefighter/src/lib.rs` _(25 símbolos)_
- `trait Clock` — L45
- `struct SystemClock` — L54
- `struct FixedClock` — L71
- `fn format_iso_millis(ms_since_epoch: i64)` — L83
- `fn load_state(mem_dir: &Path)` — L154
- `fn slice_utf16(s: &str, n: usize)` — L362
- `fn save_state(mem_dir: &Path, state: &Value)` — L164
- `fn prune_old_triages(entries: &[Value], clock: &dyn Clock)` — L173
- _... mais 17_

### `v2/crates/core-queue/src/lib.rs` _(24 símbolos)_
- `trait Clock` — L114
- `struct SystemClock` — L120
- `struct AgentQueue` — L484
- `struct AgentQueueConfig` — L182
- `enum Priority` — L239
- `struct DrainStartEvent` — L71
- `struct ManualClock` — L146
- `struct ThrottleEvent` — L53
- _... mais 16_

### `v2/crates/domain-destructive/src/lib.rs` _(24 símbolos)_
- `fn detect(mem_dir: &Path, text: &str)` — L617
- `fn format_iso_millis(ms_since_epoch: i64)` — L308
- `fn slice_utf16(s: &str, n: usize)` — L440
- `struct RandomIdGen` — L248
- `struct SystemIsoClock` — L283
- `fn compile_user_pattern(pt: &Value)` — L493
- `fn list_patterns(mem_dir: &Path)` — L530
- `struct DetectResult` — L599
- _... mais 16_

### `v2/crates/api-hooks/src/session_init.rs` _(21 símbolos)_
- `fn render_hook_payload(additional_context: Option<&str>)` — L442
- `struct Block` — L385
- `struct RecentCommit` — L116
- `struct GitContext` — L122
- `struct AdrEntry` — L159
- `fn render_session_start_record(session_id: Option<&str>, started_at: &str, cwd: &str)` — L455
- `fn render_md_block(title: &str, content: &str)` — L102
- `fn format_git_block(git: &GitContext)` — L129
- _... mais 13_

### `ui-next/src/lib/vault.ts` _(20 símbolos)_
- `interface VaultStatus` — L1
- `interface EnvFileEntry` — L95
- `function unlockVault(password: string, ttlMin = 30)` — L27
- `function listVaultKeys()` — L55
- `function generateBundlePassword()` — L183
- `function listEnvFiles()` — L101
- `function lockVault()` — L39
- `function changeVaultPassword(oldPassword: string, newPassword: string)` — L43
- _... mais 12_
