//! Core do binário `poupar` (install / uninstall / status).
//!
//! Escreve o registro do plugin em `~/.claude/settings.json`: entrada de
//! marketplace, plugin habilitado e MCP server. Output visual das
//! mensagens é estável — docs e scripts dependem
//! dos "✓" / "✗" literais.
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

pub const MARKETPLACE_KEY: &str = "open-poupar";
pub const PLUGIN_KEY: &str = "open-poupar@open-poupar";
pub const MCP_SERVER_KEY: &str = "open-poupar";

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

/// Caminho do binário do MCP server dentro de uma instalação.
///
/// Existe como função (e não literal espalhado) porque o `install.sh`, o
/// smoke do tarball e o registro no `settings.json` precisam concordar
/// byte-a-byte sobre onde o binário está.
///
/// Mesma ordem do `plugin/bin-path.sh`: `plugin/bin/` (tarball) primeiro;
/// se não existir e houver build em `v2/target/release/` (clone + `make
/// install`), usa o build. Sem nenhum dos dois, devolve o caminho do
/// tarball — é onde o `install.sh` vai colocar o binário.
pub fn mcp_bin(install_dir: &Path) -> PathBuf {
    let installed = install_dir.join("plugin").join("bin").join("poupar-mcp");
    if installed.exists() {
        return installed;
    }
    let dev_build = install_dir
        .join("v2")
        .join("target")
        .join("release")
        .join("poupar-mcp");
    if dev_build.exists() {
        return dev_build;
    }
    installed
}

/// Parâmetros para o `apply_install`.
#[derive(Debug, Clone)]
pub struct InstallConfig {
    pub install_dir: PathBuf,
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

    // extraKnownMarketplaces["open-poupar"] = { source: { source: "directory", path: install_dir } }
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

    // enabledPlugins["open-poupar@open-poupar"] = true
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

    // mcpServers[<chave>] = { type: "stdio", command: <bin>, args: [] }
    // Sem args: o binário resolve mem_dir a partir do cwd, como os hooks.
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
            "command": mcp_bin(&cfg.install_dir).to_string_lossy(),
            "args": []
        }),
    );
}

// ─── Uninstall ──────────────────────────────────────────────────────

/// Remove as 3 entradas do open-poupar. Se após remover algum dos
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
                "✓ open-poupar instalado\n  Diretório: {dir}\n  Settings:  {}\n",
                settings_path.display()
            ),
            StatusReport::Incomplete {
                has_marketplace,
                has_plugin,
            } => {
                let mut out =
                    String::from("✗ open-poupar não instalado (ou instalação incompleta)\n");
                if !has_marketplace {
                    out.push_str("  → falta: extraKnownMarketplaces[\"open-poupar\"]\n");
                }
                if !has_plugin {
                    out.push_str("  → falta: enabledPlugins[\"open-poupar@open-poupar\"]\n");
                }
                out.push_str("  → rode: make install\n");
                out
            }
        }
    }
}
