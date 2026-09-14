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
//! | SessionStart     | [`startup`]        | `poupar-hook-startup`  | `startup.cjs`        |
//! | PostToolUse      | *(pendente)*       | `poupar-hook-observe`  | `observe.cjs`        |
//! | UserPromptSubmit | *(pendente)*       | `poupar-hook-session-init` | `session-init.cjs`|
//! | SessionEnd       | *(pendente)*       | `poupar-hook-session-end`  | `session-end.cjs`|

pub mod observe;
pub mod session_end;
pub mod session_init;
pub mod startup;
