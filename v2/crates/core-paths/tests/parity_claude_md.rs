//! Paridade v1 (`plugin/scripts/lib.cjs :: syncClaudeMd`) vs v2
//! (`core_paths::claude_md::sync_claude_md`).
//!
//! Estratégia: dois dirs irmãos (`a/` = v1, `b/` = v2) com o mesmo
//! estado inicial, invocação da mesma função em cada, comparação
//! byte-a-byte de `<root>/.claude/CLAUDE.md`.
//!
//! `syncClaudeMd(projectRoot, content)` tem 3 caminhos + 1 sub-caminho
//! (malformed). Cada um coberto por ao menos um teste.

use core_paths::{sync_claude_md, CLAUDE_DIR_NAME, CLAUDE_MD_END, CLAUDE_MD_FILE, CLAUDE_MD_START};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// Invoca `syncClaudeMd(projectRoot, content)` no `.cjs` e espera
/// sucesso. Não imprime nada — só grava o arquivo no disco.
const NODE_SCRIPT: &str = r#"
const lib = require(process.argv[1]);
const projectRoot = process.argv[2];
const content = process.argv[3];
lib.syncClaudeMd(projectRoot, content);
"#;

fn lib_cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/lib.cjs")
        .canonicalize()
        .expect("lib.cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn call_v1(project_root: &Path, content: &str) {
    let out = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(lib_cjs_path())
        .arg(project_root)
        .arg(content)
        .output()
        .expect("spawn node");
    assert!(
        out.status.success(),
        "node falhou: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Cria dois project roots irmãos com estado inicial idêntico.
/// Se `initial_content` for `Some`, pré-grava `.claude/CLAUDE.md` nele.
fn twin_roots(tmp: &TempDir, initial_content: Option<&str>) -> (PathBuf, PathBuf) {
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    if let Some(content) = initial_content {
        let a_claude = a.join(CLAUDE_DIR_NAME);
        let b_claude = b.join(CLAUDE_DIR_NAME);
        fs::create_dir_all(&a_claude).unwrap();
        fs::create_dir_all(&b_claude).unwrap();
        fs::write(a_claude.join(CLAUDE_MD_FILE), content).unwrap();
        fs::write(b_claude.join(CLAUDE_MD_FILE), content).unwrap();
    }
    (a, b)
}

fn claude_md(root: &Path) -> PathBuf {
    root.join(CLAUDE_DIR_NAME).join(CLAUDE_MD_FILE)
}

fn read_bytes(p: &Path) -> Vec<u8> {
    fs::read(p).expect("read CLAUDE.md")
}

// ─── Caminho 1: arquivo ausente ────────────────────────────────────────────

#[test]
fn parity_creates_file_when_missing() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_roots(&tmp, None);
    call_v1(&a, "hello world");
    sync_claude_md(&b, "hello world").unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

#[test]
fn parity_creates_directory_when_missing() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_roots(&tmp, None);
    // Nem `.claude/` existe — ambos devem criar.
    assert!(!a.join(CLAUDE_DIR_NAME).exists());
    assert!(!b.join(CLAUDE_DIR_NAME).exists());
    call_v1(&a, "x");
    sync_claude_md(&b, "x").unwrap();
    assert!(a.join(CLAUDE_DIR_NAME).is_dir());
    assert!(b.join(CLAUDE_DIR_NAME).is_dir());
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

#[test]
fn parity_trims_content_with_surrounding_whitespace() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_roots(&tmp, None);
    let content = "  \n\ntrimmed\n\n  ";
    call_v1(&a, content);
    sync_claude_md(&b, content).unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

#[test]
fn parity_multiline_content() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_roots(&tmp, None);
    let content = "# Header\n\n- bullet 1\n- bullet 2\n\n## Sub\n\nbody text.";
    call_v1(&a, content);
    sync_claude_md(&b, content).unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

#[test]
fn parity_empty_content_after_trim() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    // `.trim()` do JS e `.trim()` do Rust ambos removem whitespace
    // Unicode → aqui o conteúdo miolo vira string vazia. `block` fica
    // `START\n\nEND`. Edge case digno de paridade.
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_roots(&tmp, None);
    call_v1(&a, "   \n\t  \n");
    sync_claude_md(&b, "   \n\t  \n").unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

// ─── Caminho 3: arquivo existe sem marker ──────────────────────────────────

#[test]
fn parity_appends_block_to_existing_file_without_marker() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_roots(&tmp, Some("# user content\n\nfoo\n"));
    call_v1(&a, "plugin content");
    sync_claude_md(&b, "plugin content").unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

#[test]
fn parity_appends_with_trailing_whitespace_in_existing() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    // `.trimEnd()` do JS e do Rust devem se comportar igual em
    // whitespace Unicode trailing (\n, espaço, tab).
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_roots(&tmp, Some("user text\n\n\n   \t\n"));
    call_v1(&a, "novo");
    sync_claude_md(&b, "novo").unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

#[test]
fn parity_appends_with_no_trailing_newline_in_existing() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    // Arquivo sem `\n` final — `trimEnd()` não muda nada.
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_roots(&tmp, Some("sem newline"));
    call_v1(&a, "x");
    sync_claude_md(&b, "x").unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

// ─── Caminho 2a: arquivo com bloco bem-formado ─────────────────────────────

#[test]
fn parity_replaces_block_when_markers_present() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let initial = format!(
        "# header\n\n{CLAUDE_MD_START}\nantigo\n{CLAUDE_MD_END}\n\n# footer\n"
    );
    let (a, b) = twin_roots(&tmp, Some(&initial));
    call_v1(&a, "novo");
    sync_claude_md(&b, "novo").unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

#[test]
fn parity_replaces_block_preserves_surrounding_whitespace() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    // Sem newline entre header e START — garante que o `before` é
    // preservado literal.
    let tmp = TempDir::new().unwrap();
    let initial = format!("x{CLAUDE_MD_START}\nmid\n{CLAUDE_MD_END}y");
    let (a, b) = twin_roots(&tmp, Some(&initial));
    call_v1(&a, "novo");
    sync_claude_md(&b, "novo").unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

#[test]
fn parity_replaces_block_at_start_of_file() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let initial = format!("{CLAUDE_MD_START}\nold\n{CLAUDE_MD_END}\n\n# footer\n");
    let (a, b) = twin_roots(&tmp, Some(&initial));
    call_v1(&a, "new");
    sync_claude_md(&b, "new").unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

#[test]
fn parity_replaces_block_at_end_of_file() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let initial = format!("# header\n\n{CLAUDE_MD_START}\nold\n{CLAUDE_MD_END}");
    let (a, b) = twin_roots(&tmp, Some(&initial));
    call_v1(&a, "new");
    sync_claude_md(&b, "new").unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

// ─── Caminho 2b: malformado (START sem END) ────────────────────────────────

#[test]
fn parity_malformed_appends_fresh_block() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let initial = format!("pre\n{CLAUDE_MD_START}\nsem fim\n");
    let (a, b) = twin_roots(&tmp, Some(&initial));
    call_v1(&a, "novo");
    sync_claude_md(&b, "novo").unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

// ─── Idempotência / múltiplas invocações ───────────────────────────────────

#[test]
fn parity_idempotent_same_content() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_roots(&tmp, None);
    call_v1(&a, "content");
    call_v1(&a, "content");
    sync_claude_md(&b, "content").unwrap();
    sync_claude_md(&b, "content").unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

#[test]
fn parity_update_flow_preserves_user_prelude() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_roots(&tmp, Some("USER TOP\n"));
    // Sync v1, depois v2 — simulando upgrade de conteúdo.
    call_v1(&a, "v1");
    call_v1(&a, "v2");
    sync_claude_md(&b, "v1").unwrap();
    sync_claude_md(&b, "v2").unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

// ─── Edge cases de bug audit ───────────────────────────────────────────────

#[test]
fn parity_content_containing_marker_literal() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    // Conteúdo do PROJECT.md mencionando o marker dentro dele — se o
    // bloco incluir `CLAUDE_MD_END` literal, a próxima invocação vai
    // truncar no primeiro END. Documentamos isso como comportamento,
    // não como bug. Aqui só verificamos paridade.
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_roots(&tmp, None);
    let content = format!("# doc\n\n{CLAUDE_MD_END} este end é literal");
    call_v1(&a, &content);
    sync_claude_md(&b, &content).unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

#[test]
fn parity_crlf_line_endings_in_existing() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    // Arquivo criado em Windows com \r\n — `trimEnd` do JS remove
    // whitespace Unicode incluindo \r; Rust faz o mesmo
    // (`char::is_whitespace`).
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_roots(&tmp, Some("line1\r\nline2\r\n"));
    call_v1(&a, "novo");
    sync_claude_md(&b, "novo").unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

#[test]
fn parity_bom_prefixed_existing_file() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    // `fs.readFileSync(..., 'utf8')` do Node preserva BOM;
    // `fs::read_to_string` do Rust também. O BOM fica no `existing`
    // e é mantido no append. Verificamos paridade byte-a-byte.
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_roots(&tmp, Some("\u{FEFF}hello\n"));
    call_v1(&a, "x");
    sync_claude_md(&b, "x").unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

#[test]
fn parity_content_with_embedded_newlines_and_tabs() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_roots(&tmp, None);
    let content = "a\tb\n\tc\n  \td";
    call_v1(&a, content);
    sync_claude_md(&b, content).unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}

#[test]
fn parity_content_with_unicode_including_nbsp() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    // `.trim()` em JS remove \u00A0; `str::trim` do Rust também
    // (char::is_whitespace inclui NBSP).
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_roots(&tmp, None);
    let content = "\u{00A0}\u{00A0}olá \u{00E9} mundo\u{00A0}";
    call_v1(&a, content);
    sync_claude_md(&b, content).unwrap();
    assert_eq!(read_bytes(&claude_md(&a)), read_bytes(&claude_md(&b)));
}
