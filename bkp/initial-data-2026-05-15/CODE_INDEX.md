# Code Index
> Gerado em: 2026-04-19T13:11:17.837Z
> 146 arquivos · 1011 símbolos · linguagens: rust (837), typescript (174)

## Top símbolos (por importância)

- **`manifest_path`** (fn) — `v2/crates/domain-agents/src/lib.rs:151` · 51 refs
  `fn manifest_path(mem_dir: &Path)`
- **`mem_dir`** (fn) — `v2/crates/core-paths/src/lib.rs:230` · 241 refs
  `fn mem_dir(cwd: &Path)`
- **`agents_dir`** (fn) — `v2/crates/domain-agents/src/lib.rs:158` · 6 refs
  `fn agents_dir(mem_dir: &Path)`
- **`is_valid_agent_type`** (fn) — `v2/crates/domain-agents/src/lib.rs:136`
  `fn is_valid_agent_type(value: &str)`
- **`is_valid_autonomy_level`** (fn) — `v2/crates/domain-agents/src/lib.rs:143`
  `fn is_valid_autonomy_level(value: &str)`
- **`tmp_dir`** (fn) — `v2/crates/core-paths/src/lib.rs:239` · 9 refs
  `fn tmp_dir(mem_dir: &Path)`
- **`find_project_root`** (fn) — `v2/crates/core-paths/src/lib.rs:171` · 6 refs
  `fn find_project_root(start_cwd: &Path)`
- **`ensure_tmp_dir`** (fn) — `v2/crates/core-paths/src/lib.rs:277` · 5 refs
  `fn ensure_tmp_dir(mem_dir: &Path)`
- **`ensure_mem_dir`** (fn) — `v2/crates/core-paths/src/lib.rs:264` · 4 refs
  `fn ensure_mem_dir(mem_dir: &Path)`
- **`Button`** (const) — `ui-next/src/components/ui/button.tsx:35` · 259 refs
  `const Button`
- **`save_manifest`** (fn) — `v2/crates/domain-agents/src/manifest.rs:59` · 76 refs
  `fn save_manifest(mem_dir: &Path, cfg: &Value)`
- **`FixedClock`** (struct) — `v2/crates/domain-bus/src/lib.rs:86` · 36 refs
  `struct FixedClock`
- **`normalize_posix`** (fn) — `v2/crates/core-paths/src/lib.rs:104`
  `fn normalize_posix(input: &str)`
- **`path_join`** (fn) — `v2/crates/core-paths/src/lib.rs:141`
  `fn path_join(a: &Path, b: &str)`
- **`EnsureError`** (enum) — `v2/crates/core-paths/src/lib.rs:249`
  `enum EnsureError`
- **`emit`** (fn) — `v2/crates/domain-bus/src/lib.rs:442` · 31 refs
  `fn emit(mem_dir: &Path, ev: &Value, clock: &dyn Clock)`
- **`append_jsonl`** (fn) — `v2/crates/core-paths/src/jsonl.rs:82` · 16 refs
  `fn append_jsonl(path: &Path, value: &Value)`
- **`FixedClock`** (struct) — `v2/crates/domain-architect/src/lib.rs:246` · 36 refs
  `struct FixedClock`
- **`read_jsonl`** (fn) — `v2/crates/core-paths/src/jsonl.rs:62` · 13 refs
  `fn read_jsonl(path: &Path)`
- **`read_last_jsonl_line`** (fn) — `v2/crates/core-paths/src/jsonl.rs:108` · 13 refs
  `fn read_last_jsonl_line(path: &Path)`
- **`read_stdin_from_str`** (fn) — `v2/crates/core-paths/src/stdin.rs:61` · 31 refs
  `fn read_stdin_from_str(raw: &str)`
- **`detect`** (fn) — `v2/crates/domain-destructive/src/lib.rs:617` · 29 refs
  `fn detect(mem_dir: &Path, text: &str)`
- **`Badge`** (fn) — `ui-next/src/components/ui/badge.tsx:26` · 49 refs
  `function Badge({ className, variant, ...props }: BadgeProps)`
- **`build_injection_block`** (fn) — `v2/crates/domain-agents/src/injection.rs:169` · 6 refs
  `fn build_injection_block(mem_dir: &Path)`
- **`sync_claude_md`** (fn) — `v2/crates/core-paths/src/claude_md.rs:88` · 31 refs
  `fn sync_claude_md(project_root: &Path, project_md_content: &str)`
- **`Clock`** (trait) — `v2/crates/domain-bus/src/lib.rs:59` · 16 refs
  `trait Clock`
- **`add_agent`** (fn) — `v2/crates/domain-agents/src/registry.rs:78` · 16 refs
  `fn add_agent(mem_dir: &Path, name: &str, config: &Value)`
- **`load_manifest`** (fn) — `v2/crates/domain-agents/src/manifest.rs:44` · 29 refs
  `fn load_manifest(mem_dir: &Path)`
- **`load_project_config`** (fn) — `v2/crates/core-paths/src/config.rs:95` · 9 refs
  `fn load_project_config(mem_dir: &Path)`
- **`Clock`** (trait) — `v2/crates/domain-architect/src/lib.rs:192` · 16 refs
  `trait Clock`
- **`js_trim`** (fn) — `v2/crates/domain-agents/src/js_trim.rs:17` · 22 refs
  `fn js_trim(s: &str)`
- **`Clock`** (trait) — `v2/crates/core-ports/src/clock.rs:14` · 16 refs
  `trait Clock`
- **`save_project_config`** (fn) — `v2/crates/core-paths/src/config.rs:114` · 8 refs
  `fn save_project_config(mem_dir: &Path, cfg: &Value)`
- **`remove_agent`** (fn) — `v2/crates/domain-agents/src/registry.rs:251` · 10 refs
  `fn remove_agent(mem_dir: &Path, name: &str)`
- **`AgentType`** (type) — `ui-next/src/lib/types.ts:1` · 23 refs
  `type AgentType`
- **`format_iso_millis`** (fn) — `v2/crates/domain-bus/src/lib.rs:100` · 8 refs
  `fn format_iso_millis(ms_since_epoch: i64)`
- **`CreateLocalInput`** (struct) — `v2/crates/domain-best-practices/src/lib.rs:593` · 11 refs
  `struct CreateLocalInput`
- **`saveUserPatterns`** (async-fn) — `ui-next/src/lib/destructive.ts:40` · 19 refs
  `function saveUserPatterns(patterns: Pattern[])`
- **`load_mini_context`** (fn) — `v2/crates/domain-agents/src/mini_context.rs:72` · 13 refs
  `fn load_mini_context(mem_dir: &Path, context_file: &str)`
- **`AgentCfg`** (interface) — `ui-next/src/lib/types.ts:10` · 18 refs
  `interface AgentCfg`

## Arquivos mais densos

### `v2/crates/domain-bus/src/lib.rs` _(41 símbolos)_
- `struct FixedClock` — L86
- `fn emit(mem_dir: &Path, ev: &Value, clock: &dyn Clock)` — L442
- `trait Clock` — L59
- `fn format_iso_millis(ms_since_epoch: i64)` — L100
- `struct SweepCallbacks` — L939
- `struct SweepOpts` — L946
- `struct SystemClock` — L69
- `struct CompactOpts` — L500
- _... mais 33_

### `v2/crates/domain-destructive/tests/parity_detect.rs` _(40 símbolos)_
- `fn cjs_path()` — L34
- `fn node_available()` — L41
- `fn run_node(mem_dir: &Path, op: &str, args: Value)` — L49
- `fn assert_parity(text: &str)` — L77
- `fn empty_string()` — L92
- `fn plain_text()` — L97
- `fn greeting_pt()` — L102
- `fn rm_rf_simple()` — L109
- _... mais 32_

### `v2/crates/domain-agents/tests/parity_scope.rs` _(37 símbolos)_
- `fn lib_cjs_path()` — L53
- `fn node_available()` — L60
- `fn call_v1(op: &str, mem_dir: &Path, payload: &Value)` — L68
- `fn assert_tools_match(input: Value)` — L95
- `fn sanitize_tools_non_array()` — L110
- `fn sanitize_tools_empty_array()` — L119
- `fn sanitize_tools_all_known()` — L124
- `fn sanitize_tools_filters_unknown()` — L132
- _... mais 29_

### `v2/crates/domain-agents/tests/parity_model.rs` _(33 símbolos)_
- `fn lib_cjs_path()` — L53
- `fn node_available()` — L60
- `fn call_v1(op: &str, arg: &str, payload: &Value)` — L68
- `fn twin_dirs(tmp: &TempDir)` — L93
- `fn norm_match(input: Value)` — L107
- `fn normalize_model_null()` — L121
- `fn normalize_model_empty()` — L126
- `fn normalize_model_whitespace()` — L131
- _... mais 25_

### `v2/crates/domain-agents/tests/parity_registry.rs` _(32 símbolos)_
- `fn lib_cjs_path()` — L45
- `fn node_available()` — L52
- `fn call_v1(op: &str, mem_dir: &Path, payload: &Value)` — L60
- `fn twin_dirs(tmp: &TempDir)` — L85
- `fn assert_agents_json_match(a: &Path, b: &Path)` — L93
- `fn assert_ctx_match(a: &Path, b: &Path, rel: &str)` — L104
- `fn add_match(name: &str, config: Value)` — L117
- `fn add_agent_minimal_specialist()` — L138
- _... mais 24_

### `v2/crates/domain-agents/tests/parity_injection.rs` _(29 símbolos)_
- `fn lib_cjs_path()` — L26
- `fn node_available()` — L33
- `fn call_v1(mem_dir: &Path)` — L41
- `fn twin_dirs(tmp: &TempDir)` — L57
- `fn setup_manifest(a: &Path, b: &Path, cfg: &Value)` — L65
- `fn assert_parity(a: &Path, b: &Path)` — L72
- `fn ensure_context_file(cfg: &mut Value)` — L81
- `fn parity_missing_manifest_returns_empty()` — L95
- _... mais 21_

### `v2/crates/domain-agents/tests/parity_autonomy.rs` _(27 símbolos)_
- `fn lib_cjs_path()` — L46
- `fn node_available()` — L53
- `fn call_v1(op: &str, arg: &str, payload: &Value)` — L61
- `fn twin_dirs(tmp: &TempDir)` — L86
- `fn norm_match(input: Value)` — L96
- `fn normalize_null()` — L110
- `fn normalize_low()` — L115
- `fn normalize_medium_with_call_human()` — L120
- _... mais 19_

### `v2/crates/domain-agents/tests/parity_invocations.rs` _(26 símbolos)_
- `fn lib_cjs_path()` — L22
- `fn node_available()` — L29
- `fn call_v1(mem_dir: &Path)` — L37
- `fn twin_dirs(tmp: &TempDir, obs: Option<&str>)` — L53
- `fn assert_parity(obs: Option<&str>)` — L65
- `fn parity_missing_file_returns_empty_object()` — L78
- `fn parity_empty_file()` — L83
- `fn parity_single_entry()` — L88
- _... mais 18_

### `v2/crates/core-paths/tests/parity_stdin.rs` _(26 símbolos)_
- `fn lib_cjs_path()` — L36
- `fn node_available()` — L43
- `fn call_v1_read_stdin(stdin_bytes: &[u8])` — L52
- `fn call_v1_get_cwd(stdin_bytes: &[u8], env: &[(&str, &str)` — L77
- `fn stringify_value(v: &Value)` — L115
- `fn parity_read_stdin_empty_object()` — L122
- `fn parity_read_stdin_object_with_cwd()` — L132
- `fn parity_read_stdin_empty_input_returns_empty_obj()` — L143
- _... mais 18_

### `v2/crates/core-paths/tests/parity_claude_md.rs` _(26 símbolos)_
- `fn lib_cjs_path()` — L26
- `fn node_available()` — L33
- `fn call_v1(project_root: &Path, content: &str)` — L41
- `fn twin_roots(tmp: &TempDir, initial_content: Option<&str>)` — L59
- `fn claude_md(root: &Path)` — L75
- `fn read_bytes(p: &Path)` — L79
- `fn parity_creates_file_when_missing()` — L86
- `fn parity_creates_directory_when_missing()` — L99
- _... mais 18_

### `v2/crates/domain-agents/tests/parity_templates.rs` _(25 símbolos)_
- `fn lib_cjs_path()` — L32
- `fn node_available()` — L39
- `fn call_v1(op: &str, payload: &Value)` — L47
- `fn assert_template_match(op: &str, payload: Value)` — L73
- `fn integrator_minimal()` — L103
- `fn integrator_with_service()` — L108
- `fn integrator_full_fields()` — L116
- `fn integrator_empty_arrays_use_fallback()` — L142
- _... mais 17_

### `v2/crates/domain-destructive/src/lib.rs` _(24 símbolos)_
- `fn detect(mem_dir: &Path, text: &str)` — L617
- `fn format_iso_millis(ms_since_epoch: i64)` — L308
- `struct FixedIdGen` — L265
- `struct FixedIsoClock` — L298
- `fn compile_user_pattern(pt: &Value)` — L493
- `fn list_patterns(mem_dir: &Path)` — L530
- `struct DetectResult` — L599
- `fn load_user_config(mem_dir: &Path)` — L200
- _... mais 16_

### `ui-next/src/lib/humans.ts` _(24 símbolos)_
- `interface HumanProfile` — L23
- `type HumanGender` — L2
- `function listHumans()` — L102
- `interface HumanSkin` — L6
- `const HUMAN_LOCATIONS` — L55
- `type HumanLocation` — L1
- `function randomSkin()` — L84
- `function heartbeatHuman(id: string)` — L138
- _... mais 16_

### `v2/crates/core-paths/tests/parity_jsonl.rs` _(23 símbolos)_
- `fn lib_cjs_path()` — L42
- `fn node_available()` — L49
- `fn call_v1(fn_name: &str, arg: &Path, payload: &str)` — L57
- `fn read_jsonl_missing_file_matches_v1()` — L79
- `fn read_jsonl_mixed_content_matches_v1()` — L91
- `fn read_jsonl_no_trailing_newline_matches_v1()` — L109
- `fn read_jsonl_crlf_line_endings_matches_v1()` — L122
- `fn append_jsonl_produces_same_bytes_as_v1()` — L140
- _... mais 15_

### `v2/crates/domain-agents/tests/parity_vault.rs` _(22 símbolos)_
- `fn lib_cjs_path()` — L40
- `fn node_available()` — L47
- `fn call_v1(op: &str, arg: &str, payload: &Value)` — L55
- `fn twin_dirs(tmp: &TempDir)` — L80
- `fn sanitize_match(input: Value)` — L90
- `fn sanitize_non_array()` — L100
- `fn sanitize_empty()` — L108
- `fn sanitize_valid_names_sorted()` — L113
- _... mais 14_
