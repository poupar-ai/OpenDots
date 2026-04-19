//! # core-paths
//!
//! Porta Rust de `plugin/scripts/lib.cjs` — **apenas a parte de paths e
//! diretórios**, que é a fundação usada por todo o resto do plugin
//! project-mem (observações, grafo, sessões, cofre, bus de agentes).
//!
//! ## Por que existe este crate
//!
//! No `.cjs` original, todo script que quer ler/gravar memória começa
//! resolvendo `<projectRoot>/.claude-project-memory` a partir do `cwd`
//! vindo do Claude Code. Essa resolução:
//!
//! 1. Descobre a raiz do projeto via `git rev-parse --show-toplevel`
//!    (com timeout e fallback para o próprio `cwd` se não houver repo).
//! 2. Anexa a constante `.claude-project-memory`.
//! 3. Garante que o diretório e seu sub-diretório `tmp/` existam.
//!
//! Este crate entrega exatamente esse pipeline, byte-a-byte idêntico ao
//! `.cjs`, validado por testes de paridade cross-language (`tests/parity_*.rs`)
//! que invocam `node` sobre o `.cjs` original e comparam o output ao
//! nosso.
//!
//! ## Escopo (Fase 2 do plano de migração)
//!
//! Pontos já portados:
//!
//! - **Ponto 1** — constantes + resolução (`MEM_DIR_NAME`,
//!   [`find_project_root`], [`mem_dir`], [`tmp_dir`]).
//! - **Ponto 2** — ensure helpers idempotentes ([`ensure_mem_dir`],
//!   [`ensure_tmp_dir`]).
//!
//! Pontos pendentes (próximos, um por vez): JSONL I/O, estimativa de
//! tokens, stats, project config, sync do CLAUDE.md, stdin/cwd helpers.
//!
//! ## Garantias de paridade
//!
//! Este crate é um **port literal**, não um redesign. Regras:
//!
//! - Mesmos nomes públicos (em `snake_case` em Rust) mapeando 1:1 com os
//!   exports do `.cjs`.
//! - Mesmo comportamento em fallback/erro (ex.: `find_project_root`
//!   nunca falha — sempre devolve um path, igual ao `.cjs`).
//! - Testes de paridade obrigatórios para cada função pública.
//!
//! Se algum dia `.cjs` e este crate divergirem, o crate está errado até
//! prova em contrário.

pub mod jsonl;
pub use jsonl::{append_jsonl, read_jsonl, read_last_jsonl_line, JsonlError};

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Nome do diretório de memória, fixo na raiz do projeto.
///
/// Equivalente a `MEM_DIR_NAME` no `.cjs`. Constante pública porque
/// outros crates (observações, grafo, cofre) vão compor paths com ela
/// diretamente.
pub const MEM_DIR_NAME: &str = ".claude-project-memory";

// ─── path.join paridade ─────────────────────────────────────────────────────

/// Replica `path.posix.normalize` do Node:
/// - Colapsa múltiplos `/` consecutivos.
/// - Remove segmentos `.`.
/// - Resolve `..` colapsando com segmento anterior (se houver e não for `..`).
/// - Remove trailing `/` exceto quando o resultado é a raiz `/`.
/// - Retorna `.` se um path relativo normaliza para vazio.
///
/// Precisa existir porque `PathBuf::join` do Rust NÃO normaliza — mantém
/// `./a`, `//`, `a/./b` literais. `path.join` do Node normaliza sempre.
/// Sem isso, os retornos stringificados divergem do `.cjs` em paths com
/// `./`, `//`, `..` no meio.
fn normalize_posix(input: &str) -> String {
    let is_absolute = input.starts_with('/');
    // `//` no início (UNC no POSIX é implementation-defined) — Node colapsa para `/`.
    // Logo usamos apenas `is_absolute` como prefixo.
    let mut stack: Vec<&str> = Vec::new();
    for seg in input.split('/') {
        match seg {
            "" | "." => continue,
            ".." => {
                // Num path absoluto, `..` acima da raiz somem.
                // Num path relativo, `..` se empilha quando não há
                // segmento "real" anterior para pop.
                match stack.last() {
                    Some(&top) if top != ".." => {
                        stack.pop();
                    }
                    _ => {
                        if !is_absolute {
                            stack.push("..");
                        }
                    }
                }
            }
            other => stack.push(other),
        }
    }
    let joined = stack.join("/");
    match (is_absolute, joined.is_empty()) {
        (true, true) => "/".to_string(),
        (true, false) => format!("/{joined}"),
        (false, true) => ".".to_string(),
        (false, false) => joined,
    }
}

/// Equivalente a `path.posix.join(a, b)`: concatena com `/` e aplica
/// `normalize_posix`.
fn path_join(a: &Path, b: &str) -> PathBuf {
    let lossy = a.to_string_lossy();
    let concat = if lossy.is_empty() {
        b.to_string()
    } else {
        format!("{}/{}", lossy, b)
    };
    PathBuf::from(normalize_posix(&concat))
}

// ─── find_project_root ──────────────────────────────────────────────────────

/// Descobre a raiz do projeto spawn-ando `git rev-parse --show-toplevel`
/// com `cwd = start_cwd`.
///
/// Replica `findProjectRoot(startCwd)` do `.cjs`. Comportamento:
///
/// - Se `git` retorna sucesso com stdout não vazio, devolve esse path.
/// - Se `git` não está instalado, o diretório não é um repo, o comando
///   falha por qualquer outro motivo, **ou** passou de 3 segundos,
///   devolve `start_cwd` sem propagar erro.
///
/// Essa função é infalível por design: tudo que consome memória
/// espera um path de volta, nunca um erro. Essa é a mesma filosofia do
/// `.cjs`, que faz `try { ... } catch { return startCwd; }`.
///
/// ### Timeout
/// 3000 ms, idêntico ao `.cjs`. Implementado com polling de `try_wait`
/// a cada 20 ms porque `std::process::Child` não oferece timeout nativo
/// no stable (evita pull de `tokio` ou `wait-timeout` só para isso).
pub fn find_project_root(start_cwd: &Path) -> PathBuf {
    // stdin null / stdout+stderr piped exatamente como o .cjs
    // (`stdio: ['pipe','pipe','pipe']`). stderr piped para não poluir
    // o terminal quando start_cwd não é repo git.
    let mut child = match Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .current_dir(start_cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        // git ausente no PATH, ou erro de spawn — fallback no cwd.
        Err(_) => return start_cwd.to_path_buf(),
    };

    let deadline = std::time::Instant::now() + Duration::from_millis(3000);
    loop {
        match child.try_wait() {
            // Saída com status 0 — consome stdout e usa como root.
            Ok(Some(status)) if status.success() => {
                let out = child.wait_with_output().ok();
                if let Some(out) = out {
                    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !s.is_empty() {
                        return PathBuf::from(s);
                    }
                }
                // stdout vazio é cenário inesperado — fallback defensivo.
                return start_cwd.to_path_buf();
            }
            // Saída com status != 0 (não é repo git, etc.) — fallback.
            Ok(Some(_)) => return start_cwd.to_path_buf(),
            // Ainda rodando — respeita deadline e dorme 20ms.
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    // Timeout. Mata o processo para não deixar zombie.
                    let _ = child.kill();
                    let _ = child.wait();
                    return start_cwd.to_path_buf();
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            // try_wait() errou — fallback silencioso como o .cjs.
            Err(_) => return start_cwd.to_path_buf(),
        }
    }
}

// ─── Path helpers puros ─────────────────────────────────────────────────────

/// Resolve `<project_root>/.claude-project-memory` a partir de um `cwd`.
///
/// Replica `getMemDir(cwd)` do `.cjs`. Infalível — herda o fallback de
/// [`find_project_root`]. Usa [`path_join`] para replicar o
/// comportamento de normalização do `path.join` do Node (colapsa `//`,
/// resolve `./` e `..`).
pub fn mem_dir(cwd: &Path) -> PathBuf {
    path_join(&find_project_root(cwd), MEM_DIR_NAME)
}

/// Resolve `<mem_dir>/tmp` — diretório usado para artefatos voláteis
/// (flags de sessão, locks curtos, caches).
///
/// Replica `getTmpDir(memDir)`. Puramente sintático, não toca disco.
/// Normaliza via [`path_join`] para paridade byte-a-byte com Node.
pub fn tmp_dir(mem_dir: &Path) -> PathBuf {
    path_join(mem_dir, "tmp")
}

// ─── ensure_* helpers (criação idempotente) ─────────────────────────────────

/// Erros de I/O ao criar diretórios. Fino wrapper em `std::io::Error`
/// — não inventamos tipos de erro que o `.cjs` não tinha (lá é só
/// `throw` do `fs.mkdirSync`).
#[derive(Debug, thiserror::Error)]
pub enum EnsureError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Garante que `mem_dir` existe. Cria recursivamente se faltar.
///
/// Replica `ensureMemDir(memDir)` do `.cjs`:
/// ```js
/// if (!fs.existsSync(memDir)) fs.mkdirSync(memDir, { recursive: true });
/// ```
///
/// Idempotente — chamar sobre diretório existente é no-op. O `.cjs`
/// retorna `undefined`; aqui retornamos `Result<()>` só para propagar
/// erro de I/O (permissão negada, etc.).
pub fn ensure_mem_dir(mem_dir: &Path) -> Result<(), EnsureError> {
    if !mem_dir.exists() {
        std::fs::create_dir_all(mem_dir)?;
    }
    Ok(())
}

/// Garante `<mem_dir>/tmp` e devolve o path resultante.
///
/// Replica `ensureTmpDir(memDir)` — note que o `.cjs` também devolve o
/// path do tmp, não apenas cria. Se `mem_dir` ainda não existe,
/// `create_dir_all` sobe a árvore inteira (mesma semântica do
/// `fs.mkdirSync(... { recursive: true })`).
pub fn ensure_tmp_dir(mem_dir: &Path) -> Result<PathBuf, EnsureError> {
    let tmp = tmp_dir(mem_dir);
    if !tmp.exists() {
        std::fs::create_dir_all(&tmp)?;
    }
    Ok(tmp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn mem_dir_name_matches_cjs_constant() {
        assert_eq!(MEM_DIR_NAME, ".claude-project-memory");
    }

    #[test]
    fn tmp_dir_joins_tmp_suffix() {
        let base = PathBuf::from("/x/y/.claude-project-memory");
        assert_eq!(tmp_dir(&base), PathBuf::from("/x/y/.claude-project-memory/tmp"));
    }

    #[test]
    fn find_project_root_falls_back_outside_git_repo() {
        let tmp = TempDir::new().unwrap();
        // TempDir não é repo git — deve retornar o próprio path.
        let root = find_project_root(tmp.path());
        // Canonicalize porque macOS/linux podem ter symlink prefix (/private, /tmp).
        let expected = tmp.path().canonicalize().unwrap();
        let got = root.canonicalize().unwrap();
        assert_eq!(got, expected);
    }

    #[test]
    fn find_project_root_returns_toplevel_inside_repo() {
        let tmp = TempDir::new().unwrap();
        let status = Command::new("git")
            .arg("init")
            .arg("-q")
            .current_dir(tmp.path())
            .status();
        if !status.map(|s| s.success()).unwrap_or(false) {
            eprintln!("git indisponível — pulando");
            return;
        }
        // Sub-diretório dentro do repo — git deve subir e achar o toplevel.
        let sub = tmp.path().join("a/b");
        std::fs::create_dir_all(&sub).unwrap();
        let root = find_project_root(&sub);
        let expected = tmp.path().canonicalize().unwrap();
        let got = root.canonicalize().unwrap();
        assert_eq!(got, expected);
    }

    #[test]
    fn ensure_mem_dir_creates_when_missing() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("nested/mem");
        assert!(!target.exists());
        ensure_mem_dir(&target).unwrap();
        assert!(target.is_dir());
    }

    #[test]
    fn ensure_mem_dir_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("m");
        ensure_mem_dir(&target).unwrap();
        // Chama de novo — não deve quebrar nem alterar nada.
        ensure_mem_dir(&target).unwrap();
        assert!(target.is_dir());
    }

    #[test]
    fn ensure_tmp_dir_creates_and_returns_path() {
        let tmp = TempDir::new().unwrap();
        let mem = tmp.path().join("m");
        std::fs::create_dir_all(&mem).unwrap();
        let got = ensure_tmp_dir(&mem).unwrap();
        assert_eq!(got, mem.join("tmp"));
        assert!(got.is_dir());
    }

    #[test]
    fn ensure_tmp_dir_creates_parent_if_missing() {
        // Mesmo que memDir não exista, mkdirSync recursive cria tudo.
        let tmp = TempDir::new().unwrap();
        let mem = tmp.path().join("missing/m");
        let got = ensure_tmp_dir(&mem).unwrap();
        assert_eq!(got, mem.join("tmp"));
        assert!(got.is_dir());
    }

    #[test]
    fn normalize_posix_colapsa_barras_duplas() {
        assert_eq!(normalize_posix("/a//b"), "/a/b");
        assert_eq!(normalize_posix("/a///b/c"), "/a/b/c");
    }

    #[test]
    fn normalize_posix_remove_dots_simples() {
        assert_eq!(normalize_posix("./mem"), "mem");
        assert_eq!(normalize_posix("a/./b"), "a/b");
        assert_eq!(normalize_posix("."), ".");
    }

    #[test]
    fn normalize_posix_resolve_dotdot() {
        assert_eq!(normalize_posix("a/b/../c"), "a/c");
        assert_eq!(normalize_posix("../mem"), "../mem");
        assert_eq!(normalize_posix("/a/b/.."), "/a");
    }

    #[test]
    fn normalize_posix_trailing_slash() {
        assert_eq!(normalize_posix("/a/b/"), "/a/b");
        assert_eq!(normalize_posix("/"), "/");
        assert_eq!(normalize_posix(""), ".");
    }

    #[test]
    fn tmp_dir_normaliza_dot_prefix() {
        // Antes da correção isso retornava "./mem/tmp" — divergia do
        // `.cjs` que retorna "mem/tmp".
        let got = tmp_dir(Path::new("./mem"));
        assert_eq!(got, PathBuf::from("mem/tmp"));
    }

    #[test]
    fn tmp_dir_colapsa_barras_duplas() {
        let got = tmp_dir(Path::new("/a//b"));
        assert_eq!(got, PathBuf::from("/a/b/tmp"));
    }

    #[test]
    fn mem_dir_appends_constant_to_project_root() {
        let tmp = TempDir::new().unwrap();
        let got = mem_dir(tmp.path());
        let expected = tmp.path().canonicalize().unwrap().join(MEM_DIR_NAME);
        assert_eq!(
            got.canonicalize().ok().unwrap_or(got.clone()),
            expected.canonicalize().ok().unwrap_or(expected)
        );
    }
}
