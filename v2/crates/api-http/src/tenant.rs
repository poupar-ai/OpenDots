//! `TenantContext` — request-scoped, identifica de qual tenant veio a
//! request e onde mora a memória dele.
//!
//! Hoje sempre `id = "default"`, `mem_dir = AppState.mem_dir` —
//! comportamento idêntico ao pré-tenant. O ponto disso existir agora é
//! que handlers já leem de `tc.mem_dir` em vez de `st.mem_dir`. Quando
//! multi-tenancy real ativar, basta mudar o resolver no middleware
//! (`auth.rs`): nenhum handler precisa mudar.
//!
//! Layout futuro de mem_dir multi-tenant:
//!   `<base>/<tenant_id>/.claude-project-memory/`
//! Pro tenant `default` (caso atual), mantemos `<base>/.claude-project-memory/`
//! pra não exigir migração de quem já está rodando.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct TenantContext {
    pub id: String,
    pub mem_dir: PathBuf,
}

impl TenantContext {
    /// Constrói o contexto do tenant `default` — usado quando a request
    /// não tem Bearer token (UI local, dev). Aponta pro `mem_dir`
    /// raiz do AppState, sem subdir.
    pub fn default_for(base_mem_dir: &Path) -> Self {
        Self {
            id: "default".to_string(),
            mem_dir: base_mem_dir.to_path_buf(),
        }
    }

    /// Constrói pro tenant nomeado — usado quando multi-tenancy real
    /// ativar. `<base>/<tenant_id>/.claude-project-memory/`.
    pub fn for_named(base_mem_dir: &Path, tenant_id: &str) -> Self {
        // Quando multi-tenant ativar, base_mem_dir provavelmente vai ser
        // um diretório-pai (ex.: `/var/lib/project-mem/tenants/`).
        // Por hora essa fn não é chamada — fica como sinalização da forma.
        let _ = base_mem_dir;
        let _ = tenant_id;
        unimplemented!("multi-tenant resolver não ativado — use default_for");
    }
}
