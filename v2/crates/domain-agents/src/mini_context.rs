//! Mini-context I/O — porta de `loadMiniContext` e `saveMiniContext`
//! em `agents.cjs:422-442`.
//!
//! ```js
//! function loadMiniContext(memDir, contextFile) {
//!   const p = path.join(memDir, contextFile);
//!   if (!fs.existsSync(p)) return null;
//!   try { return fs.readFileSync(p, 'utf8').trim(); }
//!   catch { return null; }
//! }
//!
//! function saveMiniContext(memDir, contextFile, content) {
//!   if (typeof content !== 'string') throw new Error('content precisa ser string');
//!   if (content.length > 200_000) throw new Error('mini-contexto muito longo (máx 200KB)');
//!   const rel = contextFile || '';
//!   if (!rel || rel.includes('..') || path.isAbsolute(rel)) {
//!     throw new Error('context_file inválido');
//!   }
//!   const p = path.join(memDir, rel);
//!   const dir = path.dirname(p);
//!   if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
//!   const normalized = content.endsWith('\n') ? content : content + '\n';
//!   fs.writeFileSync(p, normalized);
//!   return p;
//! }
//! ```
//!
//! ## Detalhes de paridade
//!
//! - `content.length > 200_000` em JS é **UTF-16 code units**, não
//!   bytes nem chars. Usamos `encode_utf16().count()` no Rust.
//! - `path.isAbsolute(rel)` no POSIX é `startsWith('/')`. Usamos
//!   `Path::is_absolute` que no Linux/macOS tem o mesmo comportamento.
//! - `rel.includes('..')` é substring match — catch `../x` mas também
//!   `my..file.md` ou `x/..not-parent/y`. Paridade literal — over-
//!   restritivo, mas compatível.
//! - `.trim()` em JS strip também BOM (`U+FEFF`); Rust `str::trim`
//!   NÃO strip. Usamos [`js_trim`] para paridade byte-a-byte.

use crate::js_trim::js_trim;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Limite de tamanho em UTF-16 code units (mesmo que `content.length`
/// em JS).
pub const MAX_LENGTH_UTF16: usize = 200_000;

/// Erros de [`save_mini_context`]. `load_mini_context` nunca falha —
/// retorna `None` em todo caso de erro.
#[derive(Debug, Error)]
pub enum MiniContextError {
    /// Conteúdo maior que 200_000 code units UTF-16.
    #[error("mini-contexto muito longo (máx 200KB)")]
    TooLong,
    /// `context_file` vazio, contém `..` ou é absoluto.
    #[error("context_file inválido")]
    InvalidContextFile,
    /// Erro de I/O no mkdir ou no write.
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

/// Carrega e trim-a o conteúdo de `<mem_dir>/<context_file>`.
///
/// Retorna `None` se:
/// - o arquivo não existe;
/// - leitura falha (permissão, I/O).
///
/// Nunca propaga erro (paridade com `try/catch` do `.cjs`).
pub fn load_mini_context(mem_dir: &Path, context_file: &str) -> Option<String> {
    let path = mem_dir.join(context_file);
    if !path.exists() {
        return None;
    }
    let raw = fs::read_to_string(&path).ok()?;
    Some(js_trim(&raw).to_string())
}

/// Valida `context_file` + grava `content` em
/// `<mem_dir>/<context_file>` com trailing `\n` garantido.
///
/// Cria diretório pai recursivamente se faltar.
///
/// Devolve o path absoluto gravado (mesma semântica do `.cjs` que
/// retorna `p`).
///
/// ### Validação
///
/// 1. `content` não pode exceder 200_000 UTF-16 code units.
/// 2. `context_file` não pode ser vazio, conter `..`, ou ser absoluto.
pub fn save_mini_context(
    mem_dir: &Path,
    context_file: &str,
    content: &str,
) -> Result<PathBuf, MiniContextError> {
    // 1. Limite de tamanho (UTF-16 code units, paridade com JS).
    if content.encode_utf16().count() > MAX_LENGTH_UTF16 {
        return Err(MiniContextError::TooLong);
    }
    // 2. Validação do path relativo.
    if context_file.is_empty()
        || context_file.contains("..")
        || Path::new(context_file).is_absolute()
    {
        return Err(MiniContextError::InvalidContextFile);
    }

    let path = mem_dir.join(context_file);

    // 3. Criar parent se faltar.
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    // 4. Normalizar trailing `\n`.
    let normalized = if content.ends_with('\n') {
        content.to_string()
    } else {
        format!("{content}\n")
    };

    fs::write(&path, normalized)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ─── load_mini_context ─────────────────────────────────────────────────

    #[test]
    fn load_returns_none_when_missing() {
        let tmp = TempDir::new().unwrap();
        assert!(load_mini_context(tmp.path(), "agents/manager.md").is_none());
    }

    #[test]
    fn load_trims_whitespace() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("x.md");
        fs::write(&path, "\n\n  hello world  \n\n").unwrap();
        let got = load_mini_context(tmp.path(), "x.md").unwrap();
        assert_eq!(got, "hello world");
    }

    #[test]
    fn load_trims_bom_prefix() {
        // Paridade com `.trim()` do JS que strip BOM. Rust str::trim
        // não strip — usamos js_trim.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("x.md");
        fs::write(&path, "\u{FEFF}hello\n").unwrap();
        let got = load_mini_context(tmp.path(), "x.md").unwrap();
        assert_eq!(got, "hello");
    }

    #[test]
    fn load_empty_file_returns_empty_string() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("x.md");
        fs::write(&path, "").unwrap();
        assert_eq!(
            load_mini_context(tmp.path(), "x.md"),
            Some(String::new())
        );
    }

    // ─── save_mini_context ─────────────────────────────────────────────────

    #[test]
    fn save_writes_and_appends_newline() {
        let tmp = TempDir::new().unwrap();
        let path = save_mini_context(tmp.path(), "agents/manager.md", "body").unwrap();
        let got = fs::read_to_string(&path).unwrap();
        assert_eq!(got, "body\n");
    }

    #[test]
    fn save_does_not_double_newline() {
        let tmp = TempDir::new().unwrap();
        let path = save_mini_context(tmp.path(), "agents/x.md", "body\n").unwrap();
        let got = fs::read_to_string(&path).unwrap();
        assert_eq!(got, "body\n");
    }

    #[test]
    fn save_creates_missing_parent_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(!tmp.path().join("agents").exists());
        save_mini_context(tmp.path(), "agents/new.md", "x").unwrap();
        assert!(tmp.path().join("agents").is_dir());
    }

    #[test]
    fn save_returns_written_path() {
        let tmp = TempDir::new().unwrap();
        let got = save_mini_context(tmp.path(), "agents/x.md", "body").unwrap();
        assert_eq!(got, tmp.path().join("agents/x.md"));
    }

    #[test]
    fn save_rejects_empty_context_file() {
        let tmp = TempDir::new().unwrap();
        let err = save_mini_context(tmp.path(), "", "body").unwrap_err();
        assert!(matches!(err, MiniContextError::InvalidContextFile));
    }

    #[test]
    fn save_rejects_absolute_path() {
        let tmp = TempDir::new().unwrap();
        let err = save_mini_context(tmp.path(), "/etc/passwd", "body").unwrap_err();
        assert!(matches!(err, MiniContextError::InvalidContextFile));
    }

    #[test]
    fn save_rejects_dotdot_traversal() {
        let tmp = TempDir::new().unwrap();
        let err = save_mini_context(tmp.path(), "../escape.md", "body").unwrap_err();
        assert!(matches!(err, MiniContextError::InvalidContextFile));
    }

    #[test]
    fn save_rejects_dotdot_in_middle() {
        // Substring match — over-restritivo mas paridade com `.cjs`.
        let tmp = TempDir::new().unwrap();
        let err = save_mini_context(tmp.path(), "safe/../other.md", "body").unwrap_err();
        assert!(matches!(err, MiniContextError::InvalidContextFile));
    }

    #[test]
    fn save_rejects_dotdot_as_substring_even_in_filename() {
        // "my..file.md" contains ".." — .cjs rejeita, Rust rejeita também.
        let tmp = TempDir::new().unwrap();
        let err = save_mini_context(tmp.path(), "my..file.md", "body").unwrap_err();
        assert!(matches!(err, MiniContextError::InvalidContextFile));
    }

    #[test]
    fn save_rejects_content_over_200k_utf16() {
        let tmp = TempDir::new().unwrap();
        let big = "a".repeat(MAX_LENGTH_UTF16 + 1);
        let err = save_mini_context(tmp.path(), "x.md", &big).unwrap_err();
        assert!(matches!(err, MiniContextError::TooLong));
    }

    #[test]
    fn save_accepts_content_at_exactly_200k_utf16() {
        let tmp = TempDir::new().unwrap();
        let big = "a".repeat(MAX_LENGTH_UTF16);
        save_mini_context(tmp.path(), "x.md", &big).unwrap();
    }

    #[test]
    fn save_counts_utf16_code_units_not_bytes() {
        // Emoji 🦀 = 2 UTF-16 code units, 4 bytes UTF-8. Podemos caber
        // 100_000 emojis (= 200_000 code units).
        let tmp = TempDir::new().unwrap();
        let exact = "🦀".repeat(MAX_LENGTH_UTF16 / 2);
        assert_eq!(exact.encode_utf16().count(), MAX_LENGTH_UTF16);
        save_mini_context(tmp.path(), "x.md", &exact).unwrap();

        let over = format!("{exact}x");
        let err = save_mini_context(tmp.path(), "x.md", &over).unwrap_err();
        assert!(matches!(err, MiniContextError::TooLong));
    }

    #[test]
    fn save_preserves_existing_dir() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("agents")).unwrap();
        fs::write(tmp.path().join("agents/other.md"), "keep me").unwrap();
        save_mini_context(tmp.path(), "agents/new.md", "body").unwrap();
        assert!(tmp.path().join("agents/other.md").exists());
    }

    // ─── roundtrip ─────────────────────────────────────────────────────────

    #[test]
    fn roundtrip_save_then_load() {
        let tmp = TempDir::new().unwrap();
        save_mini_context(tmp.path(), "agents/x.md", "content body").unwrap();
        let loaded = load_mini_context(tmp.path(), "agents/x.md").unwrap();
        // load faz .trim(), remove o \n final que save adicionou.
        assert_eq!(loaded, "content body");
    }
}
