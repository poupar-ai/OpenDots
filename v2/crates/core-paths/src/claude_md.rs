//! CLAUDE.md sync — porta de `syncClaudeMd(projectRoot, projectMdContent)`
//! em `lib.cjs`.
//!
//! ```js
//! const CLAUDE_MD_START = '<!-- open-poupar.ai:start -->';
//! const CLAUDE_MD_END   = '<!-- open-poupar.ai:end -->';
//!
//! function syncClaudeMd(projectRoot, projectMdContent) {
//!   const claudeDir    = path.join(projectRoot, '.claude');
//!   const claudeMdPath = path.join(claudeDir, 'CLAUDE.md');
//!   const block = `${CLAUDE_MD_START}\n${projectMdContent.trim()}\n${CLAUDE_MD_END}`;
//!
//!   if (!fs.existsSync(claudeMdPath)) {
//!     fs.mkdirSync(claudeDir, { recursive: true });
//!     fs.writeFileSync(claudeMdPath, block + '\n');
//!     return;
//!   }
//!
//!   const existing = fs.readFileSync(claudeMdPath, 'utf8');
//!   if (existing.includes(CLAUDE_MD_START)) {
//!     const startIdx = existing.indexOf(CLAUDE_MD_START);
//!     const endIdx   = existing.indexOf(CLAUDE_MD_END);
//!     if (endIdx === -1) {
//!       fs.writeFileSync(claudeMdPath, existing.trimEnd() + '\n\n' + block + '\n');
//!     } else {
//!       const before = existing.slice(0, startIdx);
//!       const after  = existing.slice(endIdx + CLAUDE_MD_END.length);
//!       fs.writeFileSync(claudeMdPath, before + block + after);
//!     }
//!   } else {
//!     fs.writeFileSync(claudeMdPath, existing.trimEnd() + '\n\n' + block + '\n');
//!   }
//! }
//! ```
//!
//! ## Por que existe
//!
//! Claude Code lê automaticamente o `CLAUDE.md` da raiz do projeto. Este
//! módulo injeta o conteúdo do `PROJECT.md` (memória estruturada do
//! plugin) num bloco delimitado por comentários HTML, preservando o
//! conteúdo pré-existente que o usuário escreveu manualmente.
//!
//! ## Três caminhos
//!
//! 1. **Arquivo ausente** — cria `.claude/CLAUDE.md` com `block + "\n"`.
//! 2. **Arquivo existe com bloco marcado** — substitui apenas o miolo
//!    entre `START` e `END`.
//!    - **Malformed** (`START` presente, `END` ausente): append de bloco
//!      fresco ao final (sem tocar no existente com `START` órfão).
//! 3. **Arquivo existe sem marker** — append: `existing.trimEnd() +
//!    "\n\n" + block + "\n"`.
//!
//! ## `trimEnd()` JS vs Rust
//!
//! `String.prototype.trimEnd` do JS remove whitespace Unicode no fim
//! (inclusive `\u00A0`, `\u2028`, `\u2029`). `str::trim_end` do Rust usa
//! a mesma definição (`char::is_whitespace`), que também inclui
//! whitespace Unicode. Paridade garantida para os casos realistas
//! (espaços, newlines, tabs).

use std::fs;
use std::io;
use std::path::Path;

/// Marker de início do bloco gerenciado pelo plugin.
pub const CLAUDE_MD_START: &str = "<!-- open-poupar.ai:start -->";

/// Marker de fim.
pub const CLAUDE_MD_END: &str = "<!-- open-poupar.ai:end -->";

/// Diretório relativo ao project root onde o `CLAUDE.md` é gravado.
pub const CLAUDE_DIR_NAME: &str = ".claude";

/// Nome do arquivo dentro de `.claude/`.
pub const CLAUDE_MD_FILE: &str = "CLAUDE.md";

/// Sincroniza o conteúdo do PROJECT.md num bloco marcado dentro de
/// `<project_root>/.claude/CLAUDE.md`, preservando conteúdo do usuário.
///
/// Replica `syncClaudeMd(projectRoot, projectMdContent)` byte-a-byte.
///
/// ### Erros
///
/// Propaga `io::Error` de `mkdirSync`/`writeFileSync`. O `.cjs` não tem
/// try/catch aqui — o único caller (`session-init.cjs:130`) envolve em
/// `try { } catch {}`. No Rust o caller faz `.ok()` ou tratamento
/// explícito.
pub fn sync_claude_md(project_root: &Path, project_md_content: &str) -> io::Result<()> {
    let claude_dir = project_root.join(CLAUDE_DIR_NAME);
    let claude_md_path = claude_dir.join(CLAUDE_MD_FILE);

    // `projectMdContent.trim()` no JS remove whitespace Unicode das duas
    // pontas (espaços, newlines, tabs). `str::trim` do Rust tem a mesma
    // definição.
    let block = format!(
        "{}\n{}\n{}",
        CLAUDE_MD_START,
        project_md_content.trim(),
        CLAUDE_MD_END
    );

    // Caminho 1: arquivo não existe — cria diretório + arquivo com
    // bloco + newline final.
    if !claude_md_path.exists() {
        fs::create_dir_all(&claude_dir)?;
        fs::write(&claude_md_path, format!("{block}\n"))?;
        return Ok(());
    }

    let existing = fs::read_to_string(&claude_md_path)?;

    // Caminho 2: arquivo existe e tem o marker START.
    if let Some(start_idx) = existing.find(CLAUDE_MD_START) {
        match existing.find(CLAUDE_MD_END) {
            // 2b: malformado (START presente, END ausente) — append fresh.
            None => {
                let new_content = format!("{}\n\n{}\n", existing.trim_end(), block);
                fs::write(&claude_md_path, new_content)?;
            }
            // 2a: bloco bem-formado — substitui miolo.
            Some(end_idx) => {
                let before = &existing[..start_idx];
                let after = &existing[end_idx + CLAUDE_MD_END.len()..];
                let new_content = format!("{before}{block}{after}");
                fs::write(&claude_md_path, new_content)?;
            }
        }
        return Ok(());
    }

    // Caminho 3: arquivo existe sem marker — append ao final.
    let new_content = format!("{}\n\n{}\n", existing.trim_end(), block);
    fs::write(&claude_md_path, new_content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn read(p: &Path) -> String {
        fs::read_to_string(p).unwrap()
    }

    fn claude_md_in(root: &Path) -> std::path::PathBuf {
        root.join(CLAUDE_DIR_NAME).join(CLAUDE_MD_FILE)
    }

    #[test]
    fn creates_file_when_missing() {
        let tmp = TempDir::new().unwrap();
        sync_claude_md(tmp.path(), "hello world").unwrap();
        let got = read(&claude_md_in(tmp.path()));
        assert_eq!(
            got,
            format!("{CLAUDE_MD_START}\nhello world\n{CLAUDE_MD_END}\n")
        );
    }

    #[test]
    fn creates_directory_if_missing() {
        let tmp = TempDir::new().unwrap();
        assert!(!tmp.path().join(".claude").exists());
        sync_claude_md(tmp.path(), "x").unwrap();
        assert!(tmp.path().join(".claude").is_dir());
    }

    #[test]
    fn trims_content_before_wrapping() {
        let tmp = TempDir::new().unwrap();
        sync_claude_md(tmp.path(), "  \n\ntrimmed\n\n  ").unwrap();
        let got = read(&claude_md_in(tmp.path()));
        // `.trim()` tira whitespace de ambos os lados, preservando o
        // conteúdo interior.
        assert_eq!(
            got,
            format!("{CLAUDE_MD_START}\ntrimmed\n{CLAUDE_MD_END}\n")
        );
    }

    #[test]
    fn appends_block_to_existing_file_without_marker() {
        let tmp = TempDir::new().unwrap();
        let path = claude_md_in(tmp.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "# user content\n\nfoo\n").unwrap();
        sync_claude_md(tmp.path(), "plugin content").unwrap();
        let got = read(&path);
        // `.trim_end()` do existing + `\n\n` + block + `\n`.
        assert_eq!(
            got,
            format!(
                "# user content\n\nfoo\n\n{CLAUDE_MD_START}\nplugin content\n{CLAUDE_MD_END}\n"
            )
        );
    }

    #[test]
    fn replaces_block_when_markers_present() {
        let tmp = TempDir::new().unwrap();
        let path = claude_md_in(tmp.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Conteúdo prévio com bloco antigo + texto ao redor.
        let initial = format!(
            "# header\n\n{CLAUDE_MD_START}\nantigo\n{CLAUDE_MD_END}\n\n# footer\n"
        );
        fs::write(&path, &initial).unwrap();
        sync_claude_md(tmp.path(), "novo").unwrap();
        let got = read(&path);
        assert_eq!(
            got,
            format!(
                "# header\n\n{CLAUDE_MD_START}\nnovo\n{CLAUDE_MD_END}\n\n# footer\n"
            )
        );
    }

    #[test]
    fn malformed_append_when_only_start_marker() {
        let tmp = TempDir::new().unwrap();
        let path = claude_md_in(tmp.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // START órfão — END ausente.
        let initial = format!("pre\n{CLAUDE_MD_START}\nsem fim\n");
        fs::write(&path, &initial).unwrap();
        sync_claude_md(tmp.path(), "novo").unwrap();
        let got = read(&path);
        // Append fresh, sem remover o START órfão — preserva o conteúdo
        // existente na íntegra.
        assert_eq!(
            got,
            format!(
                "pre\n{CLAUDE_MD_START}\nsem fim\n\n{CLAUDE_MD_START}\nnovo\n{CLAUDE_MD_END}\n"
            )
        );
    }

    #[test]
    fn idempotent_second_sync_with_same_content() {
        let tmp = TempDir::new().unwrap();
        sync_claude_md(tmp.path(), "content").unwrap();
        let first = read(&claude_md_in(tmp.path()));
        sync_claude_md(tmp.path(), "content").unwrap();
        let second = read(&claude_md_in(tmp.path()));
        assert_eq!(first, second);
    }

    #[test]
    fn update_changes_only_managed_section() {
        let tmp = TempDir::new().unwrap();
        let path = claude_md_in(tmp.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "USER TOP\n").unwrap();
        sync_claude_md(tmp.path(), "v1").unwrap();
        sync_claude_md(tmp.path(), "v2").unwrap();
        let got = read(&path);
        assert!(got.starts_with("USER TOP\n"), "user content preservado");
        assert!(got.contains("v2"), "novo conteúdo presente");
        assert!(!got.contains("v1"), "antigo conteúdo removido");
    }
}
