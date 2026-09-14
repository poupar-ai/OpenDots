//! `pmem` — binário CLI que replica `scripts/{install,uninstall,status}.cjs`.
//!
//! Thin main: parse argumentos com clap, chama funções puras do
//! `api_cli`, escreve stdout/stderr e escolhe exit code. Toda decisão
//! testável mora na lib.

use std::path::PathBuf;
use std::process::ExitCode;

use api_cli::{
    apply_install, apply_uninstall, describe_status, read_settings, settings_path, write_settings,
    InstallConfig, McpLauncher, StatusReport,
};
use clap::{Parser, Subcommand};
use serde_json::Value;

#[derive(Parser)]
#[command(name = "pmem", version, about = "project-mem CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Registra o plugin em ~/.claude/settings.json
    Install {
        /// Diretório onde o plugin está instalado (`installDir` no .cjs)
        install_dir: PathBuf,
        /// Caminho para o binário do node (substitui `process.execPath`
        /// do .cjs). Default: valor de $NODE_BIN, senão "node" (PATH).
        #[arg(long)]
        node_bin: Option<PathBuf>,
    },
    /// Remove os registros do plugin
    Uninstall,
    /// Mostra se o plugin está instalado
    Status,
}

fn main() -> ExitCode {
    match Cli::parse().cmd {
        Cmd::Install {
            install_dir,
            node_bin,
        } => run_install(install_dir, node_bin),
        Cmd::Uninstall => run_uninstall(),
        Cmd::Status => run_status(),
    }
}

fn run_install(install_dir: PathBuf, node_bin: Option<PathBuf>) -> ExitCode {
    let path = match settings_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };

    let mut settings = match read_settings(&path) {
        Ok(Some(v)) => v,
        Ok(None) => Value::Object(Default::default()),
        Err(api_cli::Error::Parse { path, source }) => {
            eprintln!("Erro ao ler {path}: {source}");
            eprintln!("Verifique se o arquivo é um JSON válido antes de continuar.");
            return ExitCode::from(1);
        }
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };

    let node_bin = node_bin
        .or_else(|| std::env::var_os("NODE_BIN").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("node"));

    let mcp = McpLauncher::detect(&install_dir, node_bin);
    let mcp_label = match &mcp {
        McpLauncher::Node { .. } => "node",
        McpLauncher::Native => "nativo",
    };

    apply_install(
        &mut settings,
        &InstallConfig {
            install_dir: install_dir.clone(),
            mcp,
        },
    );

    if let Err(e) = write_settings(&path, &settings) {
        eprintln!("{e}");
        return ExitCode::from(1);
    }

    println!("✓ settings.json atualizado: {}", path.display());
    println!("✓ Plugin registrado em: {}", install_dir.display());
    println!("✓ MCP server registrado: project-mem ({mcp_label})");
    ExitCode::SUCCESS
}

fn run_uninstall() -> ExitCode {
    let path = match settings_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };

    let mut settings = match read_settings(&path) {
        Ok(Some(v)) => v,
        Ok(None) => {
            println!("settings.json não encontrado — nada a fazer.");
            return ExitCode::SUCCESS;
        }
        Err(api_cli::Error::Parse { path, source }) => {
            eprintln!("Erro ao ler {path}: {source}");
            return ExitCode::from(1);
        }
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };

    if !apply_uninstall(&mut settings) {
        println!("project-man não estava registrado — nada a fazer.");
        return ExitCode::SUCCESS;
    }

    if let Err(e) = write_settings(&path, &settings) {
        eprintln!("{e}");
        return ExitCode::from(1);
    }

    println!("✓ project-man removido de: {}", path.display());
    ExitCode::SUCCESS
}

fn run_status() -> ExitCode {
    let path = match settings_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };

    let report = match read_settings(&path) {
        Ok(Some(v)) => describe_status(&v),
        Ok(None) => StatusReport::SettingsMissing,
        Err(api_cli::Error::Parse { .. }) => StatusReport::SettingsInvalid,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };

    print!("{}", report.render(&path));
    // Paridade com .cjs: `status` sai 0 em missing/incomplete também
    // ("log + process.exit(0)"). Só sai 1 em parse-invalid.
    if matches!(report, StatusReport::SettingsInvalid) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
