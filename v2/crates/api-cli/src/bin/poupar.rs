//! `poupar` — binário CLI que replica `scripts/{install,uninstall,status}.cjs`.
//!
//! Thin main: parse argumentos com clap, chama funções puras do
//! `api_cli`, escreve stdout/stderr e escolhe exit code. Toda decisão
//! testável mora na lib.

use std::path::PathBuf;
use std::process::ExitCode;

use api_cli::{
    apply_install, apply_uninstall, describe_status, read_settings, settings_path, write_settings,
    InstallConfig, StatusReport,
};
use clap::{Parser, Subcommand};
use serde_json::Value;

#[derive(Parser)]
#[command(name = "poupar", version, about = "open-poupar CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Registra o plugin em ~/.claude/settings.json
    Install {
        /// Diretório onde o plugin está instalado
        install_dir: PathBuf,
    },
    /// Remove os registros do plugin
    Uninstall,
    /// Mostra se o plugin está instalado
    Status,
    /// Rebuild do code index do projeto (mapa de símbolos reutilizáveis)
    Index {
        /// Só imprime o CODE_INDEX.md atual, sem reconstruir
        #[arg(long)]
        show: bool,
    },
}

fn main() -> ExitCode {
    match Cli::parse().cmd {
        Cmd::Install { install_dir } => run_install(install_dir),
        Cmd::Uninstall => run_uninstall(),
        Cmd::Status => run_status(),
        Cmd::Index { show } => run_index(show),
    }
}

fn run_install(install_dir: PathBuf) -> ExitCode {
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

    apply_install(
        &mut settings,
        &InstallConfig {
            install_dir: install_dir.clone(),
        },
    );

    if let Err(e) = write_settings(&path, &settings) {
        eprintln!("{e}");
        return ExitCode::from(1);
    }

    println!("✓ settings.json atualizado: {}", path.display());
    println!("✓ Plugin registrado em: {}", install_dir.display());
    println!("✓ MCP server registrado: open-poupar");
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
        println!("open-poupar não estava registrado — nada a fazer.");
        return ExitCode::SUCCESS;
    }

    if let Err(e) = write_settings(&path, &settings) {
        eprintln!("{e}");
        return ExitCode::from(1);
    }

    println!("✓ open-poupar removido de: {}", path.display());
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

/// `poupar index` — rebuild completo do code index.
///
/// Existe como subcomando porque o `/mem-index` precisa de um caminho
/// sem Node, e a rota HTTP equivalente não existe: o indexador só era
/// alcançável pelo hook de fim de sessão (incremental) e pelo `.cjs`.
///
/// `mem_dir` sai do mesmo lugar que os hooks usam — `<git toplevel>/.poupar`
/// a partir do cwd — pra que o índice caia junto do resto da memória.
fn run_index(show: bool) -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_root = core_paths::find_project_root(&cwd);
    let mem_dir = project_root.join(core_paths::MEM_DIR_NAME);

    if show {
        let md = mem_dir.join(domain_indexer::INDEX_MD);
        return match std::fs::read_to_string(&md) {
            Ok(c) => {
                print!("{c}");
                ExitCode::SUCCESS
            }
            Err(_) => {
                eprintln!("sem índice ainda em {} — rode `poupar index`", md.display());
                ExitCode::from(1)
            }
        };
    }

    if let Err(e) = std::fs::create_dir_all(&mem_dir) {
        eprintln!("não consegui criar {}: {e}", mem_dir.display());
        return ExitCode::from(1);
    }

    let index = match domain_indexer::rebuild_index(
        &mem_dir,
        &project_root,
        &domain_indexer::SystemClock,
    ) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("falha no rebuild: {e}");
            return ExitCode::from(1);
        }
    };
    if let Err(e) = domain_indexer::write_index_md(&mem_dir, &index) {
        eprintln!("falha ao escrever {}: {e}", domain_indexer::INDEX_MD);
        return ExitCode::from(1);
    }

    let n = index
        .get("files")
        .and_then(Value::as_object)
        .map(serde_json::Map::len)
        .unwrap_or(0);
    println!("✓ code index reconstruído: {n} arquivo(s)");
    println!("  {}", mem_dir.join(domain_indexer::INDEX_MD).display());
    ExitCode::SUCCESS
}
