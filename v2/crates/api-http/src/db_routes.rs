//! Snapshot + restore do "banco" (mem_dir).
//!
//! - `POST /api/db/snapshot` body opcional `{label}` → cria
//!   `<mem_dir>/snapshots/<ts>[-label].tar.gz` contendo todos os arquivos
//!   relevantes do mem_dir + `_manifest.json` com sha256 por arquivo.
//! - `GET  /api/db/snapshots` → lista snapshots existentes.
//! - `POST /api/db/restore` body `{snapshot: "filename.tar.gz"}` →
//!   valida manifest (sha256 de cada arquivo dentro do tar bate) e
//!   substitui arquivos atuais. Backup do estado anterior vai pra
//!   `snapshots/_pre_restore_<ts>/`.
//!
//! Pra MVP, restore aceita só snapshot **já existente** em
//! `snapshots/`. Upload via multipart fica pra fase futura.
//!
//! ## Filtros
//!
//! Snapshot inclui tudo de `mem_dir` exceto:
//! - `snapshots/` (evita recursão)
//! - `queue/state.json.tmp` (lixo de atomic write)
//! - Arquivos > 100MB (provavelmente lixo ou dump indevido)
//!
//! ## Lock
//!
//! Sem lock global hoje — snapshot tirado em alta carga pode pegar
//! estado parcialmente atualizado. Aceitável pra MVP (callers devem
//! chamar quando a carga estiver baixa). Lock cooperativo é fase
//! futura se virar dor real.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::tenant::TenantContext;
use crate::{json_error, json_response, AppState};

pub fn db_router() -> Router<AppState> {
    Router::new()
        .route("/api/db/snapshot", post(snapshot_handler))
        .route("/api/db/snapshots", get(list_snapshots_handler))
        .route("/api/db/restore", post(restore_handler))
}

const MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;
const MANIFEST_NAME: &str = "_manifest.json";

#[derive(Serialize, Deserialize)]
struct FileEntry {
    path: String,
    size: u64,
    sha256: String,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    created_at_ms: u64,
    label: Option<String>,
    files: Vec<FileEntry>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn snapshots_dir(mem_dir: &Path) -> PathBuf {
    mem_dir.join("snapshots")
}

fn sha256_bytes(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}

/// Coleta paths de tudo dentro de mem_dir que entra no snapshot.
/// Skip: `snapshots/`, `*.tmp`, arquivos > MAX_FILE_BYTES, symlinks.
fn collect_snapshot_paths(mem_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(root: &Path, base: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(base) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            // Pula symlinks (poderia escapar do mem_dir).
            let Ok(meta) = entry.file_type() else { continue };
            if meta.is_symlink() {
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if meta.is_dir() {
                // Pula a própria pasta snapshots pra evitar recursão.
                if path == root.join("snapshots") {
                    continue;
                }
                walk(root, &path, out);
            } else if meta.is_file() {
                if name_str.ends_with(".tmp") {
                    continue;
                }
                if let Ok(m) = path.metadata() {
                    if m.len() > MAX_FILE_BYTES {
                        eprintln!("snapshot skip (>{MAX_FILE_BYTES}B): {}", path.display());
                        continue;
                    }
                }
                out.push(path);
            }
        }
    }
    walk(mem_dir, mem_dir, &mut out);
    out.sort();
    out
}

/// Cria tarball + manifest em `<mem_dir>/snapshots/<ts>[-label].tar.gz`.
fn create_snapshot(mem_dir: &Path, label: Option<&str>) -> std::io::Result<(PathBuf, Manifest, u64)> {
    let dir = snapshots_dir(mem_dir);
    fs::create_dir_all(&dir)?;
    let ts = now_ms();
    let filename = match label {
        Some(l) if !l.is_empty() => format!("{ts}-{l}.tar.gz"),
        _ => format!("{ts}.tar.gz"),
    };
    let out_path = dir.join(filename);

    let paths = collect_snapshot_paths(mem_dir);
    let mut files: Vec<FileEntry> = Vec::with_capacity(paths.len());

    let f = fs::File::create(&out_path)?;
    let gz = GzEncoder::new(f, Compression::default());
    let mut tar_builder = tar::Builder::new(gz);

    for path in &paths {
        let rel = match path.strip_prefix(mem_dir) {
            Ok(r) => r.to_path_buf(),
            Err(_) => continue,
        };
        let bytes = fs::read(path)?;
        let sha = sha256_bytes(&bytes);
        files.push(FileEntry {
            path: rel.to_string_lossy().into_owned(),
            size: bytes.len() as u64,
            sha256: sha,
        });
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar_builder.append_data(&mut header, &rel, &bytes[..])?;
    }

    let manifest = Manifest {
        version: 1,
        created_at_ms: ts,
        label: label.map(|s| s.to_string()),
        files,
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest_bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar_builder.append_data(&mut header, MANIFEST_NAME, &manifest_bytes[..])?;

    let gz = tar_builder.into_inner()?;
    let _ = gz.finish()?;
    let size = fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    Ok((out_path, manifest, size))
}

#[derive(Debug)]
enum RestoreError {
    Io(std::io::Error),
    Manifest(String),
    ChecksumMismatch { path: String, expected: String, got: String },
    SnapshotNotFound,
    PathEscape(String),
}

impl From<std::io::Error> for RestoreError {
    fn from(e: std::io::Error) -> Self {
        RestoreError::Io(e)
    }
}

/// Extrai snapshot num temp, valida sha256 de cada arquivo, faz backup
/// do estado atual em `snapshots/_pre_restore_<ts>/` e move arquivos
/// validados pro mem_dir.
fn apply_restore(mem_dir: &Path, snapshot_file: &str) -> Result<(usize, PathBuf), RestoreError> {
    let snap_path = snapshots_dir(mem_dir).join(snapshot_file);
    if !snap_path.exists() {
        return Err(RestoreError::SnapshotNotFound);
    }

    // Extrai pra dir temporário dentro de snapshots/ pra ficar no mesmo
    // filesystem (rename atomic).
    let extract_dir = snapshots_dir(mem_dir).join(format!("_extract_{}", now_ms()));
    fs::create_dir_all(&extract_dir)?;

    let f = fs::File::open(&snap_path)?;
    let gz = GzDecoder::new(f);
    let mut archive = tar::Archive::new(gz);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        // Segurança: rejeita paths absolutos ou com `..`.
        if path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(RestoreError::PathEscape(path.to_string_lossy().into_owned()));
        }
        let dest = extract_dir.join(&path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        entry.unpack(&dest)?;
    }

    // Lê manifest e valida sha256.
    let manifest_bytes = fs::read(extract_dir.join(MANIFEST_NAME))?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| RestoreError::Manifest(e.to_string()))?;

    for entry in &manifest.files {
        let bytes = fs::read(extract_dir.join(&entry.path))?;
        let got = sha256_bytes(&bytes);
        if got != entry.sha256 {
            let _ = fs::remove_dir_all(&extract_dir);
            return Err(RestoreError::ChecksumMismatch {
                path: entry.path.clone(),
                expected: entry.sha256.clone(),
                got,
            });
        }
    }

    // Backup do estado atual: move snapshots dos arquivos antigos pra
    // pre_restore. Apenas o que existe e está no manifest (não toca em
    // arquivos novos que não existem no snapshot — preserve-by-default).
    let backup_dir = snapshots_dir(mem_dir).join(format!("_pre_restore_{}", now_ms()));
    fs::create_dir_all(&backup_dir)?;
    for entry in &manifest.files {
        let current = mem_dir.join(&entry.path);
        if current.exists() {
            let backup = backup_dir.join(&entry.path);
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&current, &backup)?;
        }
    }

    // Move arquivos extraídos pro mem_dir.
    let mut restored = 0;
    for entry in &manifest.files {
        let src = extract_dir.join(&entry.path);
        let dst = mem_dir.join(&entry.path);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&src, &dst)?;
        restored += 1;
    }
    let _ = fs::remove_dir_all(&extract_dir);

    Ok((restored, backup_dir))
}

// ── Handlers ────────────────────────────────────────────────────────────────

async fn snapshot_handler(
    Extension(tc): Extension<TenantContext>,
    body: Bytes,
) -> Response {
    let label = serde_json::from_slice::<Value>(&body)
        .ok()
        .as_ref()
        .and_then(|v| v.get("label"))
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));

    match create_snapshot(&tc.mem_dir, label.as_deref()) {
        Ok((path, manifest, size)) => json_response(
            StatusCode::CREATED,
            json!({
                "ok": true,
                "snapshot": path.file_name().and_then(|s| s.to_str()).unwrap_or(""),
                "size_bytes": size,
                "file_count": manifest.files.len(),
                "created_at_ms": manifest.created_at_ms,
                "label": manifest.label,
            }),
        ),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("snapshot falhou: {e}")),
    }
}

async fn list_snapshots_handler(Extension(tc): Extension<TenantContext>) -> Response {
    let dir = snapshots_dir(&tc.mem_dir);
    let Ok(entries) = fs::read_dir(&dir) else {
        return json_response(StatusCode::OK, json!({ "snapshots": [] }));
    };
    let mut list: Vec<Value> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else { continue };
        if !name.ends_with(".tar.gz") {
            continue;
        }
        let size = path.metadata().map(|m| m.len()).unwrap_or(0);
        // Tenta extrair ts do nome (prefixo até `-` ou `.tar.gz`).
        let ts_str = name
            .trim_end_matches(".tar.gz")
            .split('-')
            .next()
            .unwrap_or("");
        let ts_ms: u64 = ts_str.parse().unwrap_or(0);
        let label = name.trim_end_matches(".tar.gz").split_once('-').map(|(_, l)| l.to_string());
        list.push(json!({
            "filename": name,
            "size_bytes": size,
            "created_at_ms": ts_ms,
            "label": label,
        }));
    }
    list.sort_by(|a, b| {
        b.get("created_at_ms")
            .and_then(Value::as_u64)
            .cmp(&a.get("created_at_ms").and_then(Value::as_u64))
    });
    json_response(StatusCode::OK, json!({ "snapshots": list }))
}

async fn restore_handler(
    State(st): State<AppState>,
    Extension(tc): Extension<TenantContext>,
    body: Bytes,
) -> Response {
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let snapshot = parsed
        .get("snapshot")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.contains('/') && !s.contains(".."));
    let Some(snapshot) = snapshot else {
        return json_error(StatusCode::BAD_REQUEST, "snapshot (filename) obrigatório");
    };

    match apply_restore(&tc.mem_dir, snapshot) {
        Ok((restored, backup_dir)) => {
            // Search.db restaurado é o arquivo do snapshot, mas o
            // SearchIndex em memória (st.search) ainda aponta pro
            // file handle antigo (SQLite mantém via inode). Após
            // restart o handle aponta pra novo arquivo; até lá, melhor
            // fazer um rebuild explícito pra garantir consistência
            // com os JSONLs recém-restaurados. JsonlRepository é
            // source-of-truth — sempre.
            let mut rebuild_status = "skipped (search index não inicializado)".to_string();
            if let Some(idx) = st.search.as_ref() {
                match crate::search_routes::rebuild_search_from_jsonls(&tc.mem_dir, idx) {
                    Ok(n) => rebuild_status = format!("reindexou {n} docs"),
                    Err(e) => rebuild_status = format!("rebuild falhou: {e}"),
                }
            }
            json_response(
                StatusCode::OK,
                json!({
                    "ok": true,
                    "restored_files": restored,
                    "backed_up_to": backup_dir.file_name().and_then(|s| s.to_str()),
                    "search_rebuild": rebuild_status,
                    "warning": "reinicie o servidor pra recarregar agents.json, scope.json, queue/state.json — config em memória ficou desatualizada",
                }),
            )
        }
        Err(RestoreError::SnapshotNotFound) => json_error(StatusCode::NOT_FOUND, "snapshot não encontrado"),
        Err(RestoreError::PathEscape(p)) => {
            json_error(StatusCode::BAD_REQUEST, &format!("path escape detectado: {p}"))
        }
        Err(RestoreError::ChecksumMismatch { path, expected, got }) => json_error(
            StatusCode::BAD_REQUEST,
            &format!("sha256 não bate pra {path}: esperado {expected}, got {got}"),
        ),
        Err(RestoreError::Manifest(e)) => {
            json_error(StatusCode::BAD_REQUEST, &format!("manifest inválido: {e}"))
        }
        Err(RestoreError::Io(e)) => {
            json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("io: {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_then_restore_roundtrips_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mem = tmp.path();
        // Cria estado mock.
        fs::write(mem.join("agents.json"), r#"{"a":1}"#).unwrap();
        fs::create_dir_all(mem.join("agent_chats")).unwrap();
        fs::write(mem.join("agent_chats/oracle.jsonl"), "line1\nline2\n").unwrap();

        let (snap_path, manifest, _size) = create_snapshot(mem, Some("test")).unwrap();
        assert!(snap_path.exists());
        assert!(manifest.files.iter().any(|f| f.path == "agents.json"));
        assert!(manifest.files.iter().any(|f| f.path.contains("oracle.jsonl")));

        // Modifica estado.
        fs::write(mem.join("agents.json"), r#"{"b":2}"#).unwrap();
        fs::write(mem.join("agent_chats/oracle.jsonl"), "modified\n").unwrap();

        // Restaura.
        let filename = snap_path.file_name().unwrap().to_str().unwrap();
        let (restored, backup_dir) = apply_restore(mem, filename).unwrap();
        assert!(restored >= 2);
        assert!(backup_dir.exists(), "backup dir deveria existir");

        // Conteúdo original restaurado.
        assert_eq!(fs::read_to_string(mem.join("agents.json")).unwrap(), r#"{"a":1}"#);
        assert_eq!(
            fs::read_to_string(mem.join("agent_chats/oracle.jsonl")).unwrap(),
            "line1\nline2\n"
        );
    }

    #[test]
    fn snapshot_skips_snapshots_dir_to_avoid_recursion() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mem = tmp.path();
        fs::write(mem.join("agents.json"), r#"{}"#).unwrap();
        // Cria snapshot 1.
        let (path1, _, _) = create_snapshot(mem, None).unwrap();
        // Cria snapshot 2 — não pode incluir snapshot 1 (=> não infinitamente cresce).
        std::thread::sleep(std::time::Duration::from_millis(2));
        let (_path2, manifest2, _) = create_snapshot(mem, None).unwrap();
        assert!(
            !manifest2.files.iter().any(|f| f.path.contains("snapshots/")),
            "snapshot 2 não pode conter snapshot 1: {:?}",
            manifest2.files.iter().map(|f| &f.path).collect::<Vec<_>>()
        );
        assert!(path1.exists());
    }
}
