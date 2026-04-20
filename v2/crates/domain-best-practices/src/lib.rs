//! # domain-best-practices
//!
//! Porta Rust de `plugin/scripts/best-practices.cjs` — catálogo de
//! best-practices aplicáveis a agentes (padrões de engenharia,
//! acessibilidade, copywriting, etc.).
//!
//! O catálogo global vive em `plugin/best-practices/` (com
//! `_catalog.yaml` + pasta por prática). Cada projeto pode copiar
//! práticas do catálogo pra `<mem_dir>/best-practices/<id>/` e ainda
//! criar práticas locais (source: `local`).
//!
//! ## Escopo (Fase 5)
//!
//! - [`parse_catalog`] — parser YAML tolerante (só o que o catálogo usa).
//! - [`list_practices`] — lê o catálogo global e marca disponibilidade.
//! - [`load_practice_content`] — lê o `practice.md` do catálogo.
//! - [`apply_practices`] — copia práticas do catálogo pro `mem_dir`.
//! - [`list_local_practices`] — enumera práticas já no `mem_dir`.
//! - [`create_local_practice`] / [`update_local_practice`] /
//!   [`delete_local_practice`] — CRUD de práticas locais.
//!
//! Paridade byte-a-byte validada por `tests/parity_*.rs` contra o
//! `.cjs` original.

use serde_json::{Map, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Regex (textual) que valida ID de prática. Padrão `.cjs`:
/// `^[a-z][a-z0-9-]{1,48}[a-z0-9]$` — começa com letra minúscula, 3 a 50
/// caracteres, termina com alfanumérico.
pub const ID_RE: &str = r"^[a-z][a-z0-9-]{1,48}[a-z0-9]$";

// ── catalog parser ──────────────────────────────────────────────────────

/// Parser YAML mínimo — replica exatamente `parseCatalog` em
/// `best-practices.cjs:17-79`. Só cobre o que o catálogo usa:
///
/// - `- id: <value>` começa uma entrada.
/// - `  <key>: <value>` adiciona campo à entrada atual.
/// - `  <key>: |` inicia bloco multi-linha (indent = campo + 2).
/// - `  tags: [a, b, c]` parseia array inline.
/// - Strings entre `"..."` ou `'...'` têm aspas removidas.
/// - Linhas vazias e linhas começando com `#` são ignoradas.
pub fn parse_catalog(text: &str) -> Vec<Map<String, Value>> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut practices: Vec<Map<String, Value>> = Vec::new();
    let mut current: Option<Map<String, Value>> = None;
    let mut in_multi: Option<String> = None;
    let mut multi_buf: Vec<String> = Vec::new();
    let mut multi_indent: usize = 0;

    let flush_multi =
        |cur: &mut Option<Map<String, Value>>,
         im: &mut Option<String>,
         mb: &mut Vec<String>,
         mi: &mut usize| {
            if let (Some(key), Some(map)) = (im.as_ref(), cur.as_mut()) {
                let joined = mb.join("\n");
                // JS .trim() corta whitespace ASCII + nbsp (0xA0), aqui usamos str::trim.
                map.insert(key.clone(), Value::String(joined.trim().to_string()));
            }
            *im = None;
            mb.clear();
            *mi = 0;
        };

    for raw in &lines {
        // `line.replace(/\r$/, '')` — tira só \r no final.
        let line: String = if let Some(stripped) = raw.strip_suffix('\r') {
            stripped.to_string()
        } else {
            (*raw).to_string()
        };
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if in_multi.is_some() {
            let (ws, rest) = split_leading_ws(&line);
            if ws.len() >= multi_indent && multi_indent > 0 {
                multi_buf.push(rest.to_string());
                continue;
            }
            flush_multi(
                &mut current,
                &mut in_multi,
                &mut multi_buf,
                &mut multi_indent,
            );
        }

        // ^\s*-\s+id:\s*(.+)$
        if let Some(val) = match_item_id(&line) {
            if let Some(prev) = current.take() {
                practices.push(prev);
            }
            let mut m = Map::new();
            m.insert("id".into(), Value::String(val.trim().to_string()));
            current = Some(m);
            continue;
        }
        if current.is_none() {
            continue;
        }

        // ^\s+(\w+):\s*(.*)$
        let kv = match match_kv(&line) {
            Some(x) => x,
            None => continue,
        };
        let key = kv.0;
        let mut val = kv.1;

        if val == "|" {
            in_multi = Some(key);
            multi_buf.clear();
            let (ws, _) = split_leading_ws(&line);
            multi_indent = ws.len() + 2;
            continue;
        }

        if key == "tags" {
            if let Some(stripped) = val.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                let arr: Vec<Value> = stripped
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| Value::String(s.to_string()))
                    .collect();
                if let Some(cur) = current.as_mut() {
                    cur.insert("tags".into(), Value::Array(arr));
                }
                continue;
            }
        }

        // quote stripping — só se a string INTEIRA bate o padrão.
        if (val.starts_with('"') && val.ends_with('"') && val.len() >= 2)
            || (val.starts_with('\'') && val.ends_with('\'') && val.len() >= 2)
        {
            val = val[1..val.len() - 1].to_string();
        }

        if let Some(cur) = current.as_mut() {
            cur.insert(key, Value::String(val));
        }
    }

    flush_multi(
        &mut current,
        &mut in_multi,
        &mut multi_buf,
        &mut multi_indent,
    );
    if let Some(cur) = current {
        practices.push(cur);
    }
    practices
}

fn split_leading_ws(s: &str) -> (&str, &str) {
    let n = s.chars().take_while(|c| c.is_whitespace() && *c != '\n').count();
    // JS `\s` inclui \t. Mas `line` aqui não tem \n (split por \n antes).
    let byte_idx = s
        .char_indices()
        .take(n)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    s.split_at(byte_idx)
}

fn match_item_id(line: &str) -> Option<&str> {
    // ^\s*-\s+id:\s*(.+)$
    let s = line.trim_start();
    let s = s.strip_prefix('-')?;
    // exige pelo menos 1 espaço depois do -
    if !s.starts_with(' ') && !s.starts_with('\t') {
        return None;
    }
    let s = s.trim_start();
    let s = s.strip_prefix("id:")?;
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    Some(s)
}

fn match_kv(line: &str) -> Option<(String, String)> {
    // ^\s+(\w+):\s*(.*)$
    let mut chars = line.chars();
    let first = chars.next()?;
    if !first.is_whitespace() || first == '\n' {
        return None;
    }
    let rest = line.trim_start();
    // key = [A-Za-z0-9_]+
    let key_end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    if key_end == 0 {
        return None;
    }
    let key = &rest[..key_end];
    let after = &rest[key_end..];
    let after = after.strip_prefix(':')?;
    // `\s*` — consumir whitespace
    let val = after.trim_start().to_string();
    Some((key.to_string(), val))
}

// ── list_practices ──────────────────────────────────────────────────────

/// Resolve `<bp_dir>/_catalog.yaml`.
pub fn catalog_path(bp_dir: &Path) -> PathBuf {
    bp_dir.join("_catalog.yaml")
}

/// Lê o catálogo global e marca cada prática com `available: bool`
/// dependendo da existência do `practice.md`.
///
/// Replica `listPractices()` em `best-practices.cjs:81-93`. Se o
/// arquivo não existe ou falha no parse, retorna `[]`.
pub fn list_practices(bp_dir: &Path) -> Vec<Map<String, Value>> {
    let cat = catalog_path(bp_dir);
    if !cat.exists() {
        return Vec::new();
    }
    let txt = match fs::read_to_string(&cat) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    // O .cjs faz `try/catch` em volta de parseCatalog — nosso parser não
    // panica, mas mantemos o catch defensivo.
    let practices = parse_catalog(&txt);
    practices
        .into_iter()
        .map(|mut p| {
            let subdir = p
                .get("dir")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .or_else(|| p.get("id").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_default();
            let available = bp_dir.join(&subdir).join("practice.md").exists();
            // JS `{...p, available}` põe `available` no FINAL.
            p.insert("available".into(), Value::Bool(available));
            p
        })
        .collect()
}

/// Lê o conteúdo do `practice.md` de uma prática do catálogo global.
///
/// Replica `loadPracticeContent(id)` em `best-practices.cjs:95-102`.
pub fn load_practice_content(bp_dir: &Path, id: &str) -> Option<String> {
    let list = list_practices(bp_dir);
    let p = list.iter().find(|p| p.get("id").and_then(Value::as_str) == Some(id))?;
    let subdir = p
        .get("dir")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| p.get("id").and_then(Value::as_str).map(str::to_string))?;
    let file = bp_dir.join(subdir).join("practice.md");
    if !file.exists() {
        return None;
    }
    fs::read_to_string(&file).ok()
}

// ── apply_practices ─────────────────────────────────────────────────────

/// Resultado de [`apply_practices`] — arrays disjuntos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyPracticesResult {
    pub copied: Vec<String>,
    pub preserved: Vec<String>,
    pub missing: Vec<String>,
}

/// Erro de [`apply_practices`] — só io, já que o caller é o runtime.
#[derive(Debug, Error)]
pub enum ApplyPracticesError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Copia práticas do catálogo global pro `mem_dir`, escrevendo um
/// `meta.json` simplificado para o runtime. Replica `applyPractices` em
/// `best-practices.cjs:104-132`.
///
/// - `ids` pode ter entradas repetidas — o .cjs processa na ordem e
///   cada ID cai em exatamente um dos três arrays.
/// - Se `dst/practice.md` existe e `overwrite=false`, vai pra `preserved`.
/// - Se prática não está no catálogo ou não tem `practice.md`, vai pra
///   `missing`.
/// - `meta.json` tem chaves na ordem: `id, name, scope, tags, when_to_use`.
pub fn apply_practices(
    bp_dir: &Path,
    mem_dir: &Path,
    ids: &[&str],
    overwrite: bool,
) -> Result<ApplyPracticesResult, ApplyPracticesError> {
    let list = list_practices(bp_dir);
    // Mapa id → practice (última ocorrência vence, igual `Object.fromEntries`).
    let mut by_id: Map<String, Value> = Map::new();
    for p in &list {
        if let Some(id) = p.get("id").and_then(Value::as_str) {
            by_id.insert(id.to_string(), Value::Object(p.clone()));
        }
    }
    let out_dir = mem_dir.join("best-practices");
    if !out_dir.exists() {
        fs::create_dir_all(&out_dir)?;
    }

    let mut copied = Vec::new();
    let mut preserved = Vec::new();
    let mut missing = Vec::new();

    for id in ids {
        let p = match by_id.get(*id).and_then(Value::as_object) {
            Some(p) => p,
            None => {
                missing.push((*id).to_string());
                continue;
            }
        };
        let subdir = p
            .get("dir")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| (*id).to_string());
        let src = bp_dir.join(&subdir).join("practice.md");
        if !src.exists() {
            missing.push((*id).to_string());
            continue;
        }
        let dst_dir = out_dir.join(id);
        if !dst_dir.exists() {
            fs::create_dir_all(&dst_dir)?;
        }
        let dst = dst_dir.join("practice.md");
        if dst.exists() && !overwrite {
            preserved.push((*id).to_string());
            continue;
        }
        fs::copy(&src, &dst)?;

        let mut meta = Map::new();
        meta.insert(
            "id".into(),
            Value::String(
                p.get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_default(),
            ),
        );
        // `p.name` no JS é undefined se ausente — JSON.stringify omite.
        // Aqui reproduzimos: só insere se presente.
        if let Some(name) = p.get("name").cloned() {
            meta.insert("name".into(), name);
        } else {
            meta.insert("name".into(), Value::Null);
            // Hack: JSON.stringify omite undefined, mas aqui serde inclui null.
            // Paridade: remover a chave no .cjs é undefined, que não
            // serializa. Seguimos com Null e o caller ignora?
            // NA PRÁTICA o JS `p.name` é sempre string (vem do catálogo),
            // mas por garantia removemos o Null.
            meta.remove("name");
        }
        meta.insert(
            "scope".into(),
            Value::String(
                p.get("scope")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_default(),
            ),
        );
        meta.insert(
            "tags".into(),
            p.get("tags")
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new())),
        );
        meta.insert(
            "when_to_use".into(),
            Value::String(
                p.get("when_to_use")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_default(),
            ),
        );

        let s = serde_json::to_string_pretty(&Value::Object(meta))?;
        fs::write(dst_dir.join("meta.json"), format!("{s}\n"))?;

        copied.push((*id).to_string());
    }

    Ok(ApplyPracticesResult {
        copied,
        preserved,
        missing,
    })
}

// ── local practice CRUD ─────────────────────────────────────────────────

/// Erro comum para operações de prática local.
#[derive(Debug, Error)]
pub enum LocalPracticeError {
    #[error("id inválido (slug [a-z][a-z0-9-]+[a-z0-9], 3–50 chars)")]
    InvalidId,
    #[error("name obrigatório")]
    NameRequired,
    #[error("content obrigatório")]
    ContentRequired,
    #[error("content inválido")]
    ContentInvalid,
    #[error("content muito longo (máx 200KB)")]
    ContentTooLong,
    #[error(r#"best-practice "{id}" já existe — use overwrite=true pra substituir"#)]
    AlreadyExists { id: String },
    #[error(r#"best-practice "{id}" não existe localmente"#)]
    NotFoundLocal { id: String },
    #[error(r#"best-practice "{id}" não existe"#)]
    NotFound { id: String },
    #[error(r#"best-practice "{id}" veio do catálogo — remova pelo seed/filesystem"#)]
    NotLocalSource { id: String },
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// `true` se `s` bate o regex [`ID_RE`].
pub fn is_valid_id(s: &str) -> bool {
    let len = s.chars().count();
    if !(3..=50).contains(&len) {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_lowercase() {
        return false;
    }
    let last = s.chars().last().unwrap();
    if !(last.is_ascii_lowercase() || last.is_ascii_digit()) {
        return false;
    }
    // miolo: [a-z0-9-]
    let middle_len = len - 2;
    if middle_len == 0 {
        // id length 2 — rejeitado pelo range acima; só chega aqui se 3+.
        return true;
    }
    s.chars()
        .skip(1)
        .take(middle_len)
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Enumera práticas já presentes em `<mem_dir>/best-practices/`.
///
/// Replica `listLocalPractices(memDir)` em `best-practices.cjs:136-162`.
/// Cada entrada carrega meta.json (se existir) com defaults.
pub fn list_local_practices(mem_dir: &Path) -> Vec<Map<String, Value>> {
    let dir = mem_dir.join("best-practices");
    if !dir.exists() {
        return Vec::new();
    }
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    // `fs.readdirSync` no Linux retorna na ordem do diretório (inode),
    // não alfabética. Rust `read_dir` idem. Para estabilidade, o .cjs
    // não ordena — replicamos o comportamento do filesystem sem
    // reordenar. Para testes determinísticos, os twin dirs recebem os
    // mesmos arquivos e a iteração vai bater.
    let _ = &mut names;

    let mut out: Vec<Map<String, Value>> = Vec::new();
    for entry in names {
        let full = dir.join(&entry);
        if !full.is_dir() {
            continue;
        }
        if !full.join("practice.md").exists() {
            continue;
        }
        let meta_file = full.join("meta.json");
        let mut meta: Map<String, Value> = Map::new();
        meta.insert("id".into(), Value::String(entry.clone()));
        if meta_file.exists() {
            if let Ok(raw) = fs::read_to_string(&meta_file) {
                if let Ok(parsed) = serde_json::from_str::<Value>(&raw) {
                    if let Some(obj) = parsed.as_object() {
                        for (k, v) in obj {
                            meta.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
        }
        let mut out_map = Map::new();
        out_map.insert(
            "id".into(),
            Value::String(
                meta.get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| entry.clone()),
            ),
        );
        out_map.insert(
            "name".into(),
            Value::String(js_str_or_chain(
                &meta,
                &["name", "id"],
                &entry,
            )),
        );
        out_map.insert(
            "scope".into(),
            Value::String(js_str_or(meta.get("scope"), "")),
        );
        out_map.insert(
            "tags".into(),
            match meta.get("tags") {
                Some(Value::Array(a)) => Value::Array(a.clone()),
                _ => Value::Array(Vec::new()),
            },
        );
        out_map.insert(
            "when_to_use".into(),
            Value::String(js_str_or(meta.get("when_to_use"), "")),
        );
        out_map.insert(
            "not_for".into(),
            Value::String(js_str_or(meta.get("not_for"), "")),
        );
        out_map.insert("dir".into(), Value::String(entry.clone()));
        out_map.insert(
            "source".into(),
            Value::String(js_str_or(meta.get("source"), "local")),
        );
        out_map.insert("available".into(), Value::Bool(true));
        out.push(out_map);
    }
    out
}

fn js_str_or(v: Option<&Value>, fallback: &str) -> String {
    match v.and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => fallback.to_string(),
    }
}

/// Replica `meta.name || meta.id || entry` — cada fallback é truthy-checked.
fn js_str_or_chain(meta: &Map<String, Value>, keys: &[&str], fallback: &str) -> String {
    for k in keys {
        if let Some(Value::String(s)) = meta.get(*k) {
            if !s.is_empty() {
                return s.clone();
            }
        }
    }
    fallback.to_string()
}

/// Input de [`create_local_practice`].
#[derive(Debug, Clone, Default)]
pub struct CreateLocalInput<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub scope: Option<&'a str>,
    pub tags: Option<&'a [Value]>,
    pub when_to_use: Option<&'a str>,
    pub not_for: Option<&'a str>,
    pub content: &'a str,
}

/// Resultado de [`create_local_practice`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateLocalResult {
    pub id: String,
    pub path: PathBuf,
    pub meta: Map<String, Value>,
}

/// Cria uma prática local. Replica `createLocalPractice` em
/// `best-practices.cjs:164-193`.
///
/// Validações:
/// - `id` bate [`ID_RE`].
/// - `name` é string não-vazia.
/// - `content` é string não-vazia após trim.
/// - `content.len() <= 200_000` (em bytes para UTF-8; JS usa UTF-16 mas
///   pra ASCII é a mesma coisa).
///
/// Se `practice.md` existe e `overwrite=false`, falha com
/// `AlreadyExists`. `content` ganha `\n` no fim se não terminar com
/// newline.
pub fn create_local_practice(
    mem_dir: &Path,
    input: &CreateLocalInput,
    overwrite: bool,
) -> Result<CreateLocalResult, LocalPracticeError> {
    if input.id.is_empty() || !is_valid_id(input.id) {
        return Err(LocalPracticeError::InvalidId);
    }
    if input.name.is_empty() {
        return Err(LocalPracticeError::NameRequired);
    }
    if input.content.trim().is_empty() {
        return Err(LocalPracticeError::ContentRequired);
    }
    // JS length == UTF-16 units. Para ASCII é o mesmo que bytes; para
    // Unicode difere. Replicamos com chars UTF-16.
    let content_len_16: usize = input.content.encode_utf16().count();
    if content_len_16 > 200_000 {
        return Err(LocalPracticeError::ContentTooLong);
    }

    let dir = mem_dir.join("best-practices").join(input.id);
    let practice_file = dir.join("practice.md");
    if practice_file.exists() && !overwrite {
        return Err(LocalPracticeError::AlreadyExists {
            id: input.id.to_string(),
        });
    }
    fs::create_dir_all(&dir)?;
    let normalized: String = if input.content.ends_with('\n') {
        input.content.to_string()
    } else {
        format!("{}\n", input.content)
    };
    fs::write(&practice_file, &normalized)?;

    let mut meta = Map::new();
    meta.insert("id".into(), Value::String(input.id.to_string()));
    meta.insert("name".into(), Value::String(input.name.trim().to_string()));
    meta.insert(
        "scope".into(),
        Value::String(input.scope.map(|s| s.trim().to_string()).unwrap_or_default()),
    );
    meta.insert(
        "tags".into(),
        Value::Array(sanitize_tags(input.tags.unwrap_or(&[]))),
    );
    meta.insert(
        "when_to_use".into(),
        Value::String(
            input
                .when_to_use
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
        ),
    );
    meta.insert(
        "not_for".into(),
        Value::String(input.not_for.map(|s| s.trim().to_string()).unwrap_or_default()),
    );
    meta.insert("source".into(), Value::String("local".into()));

    let s = serde_json::to_string_pretty(&Value::Object(meta.clone()))?;
    fs::write(dir.join("meta.json"), format!("{s}\n"))?;

    Ok(CreateLocalResult {
        id: input.id.to_string(),
        path: practice_file,
        meta,
    })
}

/// Tags: `tags.filter(t => typeof t === 'string' && t.trim()).map(t => t.trim())`.
fn sanitize_tags(tags: &[Value]) -> Vec<Value> {
    tags.iter()
        .filter_map(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| Value::String(s.to_string()))
        .collect()
}

/// Input de [`update_local_practice`]. Campos `None` = "não mexer";
/// `Some` = atualizar. `content: None` preserva practice.md.
#[derive(Debug, Clone, Default)]
pub struct UpdateLocalInput<'a> {
    pub name: Option<&'a str>,
    pub scope: Option<&'a str>,
    pub tags: Option<&'a [Value]>,
    pub when_to_use: Option<&'a str>,
    pub not_for: Option<&'a str>,
    pub content: Option<&'a str>,
}

/// Resultado de [`update_local_practice`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateLocalResult {
    pub id: String,
    pub meta: Map<String, Value>,
}

/// Atualiza parcialmente uma prática local.
///
/// Replica `updateLocalPractice` em `best-practices.cjs:195-225`.
/// Campos ausentes no input preservam o que já estava em meta.json.
/// Sempre força `meta.id = id` e `meta.source = 'local'` no final.
pub fn update_local_practice(
    mem_dir: &Path,
    id: &str,
    input: &UpdateLocalInput,
) -> Result<UpdateLocalResult, LocalPracticeError> {
    if !is_valid_id(id) {
        return Err(LocalPracticeError::InvalidId);
    }
    let dir = mem_dir.join("best-practices").join(id);
    let practice_file = dir.join("practice.md");
    if !practice_file.exists() {
        return Err(LocalPracticeError::NotFoundLocal { id: id.to_string() });
    }
    if let Some(content) = input.content {
        if content.trim().is_empty() {
            return Err(LocalPracticeError::ContentInvalid);
        }
        let content_len_16: usize = content.encode_utf16().count();
        if content_len_16 > 200_000 {
            return Err(LocalPracticeError::ContentTooLong);
        }
        let normalized = if content.ends_with('\n') {
            content.to_string()
        } else {
            format!("{content}\n")
        };
        fs::write(&practice_file, &normalized)?;
    }
    let meta_file = dir.join("meta.json");
    let mut meta: Map<String, Value> = Map::new();
    meta.insert("id".into(), Value::String(id.to_string()));
    meta.insert("source".into(), Value::String("local".into()));
    if meta_file.exists() {
        if let Ok(raw) = fs::read_to_string(&meta_file) {
            if let Ok(parsed) = serde_json::from_str::<Value>(&raw) {
                if let Some(obj) = parsed.as_object() {
                    for (k, v) in obj {
                        meta.insert(k.clone(), v.clone());
                    }
                }
            }
        }
    }
    if let Some(name) = input.name {
        meta.insert("name".into(), Value::String(name.trim().to_string()));
    }
    if let Some(scope) = input.scope {
        meta.insert("scope".into(), Value::String(scope.trim().to_string()));
    }
    if let Some(tags) = input.tags {
        meta.insert("tags".into(), Value::Array(sanitize_tags(tags)));
    }
    if let Some(when_to_use) = input.when_to_use {
        meta.insert(
            "when_to_use".into(),
            Value::String(when_to_use.trim().to_string()),
        );
    }
    if let Some(not_for) = input.not_for {
        meta.insert("not_for".into(), Value::String(not_for.trim().to_string()));
    }
    // Forçar id + source no fim, igual .cjs.
    meta.insert("id".into(), Value::String(id.to_string()));
    meta.insert("source".into(), Value::String("local".into()));

    let s = serde_json::to_string_pretty(&Value::Object(meta.clone()))?;
    fs::write(&meta_file, format!("{s}\n"))?;

    Ok(UpdateLocalResult {
        id: id.to_string(),
        meta,
    })
}

/// Resultado de [`delete_local_practice`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteLocalResult {
    pub id: String,
    pub removed: bool,
}

/// Remove uma prática local. Replica `deleteLocalPractice` em
/// `best-practices.cjs:227-242`.
///
/// Só remove se `meta.source === 'local'` (ou ausente). Se veio do
/// catálogo (`source: 'catalog'` ou qualquer outra coisa ≠ `local`),
/// erra.
pub fn delete_local_practice(
    mem_dir: &Path,
    id: &str,
) -> Result<DeleteLocalResult, LocalPracticeError> {
    if !is_valid_id(id) {
        return Err(LocalPracticeError::InvalidId);
    }
    let dir = mem_dir.join("best-practices").join(id);
    if !dir.exists() {
        return Err(LocalPracticeError::NotFound { id: id.to_string() });
    }
    let meta_file = dir.join("meta.json");
    let mut source = "local".to_string();
    if meta_file.exists() {
        if let Ok(raw) = fs::read_to_string(&meta_file) {
            if let Ok(Value::Object(o)) = serde_json::from_str::<Value>(&raw) {
                if let Some(s) = o.get("source").and_then(Value::as_str) {
                    if !s.is_empty() {
                        source = s.to_string();
                    }
                }
            }
        }
    }
    if source != "local" {
        return Err(LocalPracticeError::NotLocalSource { id: id.to_string() });
    }
    fs::remove_dir_all(&dir)?;
    Ok(DeleteLocalResult {
        id: id.to_string(),
        removed: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn id_regex_bounds() {
        assert!(is_valid_id("abc"));
        assert!(is_valid_id("abc-def"));
        assert!(is_valid_id("a1z"));
        assert!(!is_valid_id("ab")); // too short
        assert!(!is_valid_id(&"a".repeat(2)));
        assert!(!is_valid_id("-abc"));
        assert!(!is_valid_id("abc-"));
        assert!(!is_valid_id("Abc"));
        assert!(!is_valid_id("abc_"));
        assert!(!is_valid_id(""));
        assert!(is_valid_id(&format!("a{}", "b".repeat(49))));
        assert!(!is_valid_id(&format!("a{}", "b".repeat(50))));
    }

    #[test]
    fn parse_catalog_minimal() {
        let yaml = "version: 1\n\npractices:\n  - id: foo\n    name: Foo\n    tags: [a, b]\n";
        let r = parse_catalog(yaml);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0]["id"], "foo");
        assert_eq!(r[0]["name"], "Foo");
        assert_eq!(r[0]["tags"], serde_json::json!(["a", "b"]));
    }

    #[test]
    fn parse_catalog_multi_line() {
        let yaml = "practices:\n  - id: x\n    when_to_use: |\n      line 1\n      line 2\n    scope: s\n";
        let r = parse_catalog(yaml);
        assert_eq!(r[0]["when_to_use"], "line 1\nline 2");
        assert_eq!(r[0]["scope"], "s");
    }

    #[test]
    fn parse_catalog_quote_strip() {
        let yaml = "practices:\n  - id: x\n    name: \"Quoted\"\n    note: 'single'\n";
        let r = parse_catalog(yaml);
        assert_eq!(r[0]["name"], "Quoted");
        assert_eq!(r[0]["note"], "single");
    }

    #[test]
    fn parse_catalog_empty() {
        assert!(parse_catalog("").is_empty());
        assert!(parse_catalog("# just a comment\n").is_empty());
    }

    #[test]
    fn list_practices_missing_catalog() {
        let tmp = TempDir::new().unwrap();
        assert!(list_practices(tmp.path()).is_empty());
    }

    #[test]
    fn list_practices_with_catalog() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("_catalog.yaml"),
            "practices:\n  - id: foo\n    name: Foo\n    dir: foo\n",
        )
        .unwrap();
        // sem practice.md → not available
        let r = list_practices(tmp.path());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0]["available"], false);

        fs::create_dir_all(tmp.path().join("foo")).unwrap();
        fs::write(tmp.path().join("foo").join("practice.md"), "body").unwrap();
        let r = list_practices(tmp.path());
        assert_eq!(r[0]["available"], true);
    }

    #[test]
    fn apply_practices_copies_and_meta() {
        let tmp = TempDir::new().unwrap();
        let bp = tmp.path().join("bp");
        let mem = tmp.path().join("mem");
        fs::create_dir_all(bp.join("foo")).unwrap();
        fs::write(
            bp.join("_catalog.yaml"),
            "practices:\n  - id: foo\n    name: Foo\n    scope: s\n    tags: [a, b]\n    when_to_use: |\n      reason\n",
        )
        .unwrap();
        fs::write(bp.join("foo").join("practice.md"), "hello").unwrap();

        let r = apply_practices(&bp, &mem, &["foo"], false).unwrap();
        assert_eq!(r.copied, vec!["foo"]);
        assert!(r.preserved.is_empty());
        assert!(r.missing.is_empty());
        let copied = fs::read_to_string(mem.join("best-practices/foo/practice.md")).unwrap();
        assert_eq!(copied, "hello");
        let meta_raw = fs::read_to_string(mem.join("best-practices/foo/meta.json")).unwrap();
        assert!(meta_raw.ends_with('\n'));
        let meta: Value = serde_json::from_str(&meta_raw).unwrap();
        assert_eq!(meta["id"], "foo");
        assert_eq!(meta["name"], "Foo");
        assert_eq!(meta["tags"], serde_json::json!(["a", "b"]));
    }

    #[test]
    fn apply_practices_preserved_when_exists() {
        let tmp = TempDir::new().unwrap();
        let bp = tmp.path().join("bp");
        let mem = tmp.path().join("mem");
        fs::create_dir_all(bp.join("foo")).unwrap();
        fs::write(
            bp.join("_catalog.yaml"),
            "practices:\n  - id: foo\n    name: Foo\n",
        )
        .unwrap();
        fs::write(bp.join("foo").join("practice.md"), "orig").unwrap();
        fs::create_dir_all(mem.join("best-practices/foo")).unwrap();
        fs::write(mem.join("best-practices/foo/practice.md"), "CUSTOM").unwrap();

        let r = apply_practices(&bp, &mem, &["foo"], false).unwrap();
        assert_eq!(r.preserved, vec!["foo"]);
        assert_eq!(
            fs::read_to_string(mem.join("best-practices/foo/practice.md")).unwrap(),
            "CUSTOM"
        );
    }

    #[test]
    fn apply_practices_missing_ids() {
        let tmp = TempDir::new().unwrap();
        let bp = tmp.path().join("bp");
        let mem = tmp.path().join("mem");
        fs::create_dir_all(&bp).unwrap();
        fs::write(bp.join("_catalog.yaml"), "practices:\n").unwrap();
        let r = apply_practices(&bp, &mem, &["ghost"], false).unwrap();
        assert_eq!(r.missing, vec!["ghost"]);
    }

    #[test]
    fn create_local_happy_path() {
        let tmp = TempDir::new().unwrap();
        let r = create_local_practice(
            tmp.path(),
            &CreateLocalInput {
                id: "foo",
                name: "Foo",
                scope: Some("engineering"),
                tags: Some(&[Value::String("a".into()), Value::String("b".into())]),
                when_to_use: Some("always"),
                not_for: Some("never"),
                content: "body",
            },
            false,
        )
        .unwrap();
        assert_eq!(r.id, "foo");
        assert!(r.path.exists());
        let md = fs::read_to_string(&r.path).unwrap();
        assert_eq!(md, "body\n"); // normalized
        assert_eq!(r.meta["source"], "local");
    }

    #[test]
    fn create_local_errors() {
        let tmp = TempDir::new().unwrap();
        // id inválido
        assert!(matches!(
            create_local_practice(
                tmp.path(),
                &CreateLocalInput {
                    id: "Bad",
                    name: "N",
                    content: "x",
                    ..Default::default()
                },
                false
            ),
            Err(LocalPracticeError::InvalidId)
        ));
        // name vazio
        assert!(matches!(
            create_local_practice(
                tmp.path(),
                &CreateLocalInput {
                    id: "foo",
                    name: "",
                    content: "x",
                    ..Default::default()
                },
                false
            ),
            Err(LocalPracticeError::NameRequired)
        ));
        // content whitespace-only
        assert!(matches!(
            create_local_practice(
                tmp.path(),
                &CreateLocalInput {
                    id: "foo",
                    name: "N",
                    content: "   ",
                    ..Default::default()
                },
                false
            ),
            Err(LocalPracticeError::ContentRequired)
        ));
    }

    #[test]
    fn create_local_already_exists() {
        let tmp = TempDir::new().unwrap();
        create_local_practice(
            tmp.path(),
            &CreateLocalInput {
                id: "foo",
                name: "N",
                content: "x",
                ..Default::default()
            },
            false,
        )
        .unwrap();
        let err = create_local_practice(
            tmp.path(),
            &CreateLocalInput {
                id: "foo",
                name: "N",
                content: "y",
                ..Default::default()
            },
            false,
        )
        .unwrap_err();
        assert!(matches!(err, LocalPracticeError::AlreadyExists { .. }));
        // overwrite=true passa
        create_local_practice(
            tmp.path(),
            &CreateLocalInput {
                id: "foo",
                name: "N",
                content: "y",
                ..Default::default()
            },
            true,
        )
        .unwrap();
    }

    #[test]
    fn update_local_partial() {
        let tmp = TempDir::new().unwrap();
        create_local_practice(
            tmp.path(),
            &CreateLocalInput {
                id: "foo",
                name: "Old",
                scope: Some("s"),
                content: "body",
                ..Default::default()
            },
            false,
        )
        .unwrap();
        let r = update_local_practice(
            tmp.path(),
            "foo",
            &UpdateLocalInput {
                name: Some("New"),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.meta["name"], "New");
        assert_eq!(r.meta["scope"], "s"); // preservado
        assert_eq!(r.meta["source"], "local");
    }

    #[test]
    fn update_local_missing() {
        let tmp = TempDir::new().unwrap();
        let err = update_local_practice(tmp.path(), "ghost", &UpdateLocalInput::default())
            .unwrap_err();
        assert!(matches!(err, LocalPracticeError::NotFoundLocal { .. }));
    }

    #[test]
    fn delete_local_refuses_non_local() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("best-practices/foo");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("practice.md"), "x").unwrap();
        fs::write(
            dir.join("meta.json"),
            r#"{"id":"foo","source":"catalog"}"#,
        )
        .unwrap();
        let err = delete_local_practice(tmp.path(), "foo").unwrap_err();
        assert!(matches!(err, LocalPracticeError::NotLocalSource { .. }));
    }

    #[test]
    fn delete_local_happy_path() {
        let tmp = TempDir::new().unwrap();
        create_local_practice(
            tmp.path(),
            &CreateLocalInput {
                id: "foo",
                name: "N",
                content: "x",
                ..Default::default()
            },
            false,
        )
        .unwrap();
        let r = delete_local_practice(tmp.path(), "foo").unwrap();
        assert!(r.removed);
        assert!(!tmp.path().join("best-practices/foo").exists());
    }

    #[test]
    fn list_local_enumerates() {
        let tmp = TempDir::new().unwrap();
        create_local_practice(
            tmp.path(),
            &CreateLocalInput {
                id: "foo",
                name: "Foo",
                content: "x",
                ..Default::default()
            },
            false,
        )
        .unwrap();
        create_local_practice(
            tmp.path(),
            &CreateLocalInput {
                id: "bar",
                name: "Bar",
                content: "y",
                ..Default::default()
            },
            false,
        )
        .unwrap();
        let r = list_local_practices(tmp.path());
        assert_eq!(r.len(), 2);
        let ids: std::collections::HashSet<&str> = r
            .iter()
            .map(|m| m["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains("foo"));
        assert!(ids.contains("bar"));
    }

    #[test]
    fn load_practice_content_round_trip() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("_catalog.yaml"),
            "practices:\n  - id: foo\n    dir: foo\n",
        )
        .unwrap();
        fs::create_dir_all(tmp.path().join("foo")).unwrap();
        fs::write(tmp.path().join("foo").join("practice.md"), "hello").unwrap();
        let c = load_practice_content(tmp.path(), "foo").unwrap();
        assert_eq!(c, "hello");
        assert!(load_practice_content(tmp.path(), "ghost").is_none());
    }
}
