//! # api-hooks
//!
//! Biblioteca com a lógica pura dos 4 hook entrypoints do Claude Code
//! — `startup`, `observe`, `session-init`, `session-end`. Cada binário
//! em `src/bin/` é um thin main que parseia stdin e imprime stdout; toda
//! decisão testável mora aqui para paridade cross-language com os
//! `.cjs` originais em `plugin/scripts/`.
//!
//! ## Estado do port (F4-03)
//!
//! | Hook             | Módulo             | Binário              | Origem               |
//! |------------------|--------------------|----------------------|----------------------|
//! | SessionStart     | [`startup`]        | `pmem-hook-startup`  | `startup.cjs`        |
//! | PostToolUse      | *(pendente)*       | `pmem-hook-observe`  | `observe.cjs`        |
//! | UserPromptSubmit | *(pendente)*       | `pmem-hook-session-init` | `session-init.cjs`|
//! | SessionEnd       | *(pendente)*       | `pmem-hook-session-end`  | `session-end.cjs`|

pub mod observe;
pub mod session_end;
pub mod session_init;
pub mod startup;
