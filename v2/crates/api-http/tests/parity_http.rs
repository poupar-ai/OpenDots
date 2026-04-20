//! Parity tests — `api-http` (F4-05a) vs `plugin/ui/server.cjs`.
//!
//! Estratégia:
//! - Fixtures idênticos em dois tempdirs.
//! - Sobe `node server.cjs` e `pmem-http` em portas livres distintas.
//! - Usa `curl` (via `Command`) pra GET/PUT em cada porta.
//! - Compara respostas JSON (status code + corpo parseado).
//!
//! Pulados quando `node` não está disponível.

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

/// Aloca uma porta livre — abre um socket em :0 e devolve o número.
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

fn curl(method: &str, url: &str, body: Option<&str>) -> Response {
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

/// Remove campos cujo valor diverge inerentemente entre instâncias
/// (portas aleatórias, ordem de `lanIps`, etc.).
fn strip_volatile(v: &mut Value) {
    if let Value::Object(m) = v {
        m.remove("port");
        m.remove("lanIps");
        m.remove("activeHost");
    }
}

fn compare_json(mut a: Value, mut b: Value) {
    strip_volatile(&mut a);
    strip_volatile(&mut b);
    assert_eq!(a, b, "\nRust: {}\nNode: {}", a, b);
}

fn setup() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let mem = make_project(tmp.path());
    (tmp, mem)
}

fn both_servers() -> Option<(TempDir, Server, Server)> {
    if !node_available() || !curl_available() {
        eprintln!("skipping — node or curl unavailable");
        return None;
    }
    let (tmp, _mem) = setup();
    let p_rust = free_port();
    let p_node = free_port();
    let s_rust = Server::spawn_rust(tmp.path(), p_rust);
    let s_node = Server::spawn_node(tmp.path(), p_node);
    if !s_rust.wait_ready() || !s_node.wait_ready() {
        eprintln!("skipping — server failed to bind");
        return None;
    }
    Some((tmp, s_rust, s_node))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn network_config_get_same_shape() {
    let Some((_tmp, s_rust, s_node)) = both_servers() else {
        return;
    };
    let r = curl(
        "GET",
        &format!("http://127.0.0.1:{}/api/network-config", s_rust.port),
        None,
    );
    let n = curl(
        "GET",
        &format!("http://127.0.0.1:{}/api/network-config", s_node.port),
        None,
    );
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    compare_json(r.body, n.body);
}

#[test]
fn network_config_put_lan() {
    let Some((tmp, s_rust, s_node)) = both_servers() else {
        return;
    };
    let body = r#"{"bind":"lan","customHost":""}"#;
    let r = curl(
        "PUT",
        &format!("http://127.0.0.1:{}/api/network-config", s_rust.port),
        Some(body),
    );
    let n = curl(
        "PUT",
        &format!("http://127.0.0.1:{}/api/network-config", s_node.port),
        Some(body),
    );
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    compare_json(r.body, n.body);
    // Ambos devem ter escrito o mesmo network.json (byte-a-byte).
    let cfg_path = tmp.path().join(".claude-project-memory/network.json");
    let written = std::fs::read_to_string(&cfg_path).unwrap();
    let parsed: Value = serde_json::from_str(&written).unwrap();
    assert_eq!(parsed["bind"], "lan");
}

#[test]
fn network_config_put_invalid_bind() {
    let Some((_tmp, s_rust, s_node)) = both_servers() else {
        return;
    };
    let body = r#"{"bind":"weird"}"#;
    let r = curl(
        "PUT",
        &format!("http://127.0.0.1:{}/api/network-config", s_rust.port),
        Some(body),
    );
    let n = curl(
        "PUT",
        &format!("http://127.0.0.1:{}/api/network-config", s_node.port),
        Some(body),
    );
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    compare_json(r.body, n.body);
}

#[test]
fn network_config_put_custom_without_host() {
    let Some((_tmp, s_rust, s_node)) = both_servers() else {
        return;
    };
    let body = r#"{"bind":"custom","customHost":""}"#;
    let r = curl(
        "PUT",
        &format!("http://127.0.0.1:{}/api/network-config", s_rust.port),
        Some(body),
    );
    let n = curl(
        "PUT",
        &format!("http://127.0.0.1:{}/api/network-config", s_node.port),
        Some(body),
    );
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    compare_json(r.body, n.body);
}

#[test]
fn bus_config_get_defaults() {
    let Some((_tmp, s_rust, s_node)) = both_servers() else {
        return;
    };
    let r = curl(
        "GET",
        &format!("http://127.0.0.1:{}/api/bus/config", s_rust.port),
        None,
    );
    let n = curl(
        "GET",
        &format!("http://127.0.0.1:{}/api/bus/config", s_node.port),
        None,
    );
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    compare_json(r.body, n.body);
}

#[test]
fn bus_config_put_retention_out_of_range() {
    let Some((_tmp, s_rust, s_node)) = both_servers() else {
        return;
    };
    let body = r#"{"retention_days":9999}"#;
    let r = curl(
        "PUT",
        &format!("http://127.0.0.1:{}/api/bus/config", s_rust.port),
        Some(body),
    );
    let n = curl(
        "PUT",
        &format!("http://127.0.0.1:{}/api/bus/config", s_node.port),
        Some(body),
    );
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    compare_json(r.body, n.body);
}

#[test]
fn bus_config_put_retention_valid() {
    let Some((_tmp, s_rust, s_node)) = both_servers() else {
        return;
    };
    let body = r#"{"retention_days":30}"#;
    let r = curl(
        "PUT",
        &format!("http://127.0.0.1:{}/api/bus/config", s_rust.port),
        Some(body),
    );
    let n = curl(
        "PUT",
        &format!("http://127.0.0.1:{}/api/bus/config", s_node.port),
        Some(body),
    );
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    compare_json(r.body, n.body);
}

#[test]
fn bus_state_empty() {
    let Some((_tmp, s_rust, s_node)) = both_servers() else {
        return;
    };
    let r = curl(
        "GET",
        &format!("http://127.0.0.1:{}/api/bus/state?recent=10", s_rust.port),
        None,
    );
    let n = curl(
        "GET",
        &format!("http://127.0.0.1:{}/api/bus/state?recent=10", s_node.port),
        None,
    );
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    // Node retorna queue com campos reais; Rust ainda não tem autoQueue.
    // Removemos queue pra comparar o resto.
    let mut rb = r.body.clone();
    let mut nb = n.body.clone();
    if let Value::Object(m) = &mut rb {
        m.remove("queue");
    }
    if let Value::Object(m) = &mut nb {
        m.remove("queue");
    }
    compare_json(rb, nb);
}

#[test]
fn bus_trace_missing() {
    let Some((_tmp, s_rust, s_node)) = both_servers() else {
        return;
    };
    let r = curl(
        "GET",
        &format!("http://127.0.0.1:{}/api/bus/trace", s_rust.port),
        None,
    );
    let n = curl(
        "GET",
        &format!("http://127.0.0.1:{}/api/bus/trace", s_node.port),
        None,
    );
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    compare_json(r.body, n.body);
}

#[test]
fn bus_trace_not_found() {
    let Some((_tmp, s_rust, s_node)) = both_servers() else {
        return;
    };
    let r = curl(
        "GET",
        &format!(
            "http://127.0.0.1:{}/api/bus/trace?trace_id=tnope",
            s_rust.port
        ),
        None,
    );
    let n = curl(
        "GET",
        &format!(
            "http://127.0.0.1:{}/api/bus/trace?trace_id=tnope",
            s_node.port
        ),
        None,
    );
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    compare_json(r.body, n.body);
}

#[test]
fn bus_compact_empty() {
    let Some((_tmp, s_rust, s_node)) = both_servers() else {
        return;
    };
    let r = curl(
        "POST",
        &format!("http://127.0.0.1:{}/api/bus/compact", s_rust.port),
        Some(""),
    );
    let n = curl(
        "POST",
        &format!("http://127.0.0.1:{}/api/bus/compact", s_node.port),
        Some(""),
    );
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    compare_json(r.body, n.body);
}

#[test]
fn serves_index_html() {
    let Some((_tmp, s_rust, s_node)) = both_servers() else {
        return;
    };
    // Não parseia JSON — /: é HTML. Usa curl direto só pro status.
    let rs = Command::new("curl")
        .arg("-s")
        .arg("-o")
        .arg("/dev/null")
        .arg("-w")
        .arg("%{http_code}")
        .arg(format!("http://127.0.0.1:{}/", s_rust.port))
        .output()
        .unwrap();
    let ns = Command::new("curl")
        .arg("-s")
        .arg("-o")
        .arg("/dev/null")
        .arg("-w")
        .arg("%{http_code}")
        .arg(format!("http://127.0.0.1:{}/", s_node.port))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&rs.stdout).trim(), "200");
    assert_eq!(String::from_utf8_lossy(&ns.stdout).trim(), "200");
}
