//! # install — onde o *produto instalado* mora no disco
//!
//! Os demais módulos de `core-paths` resolvem a **memória do projeto**
//! (`<project_root>/.claude-project-memory`), que é dado do usuário.
//! Este resolve a outra metade: os **assets do próprio plugin**
//! (`plugin/`, `ui/`) que acompanham os binários.
//!
//! ## Por que existe
//!
//! Até a distribuição via tarball, o único jeito de instalar era clonar
//! o repo — então `plugin/` e `ui-next/dist/` sempre existiam num path
//! derivável do `cwd` ou fixado em tempo de compilação
//! (`concat!(env!("CARGO_MANIFEST_DIR"), "/../../../plugin")`). Nenhuma
//! das duas heurísticas sobrevive a um binário baixado por `curl` e
//! extraído em `~/.local/share/pmem`: o `cwd` é o projeto do usuário e o
//! `CARGO_MANIFEST_DIR` é o diretório da máquina que compilou.
//!
//! A âncora estável é o **próprio executável**. Os dois layouts oficiais
//! colocam os binários a uma distância fixa da raiz:
//!
//! ```text
//! tarball                          clone git
//! <root>/                          <repo>/
//! ├── .claude-plugin/              ├── .claude-plugin/
//! │   └── marketplace.json         │   └── marketplace.json
//! ├── plugin/                      ├── plugin/
//! │   └── bin/pmem-http            ├── ui-next/dist/
//! └── ui/                          └── v2/target/release/pmem-http
//! ```
//!
//! Em ambos, subir os ancestrais do executável até achar
//! `.claude-plugin/marketplace.json` acerta a raiz — o mesmo marcador que
//! o Claude Code usa para reconhecer um marketplace de plugins, então não
//! inventamos convenção nova.
//!
//! ## Ordem de resolução
//!
//! Toda função aqui é `env override → derivação do executável → `None``.
//! O override existe para quem monta layout próprio (container, Nix,
//! packaging de distro) e é a única forma de apontar para fora da
//! árvore de instalação.

use std::path::{Path, PathBuf};

/// Marcador que identifica a raiz de instalação. É o manifesto que o
/// Claude Code lê para registrar o marketplace — existe nos dois layouts
/// (tarball e clone) e em nenhum outro lugar da árvore.
const ROOT_MARKER: &[&str] = &[".claude-plugin", "marketplace.json"];

/// Quantos ancestrais do executável inspecionar. O layout mais profundo
/// é o do clone git (`<repo>/v2/target/release/pmem-http` → 4 níveis);
/// 6 dá folga para `target/<triple>/release/` sem sair varrendo o disco
/// inteiro até `/` e casar com um marketplace alheio.
const MAX_ANCESTORS: usize = 6;

/// Lê uma env var tratando string vazia como ausente.
///
/// `PROJECT_MEM_HOME=` (vazio) é a forma usual de "desliga esse
/// override" em shell script; sem isso viraria `PathBuf::from("")`, que
/// resolve para o cwd e leva a erro silencioso.
fn env_path(key: &str) -> Option<PathBuf> {
    let raw = std::env::var(key).ok()?;
    if raw.trim().is_empty() {
        return None;
    }
    Some(PathBuf::from(raw))
}

/// Sobe os ancestrais de `start` procurando `.claude-plugin/marketplace.json`.
///
/// `start` é um **diretório**. Retorna o primeiro ancestral (inclusive
/// ele mesmo) que contém o marcador, ou `None` após [`MAX_ANCESTORS`]
/// níveis. Pura — não lê env nem `current_exe`, então é o ponto de
/// teste da heurística.
pub fn find_install_root_from(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .take(MAX_ANCESTORS + 1)
        .find(|dir| {
            let mut marker = dir.to_path_buf();
            marker.extend(ROOT_MARKER);
            marker.is_file()
        })
        .map(Path::to_path_buf)
}

/// Raiz da instalação — o diretório que contém `plugin/` e a UI.
///
/// 1. `$PROJECT_MEM_HOME`, se apontar para um diretório existente.
/// 2. Derivada do executável em curso (ver [`find_install_root_from`]).
/// 3. `None` — binário rodando fora de qualquer árvore reconhecível
///    (ex.: copiado solto para `/usr/local/bin`). Cabe ao caller decidir
///    o fallback; nenhum path é chutado aqui.
pub fn install_root() -> Option<PathBuf> {
    if let Some(home) = env_path("PROJECT_MEM_HOME") {
        if home.is_dir() {
            return Some(home);
        }
    }
    let exe = std::env::current_exe().ok()?;
    // `canonicalize` resolve o symlink de `~/.local/bin/pmem` para o
    // binário real dentro da árvore de instalação — sem isso a subida
    // de ancestrais começaria em `~/.local/bin` e não acharia nada.
    let exe = exe.canonicalize().unwrap_or(exe);
    find_install_root_from(exe.parent()?)
}

/// Diretório `plugin/` — seeds, pipelines, best-practices, skills,
/// `agent-bin/`, `hooks/`.
///
/// 1. `$PROJECT_MEM_PLUGIN_DIR` (override cru, sem checagem de
///    existência — paridade com o comportamento anterior do `pmem-http`).
/// 2. `<install_root>/plugin`.
pub fn plugin_dir() -> Option<PathBuf> {
    env_path("PROJECT_MEM_PLUGIN_DIR").or_else(|| install_root().map(|r| r.join("plugin")))
}

/// Resolve o diretório da UI buildada dentro de uma raiz conhecida.
///
/// Os dois layouts diferem: o tarball já traz o build pronto em `ui/`
/// (mais raso, sem `node_modules` por perto), o clone git tem a saída do
/// Vite em `ui-next/dist/`. Ambos são checados pela presença do
/// `index.html` — diretório vazio conta como ausente, que é o que faz o
/// `pmem-http` mostrar a página "rode o build" em vez de servir 404.
///
/// Pura, para teste direto com `tempfile`.
pub fn resolve_ui_dist(root: &Path) -> Option<PathBuf> {
    [root.join("ui"), root.join("ui-next").join("dist")]
        .into_iter()
        .find(|dir| dir.join("index.html").is_file())
}

/// Diretório servido como front estático pelo `pmem-http`.
///
/// 1. `$PROJECT_MEM_UI_DIR`, se contiver `index.html`.
/// 2. `<install_root>/ui` ou `<install_root>/ui-next/dist`.
pub fn ui_dist_dir() -> Option<PathBuf> {
    if let Some(dir) = env_path("PROJECT_MEM_UI_DIR") {
        if dir.join("index.html").is_file() {
            return Some(dir);
        }
    }
    install_root().as_deref().and_then(resolve_ui_dist)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Cria `<root>/.claude-plugin/marketplace.json`.
    fn mark_root(root: &Path) {
        let dir = root.join(".claude-plugin");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("marketplace.json"), "{}").unwrap();
    }

    fn mkdirs(path: &Path) -> PathBuf {
        std::fs::create_dir_all(path).unwrap();
        path.to_path_buf()
    }

    #[test]
    fn acha_raiz_no_layout_do_tarball() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        mark_root(root);
        let bin = mkdirs(&root.join("plugin").join("bin"));

        assert_eq!(find_install_root_from(&bin).as_deref(), Some(root));
    }

    #[test]
    fn acha_raiz_no_layout_do_clone_git() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        mark_root(root);
        let bin = mkdirs(&root.join("v2").join("target").join("release"));

        assert_eq!(find_install_root_from(&bin).as_deref(), Some(root));
    }

    #[test]
    fn o_proprio_diretorio_conta_como_raiz() {
        let tmp = TempDir::new().unwrap();
        mark_root(tmp.path());

        assert_eq!(
            find_install_root_from(tmp.path()).as_deref(),
            Some(tmp.path())
        );
    }

    #[test]
    fn sem_marcador_devolve_none() {
        let tmp = TempDir::new().unwrap();
        let bin = mkdirs(&tmp.path().join("plugin").join("bin"));

        assert_eq!(find_install_root_from(&bin), None);
    }

    #[test]
    fn nao_sobe_alem_do_limite_de_ancestrais() {
        let tmp = TempDir::new().unwrap();
        mark_root(tmp.path());
        // 7 níveis abaixo da raiz — um a mais que MAX_ANCESTORS.
        let deep = mkdirs(&tmp.path().join("a/b/c/d/e/f/g"));

        assert_eq!(find_install_root_from(&deep), None);
    }

    #[test]
    fn ui_dist_prefere_layout_do_tarball() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let tarball = mkdirs(&root.join("ui"));
        let clone = mkdirs(&root.join("ui-next").join("dist"));
        std::fs::write(tarball.join("index.html"), "<!doctype html>").unwrap();
        std::fs::write(clone.join("index.html"), "<!doctype html>").unwrap();

        assert_eq!(resolve_ui_dist(root), Some(tarball));
    }

    #[test]
    fn ui_dist_cai_pro_layout_do_clone() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let clone = mkdirs(&root.join("ui-next").join("dist"));
        std::fs::write(clone.join("index.html"), "<!doctype html>").unwrap();

        assert_eq!(resolve_ui_dist(root), Some(clone));
    }

    #[test]
    fn ui_dist_ignora_diretorio_sem_index_html() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        mkdirs(&root.join("ui"));

        assert_eq!(resolve_ui_dist(root), None);
    }
}
