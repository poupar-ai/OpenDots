//! Cross-language parity: `api-cli` (Rust) ↔ `scripts/{install,uninstall,status}.cjs`.
//!
//! Foco do port: o arquivo `settings.json` gerado precisa ser byte-idêntico
//! ao que o `.cjs` geraria no mesmo input. Output de tela das mensagens
//! humanas é coberto nos unit tests (`render`).
//!
//! Cenários:
//! - install em `settings.json` ausente: bytes iguais
//! - install sobre settings existente com chaves não-relacionadas: bytes iguais
//! - uninstall após install: bytes iguais (idealmente volta ao fixture)
//! - uninstall em `settings.json` ausente: mesmo comportamento (0 changes)
//! - install via binário `pmem` compilado: bytes iguais vs Node

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use api_cli::{
    apply_install, apply_uninstall, read_settings, write_settings, InstallConfig, McpLauncher,
};
use serde_json::Value;
use tempfile::TempDir;

// ─── Helpers ─────────────────────────────────────────────────────────

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("locate repo root")
}

fn install_cjs() -> PathBuf {
    repo_root().join("scripts/install.cjs")
}

fn uninstall_cjs() -> PathBuf {
    repo_root().join("scripts/uninstall.cjs")
}

fn pmem_bin() -> PathBuf {
    // target/debug/pmem relativo à workspace. `cargo test` já compilou.
    repo_root().join("v2/target/debug/pmem")
}

/// Caminho absoluto do node que está disponível no ambiente — usado como
/// `node_bin` no Rust pra replicar `process.execPath` do .cjs byte-a-byte.
fn node_exec_path() -> PathBuf {
    let out = Command::new("node")
        .args(["-e", "process.stdout.write(process.execPath)"])
        .output()
        .expect("run node");
    assert!(out.status.success(), "node probe failed");
    PathBuf::from(String::from_utf8(out.stdout).expect("utf8"))
}

fn run_node_script(script: &Path, args: &[&str], config_dir: &Path) {
    let status = Command::new("node")
        .arg(script)
        .args(args)
        .env("CLAUDE_CONFIG_DIR", config_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .expect("spawn node");
    assert!(status.success(), "node script {script:?} failed");
}

fn tmp() -> TempDir {
    TempDir::new().expect("tempdir")
}

fn settings_file(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

// ─── Install parity ──────────────────────────────────────────────────

#[test]
fn parity_install_empty_settings_bytes_identical() {
    let cfg_node = tmp();
    let cfg_rust = tmp();
    let install_dir = PathBuf::from("/opt/project-mem");
    let node_bin = node_exec_path();

    // Node
    run_node_script(
        &install_cjs(),
        &[install_dir.to_str().unwrap()],
        cfg_node.path(),
    );
    let node_bytes = std::fs::read(settings_file(cfg_node.path())).unwrap();

    // Rust
    let path = settings_file(cfg_rust.path());
    let mut s = Value::Object(Default::default());
    apply_install(
        &mut s,
        &InstallConfig {
            install_dir,
            mcp: McpLauncher::Node { node_bin },
        },
    );
    write_settings(&path, &s).unwrap();
    let rust_bytes = std::fs::read(&path).unwrap();

    assert_eq!(
        String::from_utf8_lossy(&node_bytes),
        String::from_utf8_lossy(&rust_bytes)
    );
}

#[test]
fn parity_install_preserves_user_keys() {
    let cfg_node = tmp();
    let cfg_rust = tmp();
    let install_dir = PathBuf::from("/srv/pm");
    let node_bin = node_exec_path();

    // Mesmo fixture inicial para os dois: settings com chaves não-relacionadas
    let fixture = r#"{
  "theme": "dark",
  "enabledPlugins": {
    "other@ns": true
  },
  "mcpServers": {
    "existing": {
      "type": "stdio",
      "command": "/usr/bin/node"
    }
  }
}
"#;
    std::fs::write(settings_file(cfg_node.path()), fixture).unwrap();
    std::fs::write(settings_file(cfg_rust.path()), fixture).unwrap();

    // Node
    run_node_script(
        &install_cjs(),
        &[install_dir.to_str().unwrap()],
        cfg_node.path(),
    );
    let node_bytes = std::fs::read(settings_file(cfg_node.path())).unwrap();

    // Rust
    let path = settings_file(cfg_rust.path());
    let mut s = read_settings(&path).unwrap().unwrap();
    apply_install(
        &mut s,
        &InstallConfig {
            install_dir,
            mcp: McpLauncher::Node { node_bin },
        },
    );
    write_settings(&path, &s).unwrap();
    let rust_bytes = std::fs::read(&path).unwrap();

    assert_eq!(
        String::from_utf8_lossy(&node_bytes),
        String::from_utf8_lossy(&rust_bytes)
    );
}

// ─── Uninstall parity ────────────────────────────────────────────────

#[test]
fn parity_uninstall_after_install_bytes_identical() {
    let cfg_node = tmp();
    let cfg_rust = tmp();
    let install_dir = PathBuf::from("/opt/pm");
    let node_bin = node_exec_path();

    // Fixture: já tem uma chave não-relacionada que deve sobreviver ao ciclo.
    let fixture = "{\n  \"theme\": \"dark\"\n}\n";
    std::fs::write(settings_file(cfg_node.path()), fixture).unwrap();
    std::fs::write(settings_file(cfg_rust.path()), fixture).unwrap();

    // Node: install + uninstall
    run_node_script(
        &install_cjs(),
        &[install_dir.to_str().unwrap()],
        cfg_node.path(),
    );
    run_node_script(&uninstall_cjs(), &[], cfg_node.path());
    let node_bytes = std::fs::read(settings_file(cfg_node.path())).unwrap();

    // Rust: install + uninstall
    let path = settings_file(cfg_rust.path());
    let mut s = read_settings(&path).unwrap().unwrap();
    apply_install(
        &mut s,
        &InstallConfig {
            install_dir,
            mcp: McpLauncher::Node { node_bin },
        },
    );
    write_settings(&path, &s).unwrap();
    let mut s = read_settings(&path).unwrap().unwrap();
    apply_uninstall(&mut s);
    write_settings(&path, &s).unwrap();
    let rust_bytes = std::fs::read(&path).unwrap();

    assert_eq!(
        String::from_utf8_lossy(&node_bytes),
        String::from_utf8_lossy(&rust_bytes)
    );
}

#[test]
fn parity_uninstall_missing_settings_is_noop() {
    // settings.json não existe → tanto Node quanto Rust saem sem
    // alterar nada. Testamos observando que nenhum arquivo é criado.
    let cfg_node = tmp();
    let cfg_rust = tmp();

    run_node_script(&uninstall_cjs(), &[], cfg_node.path());
    assert!(!settings_file(cfg_node.path()).exists());

    let path = settings_file(cfg_rust.path());
    assert!(read_settings(&path).unwrap().is_none());
    assert!(!path.exists());
}

// ─── pmem binary parity ──────────────────────────────────────────────

#[test]
fn parity_pmem_bin_install_matches_node() {
    if !pmem_bin().exists() {
        // Em CI o binário é compilado antes; localmente `cargo test`
        // compila lib e binários do crate alvo. Se por algum motivo não
        // existir, pulamos sem falhar — unit + install_fn parity cobrem
        // a mesma lógica.
        eprintln!("pmem bin not built, skipping e2e");
        return;
    }
    let cfg_node = tmp();
    let cfg_rust = tmp();
    let install_dir = PathBuf::from("/opt/pm");
    let node_bin = node_exec_path();

    // Node via .cjs
    run_node_script(
        &install_cjs(),
        &[install_dir.to_str().unwrap()],
        cfg_node.path(),
    );

    // Rust via binário pmem
    let status = Command::new(pmem_bin())
        .args(["install", install_dir.to_str().unwrap()])
        .args(["--node-bin", node_bin.to_str().unwrap()])
        .env("CLAUDE_CONFIG_DIR", cfg_rust.path())
        .env_remove("NODE_BIN")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .expect("run pmem");
    assert!(status.success());

    let node_bytes = std::fs::read(settings_file(cfg_node.path())).unwrap();
    let rust_bytes = std::fs::read(settings_file(cfg_rust.path())).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&node_bytes),
        String::from_utf8_lossy(&rust_bytes)
    );
}
