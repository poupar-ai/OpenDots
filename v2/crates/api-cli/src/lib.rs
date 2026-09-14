//! Core do binário `pmem` (install / uninstall / status).
//!
//! Replica `scripts/{install,uninstall,status}.cjs` byte-a-byte no
//! conteúdo gerado em `~/.claude/settings.json`. Output visual das
//! mensagens é byte-idêntico também — usuários e docs já dependem
//! dos "✓" / "✗" literais. Única exceção: a linha do MCP server
//! anota o launcher (`node` / `nativo`), informação que não existia
//! no `.cjs` porque lá só havia o caminho Node.
//!
//! ## Arquitetura
//!
//! - `settings_path()` — replica `CLAUDE_CONFIG_DIR || ~/.claude`.
//! - `apply_install(settings, cfg)` / `apply_uninstall(settings)` —
//!   puras, recebem o `Value` carregado e mutam. Paridade garante que
//!   os mesmos bytes de entrada produzem os mesmos bytes de saída.
//! - `describe_status(settings)` — classifica o estado de instalação
//!   sem I/O. Output de tela fica em `StatusReport::render`.
//! - `write_settings_json(path, value)` — serializa com indent de 2
//!   spaces + trailing newline, exatamente como
//!   `JSON.stringify(settings, null, 2) + '\n'` do `.cjs`.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

// ─── Constantes — chaves literais compartilhadas com o .cjs ─────────

pub const MARKETPLACE_KEY: &str = "project-man";
pub const PLUGIN_KEY: &str = "project-man@project-man";
pub const MCP_SERVER_KEY: &str = "project-mem";

// ─── Erros ──────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("parse {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("$HOME não definido e --config-dir não passado")]
    HomeMissing,
}

// ─── Path helpers ───────────────────────────────────────────────────

/// Caminho para `settings.json` seguindo a mesma resolução do `.cjs`:
/// `$CLAUDE_CONFIG_DIR/settings.json` se setada; caso contrário
/// `$HOME/.claude/settings.json`. Retorna erro só se nenhum dos dois
/// existir (ambiente realmente sem `$HOME` — raro).
pub fn settings_path() -> Result<PathBuf, Error> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Ok(PathBuf::from(dir).join("settings.json"));
    }
    let home = std::env::var_os("HOME").ok_or(Error::HomeMissing)?;
    Ok(PathBuf::from(home).join(".claude").join("settings.json"))
}

/// Serializa `settings` com indent 2 e trailing newline. Usar em vez
/// de `fs::write + serde_json::to_string_pretty` direto para manter
/// paridade exata com `JSON.stringify(settings, null, 2) + '\n'`.
pub fn serialize_settings(settings: &Value) -> String {
    let mut s = serde_json::to_string_pretty(settings).expect("Value is always serializable");
    s.push('\n');
    s
}

/// Lê `settings.json` se existe; retorna `Ok(None)` se missing para o
/// caller decidir o que fazer (install cria do zero; status mostra
/// "não instalado"; uninstall no-op).
pub fn read_settings(path: &Path) -> Result<Option<Value>, Error> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(Error::Io {
                path: path.display().to_string(),
                source,
            });
        }
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(|source| Error::Parse {
        path: path.display().to_string(),
        source,
    })?;
    Ok(Some(value))
}

pub fn write_settings(path: &Path, settings: &Value) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    std::fs::write(path, serialize_settings(settings)).map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })
}

// ─── Install ────────────────────────────────────────────────────────

/// Como o Claude Code deve lançar o MCP server do project-mem.
///
/// Os dois layouts de instalação diferem no que existe no disco:
/// o clone git traz `plugin/mcp/server.cjs` e depende de um Node
/// externo; o tarball traz `plugin/bin/pmem-mcp` compilado e não
/// depende de runtime nenhum.
#[derive(Debug, Clone)]
pub enum McpLauncher {
    /// `plugin/mcp/server.cjs` rodado por Node. `node_bin` substitui
    /// `process.execPath` — em Rust não existe equivalente implícito; o
    /// binário `pmem` resolve via `--node-bin`, env `NODE_BIN` ou
    /// fallback `"node"`.
    Node { node_bin: PathBuf },
    /// Binário nativo `plugin/bin/pmem-mcp`, invocado direto e sem args.
    Native,
}

impl McpLauncher {
    /// Caminho do binário nativo dentro de uma instalação.
    pub fn native_bin(install_dir: &Path) -> PathBuf {
        install_dir.join("plugin").join("bin").join("pmem-mcp")
    }

    /// Escolhe o launcher olhando o disco: nativo quando o tarball
    /// trouxe o binário, Node caso contrário. Deixa o `pmem install`
    /// funcionar igual nos dois layouts, sem flag nova.
    pub fn detect(install_dir: &Path, node_bin: PathBuf) -> Self {
        if Self::native_bin(install_dir).is_file() {
            Self::Native
        } else {
            Self::Node { node_bin }
        }
    }
}

/// Parâmetros para o `apply_install`.
#[derive(Debug, Clone)]
pub struct InstallConfig {
    pub install_dir: PathBuf,
    pub mcp: McpLauncher,
}

/// Faz merge das 3 entradas necessárias em `settings`, criando as
/// chaves de topo se não existirem. Não limpa valores existentes —
/// install é idempotente e convive com customizações do usuário
/// (mesma semântica do `.cjs`).
pub fn apply_install(settings: &mut Value, cfg: &InstallConfig) {
    if !settings.is_object() {
        *settings = Value::Object(Map::new());
    }
    let obj = settings.as_object_mut().unwrap();

    // extraKnownMarketplaces["project-man"] = { source: { source: "directory", path: install_dir } }
    let marketplaces = obj
        .entry("extraKnownMarketplaces".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !marketplaces.is_object() {
        *marketplaces = Value::Object(Map::new());
    }
    marketplaces.as_object_mut().unwrap().insert(
        MARKETPLACE_KEY.to_string(),
        json!({
            "source": {
                "source": "directory",
                "path": cfg.install_dir.to_string_lossy()
            }
        }),
    );

    // enabledPlugins["project-man@project-man"] = true
    let plugins = obj
        .entry("enabledPlugins".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !plugins.is_object() {
        *plugins = Value::Object(Map::new());
    }
    plugins
        .as_object_mut()
        .unwrap()
        .insert(PLUGIN_KEY.to_string(), Value::Bool(true));

    // mcpServers["project-mem"] = { type: "stdio", command, args }
    //   Node:   command = node_bin, args = [ install_dir/plugin/mcp/server.cjs ]
    //   Native: command = install_dir/plugin/bin/pmem-mcp, args = []
    let (command, args) = match &cfg.mcp {
        McpLauncher::Node { node_bin } => {
            let script = cfg
                .install_dir
                .join("plugin")
                .join("mcp")
                .join("server.cjs");
            (
                node_bin.to_string_lossy().into_owned(),
                vec![Value::String(script.to_string_lossy().into_owned())],
            )
        }
        McpLauncher::Native => (
            McpLauncher::native_bin(&cfg.install_dir)
                .to_string_lossy()
                .into_owned(),
            vec![],
        ),
    };
    let mcp_servers = obj
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !mcp_servers.is_object() {
        *mcp_servers = Value::Object(Map::new());
    }
    mcp_servers.as_object_mut().unwrap().insert(
        MCP_SERVER_KEY.to_string(),
        json!({
            "type": "stdio",
            "command": command,
            "args": args
        }),
    );
}

// ─── Uninstall ──────────────────────────────────────────────────────

/// Remove as 3 entradas do project-man. Se após remover algum dos
/// maps de topo ficar vazio, remove a chave de topo também (paridade
/// com `if (Object.keys(x).length === 0) delete settings[x]`).
///
/// Retorna `true` se algo foi removido — caller usa o flag para decidir
/// entre "removido" e "nada a fazer" no output.
pub fn apply_uninstall(settings: &mut Value) -> bool {
    let Some(obj) = settings.as_object_mut() else {
        return false;
    };
    let mut changed = false;

    changed |= remove_from_map(obj, "extraKnownMarketplaces", MARKETPLACE_KEY);
    changed |= remove_from_map(obj, "enabledPlugins", PLUGIN_KEY);
    changed |= remove_from_map(obj, "mcpServers", MCP_SERVER_KEY);

    changed
}

fn remove_from_map(obj: &mut Map<String, Value>, top_key: &str, inner_key: &str) -> bool {
    let Some(inner) = obj.get_mut(top_key) else {
        return false;
    };
    let Some(inner_obj) = inner.as_object_mut() else {
        return false;
    };
    if inner_obj.remove(inner_key).is_none() {
        return false;
    }
    if inner_obj.is_empty() {
        obj.remove(top_key);
    }
    true
}

// ─── Status ─────────────────────────────────────────────────────────

/// Estado observável de instalação. Nomes das variantes espelham os
/// branches do `scripts/status.cjs`.
#[derive(Debug, PartialEq, Eq)]
pub enum StatusReport {
    /// settings.json não existe.
    SettingsMissing,
    /// settings.json existe mas não parseia.
    SettingsInvalid,
    /// marketplace + plugin presentes.
    Installed { dir: String },
    /// Ao menos um ausente.
    Incomplete {
        has_marketplace: bool,
        has_plugin: bool,
    },
}

/// Classifica o estado a partir de um `Value` já carregado. Separar
/// classificação de render permite testar a decisão sem capturar
/// stdout.
pub fn describe_status(settings: &Value) -> StatusReport {
    let has_marketplace = settings
        .get("extraKnownMarketplaces")
        .and_then(|v| v.get(MARKETPLACE_KEY))
        .is_some();
    let has_plugin = settings
        .get("enabledPlugins")
        .and_then(|v| v.get(PLUGIN_KEY))
        .and_then(|v| v.as_bool())
        == Some(true)
        || settings
            .get("enabledPlugins")
            .and_then(|v| v.get(PLUGIN_KEY))
            .is_some_and(|v| !v.is_null() && v != &Value::Bool(false));

    if has_marketplace && has_plugin {
        let dir = settings
            .get("extraKnownMarketplaces")
            .and_then(|v| v.get(MARKETPLACE_KEY))
            .and_then(|v| v.get("source"))
            .and_then(|v| v.get("path"))
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string();
        StatusReport::Installed { dir }
    } else {
        StatusReport::Incomplete {
            has_marketplace,
            has_plugin,
        }
    }
}

impl StatusReport {
    /// Renderiza o bloco de texto emitido pelo `status.cjs`. `settings_path`
    /// é passado à parte porque `describe_status` não precisa saber disso —
    /// só quem printou (binário) sabe de onde leu.
    pub fn render(&self, settings_path: &Path) -> String {
        match self {
            StatusReport::SettingsMissing => {
                "✗ settings.json não encontrado\n  → rode: make install\n".to_string()
            }
            StatusReport::SettingsInvalid => "✗ settings.json inválido\n".to_string(),
            StatusReport::Installed { dir } => format!(
                "✓ project-man instalado\n  Diretório: {dir}\n  Settings:  {}\n",
                settings_path.display()
            ),
            StatusReport::Incomplete {
                has_marketplace,
                has_plugin,
            } => {
                let mut out =
                    String::from("✗ project-man não instalado (ou instalação incompleta)\n");
                if !has_marketplace {
                    out.push_str("  → falta: extraKnownMarketplaces[\"project-man\"]\n");
                }
                if !has_plugin {
                    out.push_str("  → falta: enabledPlugins[\"project-man@project-man\"]\n");
                }
                out.push_str("  → rode: make install\n");
                out
            }
        }
    }
}
