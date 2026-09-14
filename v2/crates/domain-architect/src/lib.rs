//! # domain-architect
//!
//! Porta Rust de `plugin/scripts/architect.cjs` — o **Arquiteto** é um
//! agente meta do open-poupar que entrevista o humano e gera "seeds"
//! customizados (packs de agentes) pro negócio dele.
//!
//! ## Escopo (Fase 4)
//!
//! - Constantes: [`ARCHITECT_AGENT`], [`ARCHITECT_PROMPT`].
//! - [`architect_config`] / [`architect_context`] — config estática +
//!   prompt do agente.
//! - [`ensure_installed`] — garante que o architect aparece na
//!   `agents.json` do projeto e o mini-context existe.
//! - [`generate_seed`] — converte um spec JSON produzido pela conversa
//!   com o humano em `plugin/seeds/<category>/<slug>/`.
//!
//! Paridade byte-a-byte validada por `tests/parity_*.rs` contra o
//! `.cjs` original.

use serde_json::{Map, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Nome canônico do agente meta. Replica `ARCHITECT_AGENT = 'architect'`
/// em `architect.cjs:20`.
pub const ARCHITECT_AGENT: &str = "architect";

/// Prompt que vai no mini-context do arquiteto. Texto literal do
/// `architect.cjs:22-49`.
pub const ARCHITECT_PROMPT: &str = "Você é o Arquiteto — agente meta do open-poupar.

Seu papel é entrevistar o humano para entender o negócio dele e desenhar um
TIME DE AGENTES customizado (um \"seed\"). Você:

- Faz UMA pergunta por vez. Curtas. Em tom de conversa, não de formulário.
- Nunca pede pra responder por número. Aceita resposta livre.
- Sempre confirma antes de avançar (\"entendi assim, segue ou ajusto?\").
- Quando tiver contexto suficiente, propõe o time: nomes, funções, 1 linha cada.
- Usa YAGNI: só cria agente que resolve dor clara.

Informações que você precisa coletar (em qualquer ordem):
1. Nome do negócio e o que ele faz em 1 frase.
2. Setor/categoria (tecnologia, atendimento, delivery, vendas, outros).
3. Canais principais (WhatsApp, Instagram, site, marketplace).
4. 3 dores atuais que um time de agentes resolveria.
5. Tom do negócio (formal, casual, técnico, emocional).

Ao final, apresente o time e espere \"gera o seed\" para concluir. Depois disso
o humano pode rodar /api/architect/generate via UI, que converte a conversa em
seeds/custom/<slug>/.

Lembretes:
- Se o humano começar direto pelo \"gera o seed\", pergunte o mínimo antes.
- Se o negócio for claramente um dos existentes (tech), diga isso antes de
  criar custom — pode ser mais barato aplicar um pack pronto.
- Glossário do projeto em PROJECT.md é referência obrigatória.
";

/// Config estática do arquiteto — usada em `agents.json`.
///
/// Replica `getArchitectConfig()` em `architect.cjs:51-63`.
/// Ordem das chaves: `label, icon, category, description, context_file,
/// best_practices, channels`.
pub fn architect_config() -> Value {
    let mut m = Map::new();
    m.insert("label".into(), Value::String("Arquiteto".into()));
    m.insert("icon".into(), Value::String("🧠".into()));
    m.insert("category".into(), Value::String("meta".into()));
    m.insert(
        "description".into(),
        Value::String(
            "Entrevista o humano e desenha um time de agentes customizado pro negócio.".into(),
        ),
    );
    m.insert(
        "context_file".into(),
        Value::String("agents/architect-meta.md".into()),
    );
    m.insert("best_practices".into(), Value::Array(Vec::new()));
    let mut channels = Map::new();
    channels.insert("chat_required".into(), Value::Bool(true));
    m.insert("channels".into(), Value::Object(channels));
    Value::Object(m)
}

/// Retorna o [`ARCHITECT_PROMPT`] como `&str`. Wrapper paridade 1:1
/// com `getArchitectContext()`.
pub fn architect_context() -> &'static str {
    ARCHITECT_PROMPT
}

/// Erro de [`ensure_installed`].
#[derive(Debug, Error)]
pub enum EnsureInstalledError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("agents.json inválido: {0}")]
    InvalidManifest(serde_json::Error),
}

/// Resultado de [`ensure_installed`] — replica `{ installed: true }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsureInstalledResult {
    pub installed: bool,
}

/// Garante que o architect aparece na `agents.json` do projeto e o
/// mini-context existe. Idempotente.
///
/// Replica `ensureInstalled({ memDir })` em `architect.cjs:70-92`.
///
/// Semântica exata do `.cjs`:
/// - Se `memDir` não existe, cria (recursive).
/// - Se `agents.json` existe, parseia; senão cria default
///   `{version:1, manager:'manager', oracle:'oracle', agents:{}}`.
/// - Se `manifest.agents` ausente (objeto sem campo), inicializa com
///   `{}`.
/// - Só regrava `agents.json` se architect NÃO está lá já como
///   `category: 'meta'`.
/// - Cria `agents/architect-meta.md` somente se o arquivo não existir
///   (não sobrescreve customizações).
///
/// Retorna sempre `{ installed: true }` — o nome é enganoso, significa
/// "após esta chamada está instalado", não "instalei agora".
pub fn ensure_installed(mem_dir: &Path) -> Result<EnsureInstalledResult, EnsureInstalledError> {
    if !mem_dir.exists() {
        fs::create_dir_all(mem_dir)?;
    }
    let manifest_path = mem_dir.join("agents.json");
    let mut manifest: Value = if manifest_path.exists() {
        let raw = fs::read_to_string(&manifest_path)?;
        serde_json::from_str(&raw).map_err(EnsureInstalledError::InvalidManifest)?
    } else {
        let mut m = Map::new();
        m.insert("version".into(), Value::from(1));
        m.insert("manager".into(), Value::String("manager".into()));
        m.insert("oracle".into(), Value::String("oracle".into()));
        m.insert("agents".into(), Value::Object(Map::new()));
        Value::Object(m)
    };

    // `if (!manifest.agents) manifest.agents = {}` — só cria se ausente ou não-objeto.
    let needs_agents = matches!(
        manifest.as_object().and_then(|o| o.get("agents")),
        None | Some(Value::Null)
    ) || manifest
        .as_object()
        .and_then(|o| o.get("agents"))
        .is_some_and(|v| !v.is_object());
    if needs_agents {
        if let Some(o) = manifest.as_object_mut() {
            o.insert("agents".into(), Value::Object(Map::new()));
        }
    }

    let already = manifest
        .get("agents")
        .and_then(|a| a.get("architect"))
        .and_then(|ar| ar.get("category"))
        .and_then(Value::as_str)
        == Some("meta");

    if !already {
        if let Some(agents) = manifest
            .as_object_mut()
            .and_then(|o| o.get_mut("agents"))
            .and_then(Value::as_object_mut)
        {
            agents.insert("architect".into(), architect_config());
        }
        let serialized = serde_json::to_string_pretty(&manifest).map_err(|e| {
            EnsureInstalledError::Io(io::Error::new(io::ErrorKind::InvalidData, e))
        })?;
        fs::write(&manifest_path, format!("{serialized}\n"))?;
    }

    let ctx_path = mem_dir.join("agents").join("architect-meta.md");
    if !ctx_path.exists() {
        if let Some(parent) = ctx_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&ctx_path, ARCHITECT_PROMPT)?;
    }

    Ok(EnsureInstalledResult { installed: true })
}

/// Provedor de timestamp. Existe pra permitir paridade determinística
/// em testes (o `.cjs` usa `new Date().toISOString()` direto).
pub trait Clock {
    fn now_iso(&self) -> String;
}

/// Clock de sistema — `chrono`-free: replica `new Date().toISOString()`
/// usando [`std::time::SystemTime`].
///
/// ISO 8601 sempre em UTC com precisão de millis (igual ao JS):
/// `YYYY-MM-DDTHH:MM:SS.sssZ`.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_iso(&self) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dur = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        format_iso_millis(dur.as_millis() as i64)
    }
}

fn format_iso_millis(mut ms_since_epoch: i64) -> String {
    // Decompõe epoch ms → YYYY-MM-DDTHH:MM:SS.sssZ.
    // Algoritmo Gregoriano direto — sem depender de chrono.
    let mut seconds = ms_since_epoch.div_euclid(1000);
    let millis = ms_since_epoch.rem_euclid(1000);
    let _ = &mut ms_since_epoch; // não usamos mais depois
    let days = seconds.div_euclid(86_400);
    seconds = seconds.rem_euclid(86_400);
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    // Convert days since 1970-01-01 → y/m/d (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, m, d, hours, minutes, secs, millis
    )
}

/// Clock fixo — devolve sempre a mesma string. Útil pra testes.
#[derive(Debug, Clone)]
pub struct FixedClock(pub String);

impl Clock for FixedClock {
    fn now_iso(&self) -> String {
        self.0.clone()
    }
}

/// Erro de [`generate_seed`].
#[derive(Debug, Error)]
pub enum GenerateSeedError {
    #[error("spec inválido: precisa slug e category")]
    MissingSlugOrCategory,
    #[error("slug e category devem ser kebab-case (a-z0-9-)")]
    InvalidKebabCase,
    #[error(r#"seed "{category}/{slug}" já existe"#)]
    AlreadyExists { category: String, slug: String },
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Resultado de [`generate_seed`]. Replica `{ok, ref, dir, agent_count}`
/// do `.cjs`. `ref` é palavra reservada em Rust — usamos `reference`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerateSeedResult {
    pub ok: bool,
    pub reference: String,
    pub dir: PathBuf,
    pub agent_count: usize,
}

/// Converte um spec produzido pela conversa arquiteto+humano em
/// `<seeds_dir>/<category>/<slug>/`.
///
/// Replica `generateSeed(spec)` em `architect.cjs:97-162`.
///
/// - Valida slug/category (kebab-case regex `^[a-z0-9-]+$`).
/// - Cria `<seeds_dir>/<category>/meta.json` se ainda não existe (com
///   `label` e `description` default derivados do spec).
/// - Falha se o pack `<seeds_dir>/<category>/<slug>` já existe.
/// - Cria `agents/` e, por cada agente do spec, escreve
///   `agents/<name>.md` e registra em `manifest.agents[name]`.
/// - Agrega `best_practices` do spec + de cada agente em um `Set`
///   (dedupe, ordem de primeira aparição).
/// - `generated_at` vem do [`Clock`] — em produção use [`SystemClock`].
///
/// Diferenças de paridade: ordem de chaves no JSON é mantida via
/// `serde_json` com `preserve_order` feature. Normalização de nome de
/// agente: `toLowerCase().replace(/[^a-z0-9_]/g, '_')` — ASCII-only,
/// todos os não-matches viram `_`.
pub fn generate_seed(
    seeds_dir: &Path,
    spec: &Value,
    clock: &dyn Clock,
) -> Result<GenerateSeedResult, GenerateSeedError> {
    let slug = spec
        .get("slug")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let category = spec
        .get("category")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let (slug, category) = match (slug, category) {
        (Some(s), Some(c)) => (s, c),
        _ => return Err(GenerateSeedError::MissingSlugOrCategory),
    };

    if !is_kebab_case(slug) || !is_kebab_case(category) {
        return Err(GenerateSeedError::InvalidKebabCase);
    }

    let cat_dir = seeds_dir.join(category);
    let pack_dir = cat_dir.join(slug);

    if !cat_dir.exists() {
        fs::create_dir_all(&cat_dir)?;
    }
    let cat_meta = cat_dir.join("meta.json");
    if !cat_meta.exists() {
        let mut m = Map::new();
        let cat_label = js_str_or(spec.get("category_label"), category);
        let cat_desc = js_str_or_owned(
            spec.get("category_description"),
            format!("Seeds gerados pelo Arquiteto ({category})."),
        );
        m.insert("label".into(), Value::String(cat_label.into()));
        m.insert("description".into(), Value::String(cat_desc));
        let serialized = serde_json::to_string_pretty(&Value::Object(m))?;
        fs::write(&cat_meta, format!("{serialized}\n"))?;
    }

    if pack_dir.exists() {
        return Err(GenerateSeedError::AlreadyExists {
            category: category.to_string(),
            slug: slug.to_string(),
        });
    }
    fs::create_dir_all(&pack_dir)?;
    fs::create_dir_all(pack_dir.join("agents"))?;

    let mut manifest = Map::new();
    manifest.insert("version".into(), Value::from(1));
    manifest.insert("manager".into(), Value::String("manager".into()));
    manifest.insert("oracle".into(), Value::String("oracle".into()));
    manifest.insert("agents".into(), Value::Object(Map::new()));

    // Set preservando ordem de primeira aparição (paridade com JS Set).
    let mut pack_bp_order: Vec<String> = Vec::new();
    let mut pack_bp_seen = std::collections::HashSet::new();
    if let Some(arr) = spec.get("best_practices").and_then(Value::as_array) {
        for v in arr {
            if let Some(s) = v.as_str() {
                if pack_bp_seen.insert(s.to_string()) {
                    pack_bp_order.push(s.to_string());
                }
            } else {
                // JS `new Set(arr)` adiciona o valor cru; quando iteramos
                // com `[...set]` e depois `JSON.stringify`, não-strings
                // aparecem como seu JSON. Preservamos paridade replicando
                // via Value::to_string, mas ainda deduplicando pelo JSON.
                let key = v.to_string();
                if pack_bp_seen.insert(key.clone()) {
                    pack_bp_order.push(js_stringify_value(v));
                }
            }
        }
    }

    let agents_arr: Vec<Value> = spec
        .get("agents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut written_count = 0usize;

    for a in &agents_arr {
        let raw_name = a.get("name");
        // JS: `if (!a.name) continue;` — vazio, null, undefined, 0, false caem.
        if !is_truthy(raw_name) {
            continue;
        }
        let name_str = js_to_string(raw_name.unwrap());
        let name = normalize_agent_name(&name_str);
        let ctx_rel = format!("agents/{name}.md");

        let mut agent_obj = Map::new();
        agent_obj.insert(
            "label".into(),
            Value::String(js_str_or(a.get("label"), &name).into()),
        );
        agent_obj.insert(
            "icon".into(),
            Value::String(js_str_or(a.get("icon"), "🤖").into()),
        );
        agent_obj.insert(
            "category".into(),
            Value::String(js_str_or(a.get("category"), "specialist").into()),
        );
        agent_obj.insert(
            "description".into(),
            Value::String(js_str_or(a.get("description"), "").into()),
        );
        agent_obj.insert("context_file".into(), Value::String(ctx_rel.clone()));
        agent_obj.insert(
            "best_practices".into(),
            a.get("best_practices")
                .and_then(|v| v.as_array().cloned())
                .map(Value::Array)
                .unwrap_or_else(|| Value::Array(Vec::new())),
        );

        if let Some(agents_map) = manifest.get_mut("agents").and_then(Value::as_object_mut) {
            agents_map.insert(name.clone(), Value::Object(agent_obj));
        }

        // `(a.best_practices || []).forEach(id => packBp.add(id))`
        if let Some(arr) = a.get("best_practices").and_then(Value::as_array) {
            for v in arr {
                let key = if let Some(s) = v.as_str() {
                    s.to_string()
                } else {
                    v.to_string()
                };
                if pack_bp_seen.insert(key.clone()) {
                    pack_bp_order.push(if let Some(s) = v.as_str() {
                        s.to_string()
                    } else {
                        js_stringify_value(v)
                    });
                }
            }
        }

        // `const md = a.prompt || \`# ${a.label || name}\n\n${a.description || ''}\n\``;
        let md = js_str_or_owned(
            a.get("prompt"),
            format!(
                "# {}\n\n{}\n",
                js_str_or(a.get("label"), &name),
                js_str_or(a.get("description"), "")
            ),
        );
        fs::write(pack_dir.join(&ctx_rel), md)?;
        written_count += 1;
    }

    let agents_json = pack_dir.join("agents.json");
    let serialized_manifest = serde_json::to_string_pretty(&Value::Object(manifest.clone()))?;
    fs::write(&agents_json, format!("{serialized_manifest}\n"))?;

    let mut pack_meta = Map::new();
    pack_meta.insert(
        "label".into(),
        Value::String(js_str_or(spec.get("label"), slug).into()),
    );
    pack_meta.insert(
        "description".into(),
        Value::String(
            js_str_or(spec.get("description"), "Seed customizado gerado pelo Arquiteto.").into(),
        ),
    );
    pack_meta.insert(
        "best_practices".into(),
        Value::Array(pack_bp_order.into_iter().map(Value::String).collect()),
    );
    pack_meta.insert("generated_by".into(), Value::String("architect".into()));
    pack_meta.insert("generated_at".into(), Value::String(clock.now_iso()));
    let serialized_pack_meta = serde_json::to_string_pretty(&Value::Object(pack_meta))?;
    fs::write(
        pack_dir.join("meta.json"),
        format!("{serialized_pack_meta}\n"),
    )?;

    let _ = written_count; // não usamos — .cjs usa Object.keys(manifest.agents).length
    let agent_count = manifest
        .get("agents")
        .and_then(Value::as_object)
        .map(|o| o.len())
        .unwrap_or(0);

    Ok(GenerateSeedResult {
        ok: true,
        reference: format!("{category}/{slug}"),
        dir: pack_dir,
        agent_count,
    })
}

// ── helpers privados ─────────────────────────────────────────────────────

/// Replica regex `^[a-z0-9-]+$` — non-empty, apenas ASCII lower/digit/hyphen.
fn is_kebab_case(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Replica `toLowerCase().replace(/[^a-z0-9_]/g, '_')` — ASCII-only.
///
/// Caracteres não-ASCII no `.cjs` passam pelo `toLowerCase` (Unicode)
/// e depois caem no regex como non-match → viram `_`. Replicamos:
/// lowercase ASCII, não-ASCII → `_`.
fn normalize_agent_name(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_lowercase() || lower.is_ascii_digit() || lower == '_' {
            out.push(lower);
        } else {
            out.push('_');
        }
    }
    out
}

/// JS truthy check aplicado a um `Option<&Value>` (tratando `None` =
/// undefined).
fn is_truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

/// `x || fallback` para strings — `""` é falsy.
fn js_str_or<'a>(v: Option<&'a Value>, fallback: &'a str) -> &'a str {
    match v.and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s,
        _ => fallback,
    }
}

fn js_str_or_owned(v: Option<&Value>, fallback: String) -> String {
    match v.and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => fallback,
    }
}

/// Replica `String(x)` do JS para um Value (usado em `String(a.name)`).
/// Conversão canônica:
/// - string → própria
/// - número → representação decimal JS-style
/// - bool → "true"/"false"
/// - null → "null"
/// - array/object → `JSON.stringify` (aproximação; o .cjs dá `"[object Object]"`
///   na real via `toString`, mas `String(a.name)` com object vira "[object Object]")
fn js_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(_) => {
            // JS: Array.prototype.toString → join(',')
            v.as_array()
                .unwrap()
                .iter()
                .map(js_to_string)
                .collect::<Vec<_>>()
                .join(",")
        }
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// Replica `JSON.stringify(x)` cru — útil quando um Set JS vira array
/// serializado.
fn js_stringify_value(v: &Value) -> String {
    v.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── architect_config ────────────────────────────────────────────────

    #[test]
    fn config_has_expected_fields() {
        let c = architect_config();
        assert_eq!(c["label"], "Arquiteto");
        assert_eq!(c["icon"], "🧠");
        assert_eq!(c["category"], "meta");
        assert_eq!(c["context_file"], "agents/architect-meta.md");
        assert_eq!(c["channels"]["chat_required"], true);
        assert!(c["best_practices"].is_array());
        assert_eq!(c["best_practices"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn config_key_order_matches_cjs() {
        let c = architect_config();
        let keys: Vec<&str> = c.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "label",
                "icon",
                "category",
                "description",
                "context_file",
                "best_practices",
                "channels",
            ]
        );
    }

    #[test]
    fn context_returns_prompt() {
        assert_eq!(architect_context(), ARCHITECT_PROMPT);
    }

    // ── ensure_installed ────────────────────────────────────────────────

    #[test]
    fn ensure_creates_dir_and_manifest() {
        let tmp = TempDir::new().unwrap();
        let mem = tmp.path().join("mem");
        let r = ensure_installed(&mem).unwrap();
        assert!(r.installed);
        assert!(mem.join("agents.json").exists());
        assert!(mem.join("agents").join("architect-meta.md").exists());
        let raw = fs::read_to_string(mem.join("agents.json")).unwrap();
        assert!(raw.ends_with('\n'));
        let cfg: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(cfg["agents"]["architect"]["category"], "meta");
    }

    #[test]
    fn ensure_idempotent() {
        let tmp = TempDir::new().unwrap();
        let mem = tmp.path().to_path_buf();
        ensure_installed(&mem).unwrap();
        let raw1 = fs::read(mem.join("agents.json")).unwrap();
        let ctx1 = fs::read(mem.join("agents").join("architect-meta.md")).unwrap();
        ensure_installed(&mem).unwrap();
        let raw2 = fs::read(mem.join("agents.json")).unwrap();
        let ctx2 = fs::read(mem.join("agents").join("architect-meta.md")).unwrap();
        assert_eq!(raw1, raw2);
        assert_eq!(ctx1, ctx2);
    }

    #[test]
    fn ensure_preserves_existing_context_file() {
        let tmp = TempDir::new().unwrap();
        let mem = tmp.path().to_path_buf();
        fs::create_dir_all(mem.join("agents")).unwrap();
        fs::write(mem.join("agents").join("architect-meta.md"), "custom!").unwrap();
        ensure_installed(&mem).unwrap();
        let got = fs::read_to_string(mem.join("agents").join("architect-meta.md")).unwrap();
        assert_eq!(got, "custom!");
    }

    #[test]
    fn ensure_adds_architect_to_existing_manifest() {
        let tmp = TempDir::new().unwrap();
        let mem = tmp.path().to_path_buf();
        fs::write(
            mem.join("agents.json"),
            r#"{"version":1,"agents":{"existing":{"category":"specialist"}}}"#,
        )
        .unwrap();
        ensure_installed(&mem).unwrap();
        let cfg: Value =
            serde_json::from_str(&fs::read_to_string(mem.join("agents.json")).unwrap()).unwrap();
        assert_eq!(cfg["agents"]["architect"]["category"], "meta");
        assert_eq!(cfg["agents"]["existing"]["category"], "specialist");
    }

    // ── generate_seed ───────────────────────────────────────────────────

    fn fixed() -> FixedClock {
        FixedClock("2026-04-19T12:00:00.000Z".into())
    }

    #[test]
    fn generate_requires_slug_and_category() {
        let tmp = TempDir::new().unwrap();
        let err =
            generate_seed(tmp.path(), &serde_json::json!({"slug": "x"}), &fixed()).unwrap_err();
        assert!(matches!(err, GenerateSeedError::MissingSlugOrCategory));

        let err = generate_seed(
            tmp.path(),
            &serde_json::json!({"category": "tech"}),
            &fixed(),
        )
        .unwrap_err();
        assert!(matches!(err, GenerateSeedError::MissingSlugOrCategory));
    }

    #[test]
    fn generate_rejects_invalid_kebab() {
        let tmp = TempDir::new().unwrap();
        let err = generate_seed(
            tmp.path(),
            &serde_json::json!({"slug":"Foo","category":"tech"}),
            &fixed(),
        )
        .unwrap_err();
        assert!(matches!(err, GenerateSeedError::InvalidKebabCase));

        let err = generate_seed(
            tmp.path(),
            &serde_json::json!({"slug":"foo_bar","category":"tech"}),
            &fixed(),
        )
        .unwrap_err();
        assert!(matches!(err, GenerateSeedError::InvalidKebabCase));
    }

    #[test]
    fn generate_creates_files() {
        let tmp = TempDir::new().unwrap();
        let r = generate_seed(
            tmp.path(),
            &serde_json::json!({
                "slug": "my-biz",
                "category": "custom",
                "label": "My Biz",
                "description": "A biz.",
                "agents": [
                    {"name": "bob", "label": "Bob", "description": "Sells stuff."}
                ]
            }),
            &fixed(),
        )
        .unwrap();
        assert!(r.ok);
        assert_eq!(r.reference, "custom/my-biz");
        assert_eq!(r.agent_count, 1);
        let d = tmp.path().join("custom").join("my-biz");
        assert!(d.join("agents.json").exists());
        assert!(d.join("meta.json").exists());
        assert!(d.join("agents").join("bob.md").exists());
    }

    #[test]
    fn generate_writes_category_meta_only_when_absent() {
        let tmp = TempDir::new().unwrap();
        let cat_dir = tmp.path().join("custom");
        fs::create_dir_all(&cat_dir).unwrap();
        fs::write(cat_dir.join("meta.json"), "preexisting").unwrap();
        generate_seed(
            tmp.path(),
            &serde_json::json!({"slug":"x","category":"custom","agents":[]}),
            &fixed(),
        )
        .unwrap();
        let cat_meta = fs::read_to_string(cat_dir.join("meta.json")).unwrap();
        assert_eq!(cat_meta, "preexisting");
    }

    #[test]
    fn generate_duplicate_errors() {
        let tmp = TempDir::new().unwrap();
        generate_seed(
            tmp.path(),
            &serde_json::json!({"slug":"x","category":"custom","agents":[]}),
            &fixed(),
        )
        .unwrap();
        let err = generate_seed(
            tmp.path(),
            &serde_json::json!({"slug":"x","category":"custom","agents":[]}),
            &fixed(),
        )
        .unwrap_err();
        assert!(matches!(err, GenerateSeedError::AlreadyExists { .. }));
    }

    #[test]
    fn generate_normalizes_agent_names() {
        let tmp = TempDir::new().unwrap();
        let r = generate_seed(
            tmp.path(),
            &serde_json::json!({
                "slug": "x",
                "category": "custom",
                "agents": [
                    {"name": "Bob-Smith"},
                    {"name": "foo.bar"},
                ]
            }),
            &fixed(),
        )
        .unwrap();
        assert!(r.dir.join("agents").join("bob_smith.md").exists());
        assert!(r.dir.join("agents").join("foo_bar.md").exists());
    }

    #[test]
    fn generate_skips_nameless_agents() {
        let tmp = TempDir::new().unwrap();
        let r = generate_seed(
            tmp.path(),
            &serde_json::json!({
                "slug": "x",
                "category": "custom",
                "agents": [
                    {"name": "bob"},
                    {"label": "no name"},
                    {"name": ""},
                    {"name": null},
                ]
            }),
            &fixed(),
        )
        .unwrap();
        assert_eq!(r.agent_count, 1);
    }

    #[test]
    fn generate_merges_best_practices_order() {
        let tmp = TempDir::new().unwrap();
        let r = generate_seed(
            tmp.path(),
            &serde_json::json!({
                "slug": "x",
                "category": "custom",
                "best_practices": ["alpha", "beta"],
                "agents": [
                    {"name":"a", "best_practices": ["gamma", "alpha"]},
                    {"name":"b", "best_practices": ["delta"]},
                ]
            }),
            &fixed(),
        )
        .unwrap();
        let pack_meta: Value =
            serde_json::from_str(&fs::read_to_string(r.dir.join("meta.json")).unwrap()).unwrap();
        let bps: Vec<&str> = pack_meta["best_practices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(bps, vec!["alpha", "beta", "gamma", "delta"]);
    }

    #[test]
    fn kebab_validation_strict() {
        assert!(is_kebab_case("tech"));
        assert!(is_kebab_case("a-b-c"));
        assert!(is_kebab_case("a1"));
        assert!(!is_kebab_case(""));
        assert!(!is_kebab_case("A"));
        assert!(!is_kebab_case("a_b"));
        assert!(!is_kebab_case("a b"));
    }

    // ── format_iso_millis smoke ─────────────────────────────────────────

    #[test]
    fn iso_epoch_zero() {
        assert_eq!(format_iso_millis(0), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn iso_known_moment() {
        // 2026-04-19T12:00:00.000Z → 1776600000 s = 1776600000000 ms.
        assert_eq!(
            format_iso_millis(1_776_600_000_000),
            "2026-04-19T12:00:00.000Z"
        );
    }

    #[test]
    fn iso_with_millis() {
        assert_eq!(format_iso_millis(1_234), "1970-01-01T00:00:01.234Z");
    }
}
