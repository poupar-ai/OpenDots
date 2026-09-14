//! # core-branding
//!
//! Carrega a identidade do produto (nome, URLs, canal de build) a
//! partir de `settings.toml` na raiz do workspace.
//!
//! ## Por que existe
//!
//! open-poupar é distribuído como binário e pode ser rebrandeado sem
//! recompilar. Tudo que é **identidade exposta** (nome mostrado no
//! CLI, URL do dashboard, endpoint de update) mora aqui num único
//! lugar — mudar o branding não requer search/replace no código.
//! (Rebranding para uso comercial depende de licença comercial; a
//! PolyForm Noncommercial cobre só uso não-comercial.)
//!
//! O arquivo é lido em tempo de boot, não embutido no binário:
//! um mesmo binário Pro roda com `settings.toml` diferente em
//! clientes diferentes.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Identidade completa. Estrutura top-level do `settings.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Branding {
    pub product: Product,
    pub urls: Urls,
    pub build: Build,
}

/// Nomes e assets do produto.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Product {
    /// Nome interno usado em logs/paths (estável entre versões
    /// whitelabel para não quebrar memória do usuário).
    pub internal_name: String,
    /// Nome mostrado ao usuário (pode variar por whitelabel).
    pub display_name: String,
    /// Nome do executável instalado (`poupar`, `mem`, etc.).
    pub binary_name: String,
    /// Path relativo do logo exibido na UI/instalador.
    pub logo_path: String,
}

/// URLs externas resolvidas em runtime. Mudar aqui reaponta telemetria,
/// updates, docs sem recompilar.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Urls {
    pub website: String,
    pub docs: String,
    pub dashboard: String,
    pub update: String,
    pub api: String,
}

/// Metadados do build em si.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Build {
    /// Canal de release: "dev" / "beta" / "stable".
    pub channel: String,
    /// Target triple para match com o `UpdateManifest`.
    pub target: String,
}

#[derive(Debug, thiserror::Error)]
pub enum BrandingError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    /// `toml::de::Error` já traz linha/coluna — não reembrulhamos.
    #[error(transparent)]
    Parse(#[from] toml::de::Error),
}

impl Branding {
    /// Parse a partir de string TOML. Usado em testes e quando o
    /// conteúdo já está em memória.
    pub fn from_toml(s: &str) -> Result<Self, BrandingError> {
        Ok(toml::from_str(s)?)
    }

    /// Parse a partir de arquivo. Caminho comum: boot lê
    /// `./settings.toml` relativo ao diretório do binário.
    pub fn from_file(path: &Path) -> Result<Self, BrandingError> {
        let content = std::fs::read_to_string(path).map_err(|source| BrandingError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_toml(&content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"
[product]
internal_name = "open-poupar"
display_name = "open-poupar"
binary_name = "poupar"
logo_path = "assets/logo.png"

[urls]
website = "https://example.invalid"
docs = "https://example.invalid/docs"
dashboard = "https://example.invalid/app"
update = "https://example.invalid/updates"
api = "https://example.invalid/api"

[build]
channel = "dev"
target = "x86_64-unknown-linux-gnu"
"#;

    #[test]
    fn parses_example() {
        let b = Branding::from_toml(EXAMPLE).unwrap();
        assert_eq!(b.product.internal_name, "open-poupar");
        assert_eq!(b.product.binary_name, "poupar");
        assert_eq!(b.urls.website, "https://example.invalid");
        assert_eq!(b.build.channel, "dev");
    }

    #[test]
    fn roundtrips() {
        let b = Branding::from_toml(EXAMPLE).unwrap();
        let out = toml::to_string(&b).unwrap();
        let b2 = Branding::from_toml(&out).unwrap();
        assert_eq!(b, b2);
    }

    #[test]
    fn rejects_missing_fields() {
        let bad = "[product]\ninternal_name = \"x\"\n";
        assert!(Branding::from_toml(bad).is_err());
    }
}
