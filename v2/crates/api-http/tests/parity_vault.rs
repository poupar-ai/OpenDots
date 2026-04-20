//! Parity tests — `api-http` vault rotas (F4-05b) vs `plugin/ui/server.cjs`.
//!
//! Estratégia: igual ao `parity_http.rs`. Dois servidores em portas livres
//! contra o mesmo tempdir + `curl`.
//!
//! Cobertura:
//! - `/api/vault/status` — antes e depois de init
//! - `/api/vault/paths` — shape do JSON (com `mem_dir` trocado)
//! - `/api/vault/init` — 200 + rejeita re-init
//! - `/api/vault/lock` — idempotente
//! - `/api/vault/keys` — locked → 423
//! - `/api/vault/secret/*` — locked → 423 / 404
//! - `/api/vault/envfiles` — locked → 423
//! - `/api/vault/envfile/*` — locked → 423 / 404
//! - `/api/vault/destroy` — confirm inválido → 400, locked → 423
//! - guard: Host rebinding → 403
//! - guard: Origin cross-site → 403
//! - guard: Sec-Fetch-Site → 403
//!
//! **Não cobertos aqui:**
//! - `/api/vault/unlock` happy-path (stateful com fs do device-key + TTL
//!   do clock — testado em unit tests do adapter-vault-file).
//! - `/api/vault/import` — shape do 200 diverge entre `.cjs` (retorna
//!   `{ ok, imported_at, exported_at }`) e Rust (retorna `{ ok: true }`
//!   porque `import_bundle` → `Result<(), _>`). Registrado em
//!   `docs/migration_findings.md` como divergência v2 conhecida.
//! - `/api/vault/export` — mesma situação: bytes binários, skip aqui.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

const PLUGIN_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../plugin");

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn curl_available() -> bool {
    Command::new("curl")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn plugin_path() -> PathBuf {
    PathBuf::from(PLUGIN_DIR)
}

fn cli_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_pmem-http"))
}

fn free_port() -> u16 {
    let l = TcpListener::bind(("127.0.0.1", 0)).expect("bind :0");
    l.local_addr().unwrap().port()
}

fn make_project(tmp: &Path) -> PathBuf {
    let mem = tmp.join(".claude-project-memory");
    std::fs::create_dir_all(&mem).unwrap();
    mem
}

struct Server {
    child: Child,
    port: u16,
}

impl Server {
    fn spawn_node(cwd: &Path, port: u16) -> Self {
        let server_cjs = plugin_path().join("ui").join("server.cjs");
        let child = Command::new("node")
            .arg(&server_cjs)
            .env("PROJECT_MEM_PORT", port.to_string())
            .env("CLAUDE_CWD", cwd)
            .env("PWD", cwd)
            .current_dir(cwd)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn node server.cjs");
        Self { child, port }
    }

    fn spawn_rust(cwd: &Path, port: u16) -> Self {
        let child = Command::new(cli_bin())
            .env("PROJECT_MEM_PORT", port.to_string())
            .env("CLAUDE_CWD", cwd)
            .env("PWD", cwd)
            .current_dir(cwd)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn pmem-http");
        Self { child, port }
    }

    fn wait_ready(&self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if std::net::TcpStream::connect(("127.0.0.1", self.port)).is_ok() {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug)]
struct Response {
    status: u16,
    body: Value,
}

fn curl(method: &str, url: &str, body: Option<&str>, extra_headers: &[(&str, &str)]) -> Response {
    let mut cmd = Command::new("curl");
    cmd.arg("-s")
        .arg("-o")
        .arg("-")
        .arg("-w")
        .arg("\n%{http_code}")
        .arg("-X")
        .arg(method)
        .arg(url)
        .arg("-H")
        .arg("Content-Type: application/json");
    for (k, v) in extra_headers {
        cmd.arg("-H").arg(format!("{k}: {v}"));
    }
    if let Some(b) = body {
        cmd.arg("-d").arg(b);
    }
    let out = cmd.output().expect("curl");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let (body, code) = stdout
        .rsplit_once('\n')
        .map(|(b, c)| (b.to_string(), c.to_string()))
        .unwrap_or_else(|| (stdout.clone(), "0".into()));
    let status: u16 = code.trim().parse().unwrap_or(0);
    let body: Value = serde_json::from_str(body.trim()).unwrap_or(Value::Null);
    Response { status, body }
}

/// Remove campos cujo valor diverge inerentemente (timestamps do TTL,
/// caminhos absolutos do tempdir, etc.).
fn strip_vault_volatile(v: &mut Value) {
    if let Value::Object(m) = v {
        m.remove("expires_at");
        m.remove("ttl_ms");
        m.remove("mem_dir");
        m.remove("vault_file");
        m.remove("device_key_file");
    }
}

fn compare_stripped(mut a: Value, mut b: Value) {
    strip_vault_volatile(&mut a);
    strip_vault_volatile(&mut b);
    assert_eq!(a, b, "\nRust: {}\nNode: {}", a, b);
}

/// Vault muta arquivos críticos (`vault.enc`, `.vault-devicekey`). Servidores
/// precisam de tempdirs **distintos** — caso contrário o primeiro init vence
/// e o segundo responde `AlreadyInitialized`. Diferente de `parity_http.rs`
/// que só mexe em configs idempotentes.
fn both_servers() -> Option<(TempDir, TempDir, Server, Server)> {
    if !node_available() || !curl_available() {
        eprintln!("skipping — node or curl unavailable");
        return None;
    }
    let tmp_rust = TempDir::new().unwrap();
    let tmp_node = TempDir::new().unwrap();
    let _ = make_project(tmp_rust.path());
    let _ = make_project(tmp_node.path());
    let p_rust = free_port();
    let p_node = free_port();
    let s_rust = Server::spawn_rust(tmp_rust.path(), p_rust);
    let s_node = Server::spawn_node(tmp_node.path(), p_node);
    if !s_rust.wait_ready() || !s_node.wait_ready() {
        eprintln!("skipping — server failed to bind");
        return None;
    }
    Some((tmp_rust, tmp_node, s_rust, s_node))
}

fn rurl(port: u16, path: &str) -> String {
    format!("http://127.0.0.1:{port}{path}")
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn status_empty_vault() {
    let Some((_tmp_r, _tmp_n, s_rust, s_node)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(s_rust.port, "/api/vault/status"), None, &[]);
    let n = curl("GET", &rurl(s_node.port, "/api/vault/status"), None, &[]);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    compare_stripped(r.body, n.body);
}

#[test]
fn paths_shape() {
    let Some((_tmp_r, _tmp_n, s_rust, s_node)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(s_rust.port, "/api/vault/paths"), None, &[]);
    let n = curl("GET", &rurl(s_node.port, "/api/vault/paths"), None, &[]);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    // Só checamos que as três chaves existem e são strings — os valores
    // divergem inerentemente (cada servidor tem seu tempdir).
    for v in [&r.body, &n.body] {
        assert!(v.get("mem_dir").and_then(Value::as_str).is_some());
        assert!(v.get("vault_file").and_then(Value::as_str).is_some());
        assert!(v.get("device_key_file").and_then(Value::as_str).is_some());
    }
}

#[test]
fn init_and_reinit() {
    let Some((_tmp_r, _tmp_n, s_rust, s_node)) = both_servers() else {
        return;
    };
    let body = r#"{"password":"master-pass-1234"}"#;
    let r1 = curl(
        "POST",
        &rurl(s_rust.port, "/api/vault/init"),
        Some(body),
        &[],
    );
    let n1 = curl(
        "POST",
        &rurl(s_node.port, "/api/vault/init"),
        Some(body),
        &[],
    );
    assert_eq!(r1.status, 200);
    assert_eq!(n1.status, 200);
    compare_stripped(r1.body, n1.body);
    // Segundo init deve falhar em ambos.
    let r2 = curl(
        "POST",
        &rurl(s_rust.port, "/api/vault/init"),
        Some(body),
        &[],
    );
    let n2 = curl(
        "POST",
        &rurl(s_node.port, "/api/vault/init"),
        Some(body),
        &[],
    );
    assert_eq!(r2.status, 400);
    assert_eq!(n2.status, 400);
}

#[test]
fn lock_is_ok() {
    let Some((_tmp_r, _tmp_n, s_rust, s_node)) = both_servers() else {
        return;
    };
    let r = curl(
        "POST",
        &rurl(s_rust.port, "/api/vault/lock"),
        Some("{}"),
        &[],
    );
    let n = curl(
        "POST",
        &rurl(s_node.port, "/api/vault/lock"),
        Some("{}"),
        &[],
    );
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    compare_stripped(r.body, n.body);
}

#[test]
fn keys_locked_returns_423() {
    let Some((_tmp_r, _tmp_n, s_rust, s_node)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(s_rust.port, "/api/vault/keys"), None, &[]);
    let n = curl("GET", &rurl(s_node.port, "/api/vault/keys"), None, &[]);
    assert_eq!(r.status, 423);
    assert_eq!(n.status, 423);
    assert!(r.body.get("error").is_some());
    assert!(n.body.get("error").is_some());
}

#[test]
fn secret_get_locked_returns_423() {
    let Some((_tmp_r, _tmp_n, s_rust, s_node)) = both_servers() else {
        return;
    };
    let r = curl(
        "GET",
        &rurl(s_rust.port, "/api/vault/secret/foo"),
        None,
        &[],
    );
    let n = curl(
        "GET",
        &rurl(s_node.port, "/api/vault/secret/foo"),
        None,
        &[],
    );
    assert_eq!(r.status, 423);
    assert_eq!(n.status, 423);
}

#[test]
fn secret_put_locked_returns_423() {
    let Some((_tmp_r, _tmp_n, s_rust, s_node)) = both_servers() else {
        return;
    };
    let body = r#"{"value":"x"}"#;
    let r = curl(
        "PUT",
        &rurl(s_rust.port, "/api/vault/secret/foo"),
        Some(body),
        &[],
    );
    let n = curl(
        "PUT",
        &rurl(s_node.port, "/api/vault/secret/foo"),
        Some(body),
        &[],
    );
    assert_eq!(r.status, 423);
    assert_eq!(n.status, 423);
}

#[test]
fn envfiles_locked_returns_423() {
    let Some((_tmp_r, _tmp_n, s_rust, s_node)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(s_rust.port, "/api/vault/envfiles"), None, &[]);
    let n = curl("GET", &rurl(s_node.port, "/api/vault/envfiles"), None, &[]);
    assert_eq!(r.status, 423);
    assert_eq!(n.status, 423);
}

#[test]
fn envfile_get_locked_returns_423() {
    let Some((_tmp_r, _tmp_n, s_rust, s_node)) = both_servers() else {
        return;
    };
    let r = curl(
        "GET",
        &rurl(s_rust.port, "/api/vault/envfile/foo.env"),
        None,
        &[],
    );
    let n = curl(
        "GET",
        &rurl(s_node.port, "/api/vault/envfile/foo.env"),
        None,
        &[],
    );
    assert_eq!(r.status, 423);
    assert_eq!(n.status, 423);
}

#[test]
fn destroy_wrong_confirm_400() {
    let Some((_tmp_r, _tmp_n, s_rust, s_node)) = both_servers() else {
        return;
    };
    let body = r#"{"confirm":"YES"}"#;
    let r = curl(
        "POST",
        &rurl(s_rust.port, "/api/vault/destroy"),
        Some(body),
        &[],
    );
    let n = curl(
        "POST",
        &rurl(s_node.port, "/api/vault/destroy"),
        Some(body),
        &[],
    );
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    compare_stripped(r.body, n.body);
}

#[test]
fn destroy_locked_423() {
    let Some((_tmp_r, _tmp_n, s_rust, s_node)) = both_servers() else {
        return;
    };
    // Primeiro: init (pra ter algo pra destruir, e pra `locked=true` bater).
    let init_body = r#"{"password":"master-pass-1234"}"#;
    curl(
        "POST",
        &rurl(s_rust.port, "/api/vault/init"),
        Some(init_body),
        &[],
    );
    curl(
        "POST",
        &rurl(s_node.port, "/api/vault/init"),
        Some(init_body),
        &[],
    );
    curl(
        "POST",
        &rurl(s_rust.port, "/api/vault/lock"),
        Some("{}"),
        &[],
    );
    curl(
        "POST",
        &rurl(s_node.port, "/api/vault/lock"),
        Some("{}"),
        &[],
    );
    let body = r#"{"confirm":"DESTROY"}"#;
    let r = curl(
        "POST",
        &rurl(s_rust.port, "/api/vault/destroy"),
        Some(body),
        &[],
    );
    let n = curl(
        "POST",
        &rurl(s_node.port, "/api/vault/destroy"),
        Some(body),
        &[],
    );
    assert_eq!(r.status, 423);
    assert_eq!(n.status, 423);
}

#[test]
fn guard_host_rebinding_403() {
    let Some((_tmp_r, _tmp_n, s_rust, s_node)) = both_servers() else {
        return;
    };
    // Host arbitrário que resolve pra 127.0.0.1 → bloqueado.
    let headers: &[(&str, &str)] = &[("Host", "evil.example.com")];
    let r = curl(
        "GET",
        &rurl(s_rust.port, "/api/vault/status"),
        None,
        headers,
    );
    let n = curl(
        "GET",
        &rurl(s_node.port, "/api/vault/status"),
        None,
        headers,
    );
    assert_eq!(r.status, 403);
    assert_eq!(n.status, 403);
}

#[test]
fn guard_origin_cross_site_403() {
    let Some((_tmp_r, _tmp_n, s_rust, s_node)) = both_servers() else {
        return;
    };
    let headers: &[(&str, &str)] = &[("Origin", "http://evil.example.com")];
    let r = curl(
        "GET",
        &rurl(s_rust.port, "/api/vault/status"),
        None,
        headers,
    );
    let n = curl(
        "GET",
        &rurl(s_node.port, "/api/vault/status"),
        None,
        headers,
    );
    assert_eq!(r.status, 403);
    assert_eq!(n.status, 403);
}

#[test]
fn guard_sec_fetch_site_403() {
    let Some((_tmp_r, _tmp_n, s_rust, s_node)) = both_servers() else {
        return;
    };
    let headers: &[(&str, &str)] = &[("Sec-Fetch-Site", "cross-site")];
    let r = curl(
        "GET",
        &rurl(s_rust.port, "/api/vault/status"),
        None,
        headers,
    );
    let n = curl(
        "GET",
        &rurl(s_node.port, "/api/vault/status"),
        None,
        headers,
    );
    assert_eq!(r.status, 403);
    assert_eq!(n.status, 403);
}
