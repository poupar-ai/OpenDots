//! Domain-pipelines — porta de `plugin/scripts/pipelines.cjs`.
//!
//! Fluxos reproduzíveis com checkpoints. Os templates vivem em
//! `plugin/pipelines/<id>/pipeline.yaml` e são copiados para
//! `<memDir>/pipelines/<id>/` quando instalados. O estado de uma run ativa
//! fica em `<memDir>/pipeline-state.json`; o log histórico em
//! `<memDir>/pipeline-runs.jsonl`.
//!
//! # Diferenças com o `.cjs`
//! - `PIPE_DIR` era `__dirname/../pipelines` no Node. Aqui é parâmetro
//!   `templates_dir: &Path` nas funções que o usam.
//! - Todas as operações de estado aceitam `clock: &dyn Clock` para testar
//!   transições sem depender do relógio real.

use std::fs;
use std::path::{Path, PathBuf};

use domain_graph::Clock;
use regex::Regex;
use serde_json::{json, Map, Value};
use thiserror::Error;

pub use domain_graph::{FixedClock, SystemClock};

// ── Errors ──────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Validation(String),
    #[error("pipeline \"{0}\" não existe em templates")]
    TemplateMissing(String),
    #[error("pipeline \"{0}\" não encontrada")]
    PipelineMissing(String),
    #[error("já existe uma pipeline rodando: {0}")]
    AlreadyRunning(String),
    #[error("sem pipeline rodando")]
    NoRun,
    #[error("pipeline não está rodando (status: {0})")]
    NotRunning(String),
    #[error("step \"{0}\" não existe nessa pipeline")]
    StepMissing(String),
    #[error("step \"{requested}\" não é o atual (atual: {current})")]
    StepNotCurrent { requested: String, current: String },
    #[error("pipeline \"{0}\" já existe em {1} — use overwrite")]
    AlreadyExists(String, String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

// ── YAML parser mínimo ──────────────────────────────────────────────────

/// Parsea o subset YAML específico do schema `pipeline.yaml`. Retorna
/// `Value::Object` com insertion order preservada (via feature
/// `preserve_order` do `serde_json`). Paridade com
/// `parsePipelineYaml` em `pipelines.cjs:20`.
pub fn parse_pipeline_yaml(text: &str) -> Value {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Map::new();
    out.insert("steps".to_string(), Value::Array(Vec::new()));
    let mut current_step: Option<Map<String, Value>> = None;
    let mut block: Option<Block> = None;
    let mut in_steps = false;

    let kv_re = Regex::new(r"^(\s*)(\w+):\s*(.*)$").unwrap();
    let step_re = Regex::new(r"^\s*-\s+id:\s*(.+)$").unwrap();
    let steps_header_re = Regex::new(r"^steps:\s*$").unwrap();
    let arr_re = Regex::new(r"^\[(.*)\]$").unwrap();
    let dq_re = Regex::new(r#"^".*"$"#).unwrap();
    let sq_re = Regex::new(r"^'.*'$").unwrap();

    for raw in lines {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.trim().is_empty() {
            if let Some(b) = block.as_mut() {
                b.buf.push(String::new());
            }
            continue;
        }
        if line.trim_start().starts_with('#') {
            continue;
        }

        if let Some(b) = block.as_mut() {
            let indent = count_leading_ws(line);
            if indent >= b.indent {
                b.buf.push(line.chars().skip(b.indent).collect::<String>());
                continue;
            }
            flush_block(&mut block, &mut out, &mut current_step);
        }

        if let Some(caps) = step_re.captures(line) {
            if in_steps {
                if let Some(cs) = current_step.take() {
                    out.get_mut("steps")
                        .and_then(|v| v.as_array_mut())
                        .unwrap()
                        .push(Value::Object(cs));
                }
                let mut ns = Map::new();
                ns.insert("id".to_string(), Value::String(caps[1].trim().to_string()));
                current_step = Some(ns);
                continue;
            }
        }

        if steps_header_re.is_match(line) {
            in_steps = true;
            continue;
        }

        let Some(kv) = kv_re.captures(line) else {
            continue;
        };
        let indent = kv[1].len();
        let key = kv[2].to_string();
        let mut val = kv[3].to_string();

        let is_step_field = in_steps && current_step.is_some() && indent >= 4;

        if val == "|" {
            block = Some(Block {
                target_key: key,
                indent: indent + 2,
                buf: Vec::new(),
                owner: if is_step_field {
                    BlockOwner::Step
                } else {
                    BlockOwner::Out
                },
            });
            continue;
        }
        if let Some(arr_caps) = arr_re.captures(&val) {
            let items: Vec<Value> = arr_caps[1]
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .map(Value::String)
                .collect();
            insert_field(
                is_step_field,
                &mut current_step,
                &mut out,
                &key,
                Value::Array(items),
            );
            continue;
        }
        if dq_re.is_match(&val) || sq_re.is_match(&val) {
            // Remove primeira e última aspa (qualquer uma das duas).
            val = val[1..val.len() - 1].to_string();
        }
        insert_field(
            is_step_field,
            &mut current_step,
            &mut out,
            &key,
            Value::String(val),
        );
    }
    flush_block(&mut block, &mut out, &mut current_step);
    if let Some(cs) = current_step {
        out.get_mut("steps")
            .and_then(|v| v.as_array_mut())
            .unwrap()
            .push(Value::Object(cs));
    }
    Value::Object(out)
}

fn count_leading_ws(s: &str) -> usize {
    s.chars()
        .take_while(|c| c.is_whitespace() && *c != '\n')
        .count()
}

struct Block {
    target_key: String,
    indent: usize,
    buf: Vec<String>,
    owner: BlockOwner,
}

#[derive(PartialEq)]
enum BlockOwner {
    Out,
    Step,
}

fn flush_block(
    block: &mut Option<Block>,
    out: &mut Map<String, Value>,
    current_step: &mut Option<Map<String, Value>>,
) {
    let Some(b) = block.take() else { return };
    let value = b.buf.join("\n").trim().to_string();
    match b.owner {
        BlockOwner::Out => {
            out.insert(b.target_key, Value::String(value));
        }
        BlockOwner::Step => {
            if let Some(cs) = current_step.as_mut() {
                cs.insert(b.target_key, Value::String(value));
            }
        }
    }
}

fn insert_field(
    is_step_field: bool,
    current_step: &mut Option<Map<String, Value>>,
    out: &mut Map<String, Value>,
    key: &str,
    val: Value,
) {
    if is_step_field {
        if let Some(cs) = current_step.as_mut() {
            cs.insert(key.to_string(), val);
        }
    } else {
        out.insert(key.to_string(), val);
    }
}

// ── Discovery ───────────────────────────────────────────────────────────

/// Lista templates presentes em `templates_dir`. Paridade com
/// `listTemplates` em `pipelines.cjs:87`.
pub fn list_templates(templates_dir: &Path) -> Vec<Value> {
    list_dir(templates_dir, "template")
}

/// Lista pipelines instaladas em `<mem_dir>/pipelines/`. Paridade com
/// `listInstalled` em `pipelines.cjs:114`.
pub fn list_installed(mem_dir: &Path) -> Vec<Value> {
    list_dir(&mem_dir.join("pipelines"), "installed")
}

fn list_dir(dir: &Path, source: &str) -> Vec<Value> {
    if !dir.exists() {
        return Vec::new();
    }
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<String> = read
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                && e.path().join("pipeline.yaml").exists()
        })
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    entries.sort(); // paridade: fs.readdirSync é sorted em Linux
    entries
        .into_iter()
        .filter_map(|name| {
            let file = dir.join(&name).join("pipeline.yaml");
            let raw = fs::read_to_string(&file).ok()?;
            // parse nunca falha (retorna Object mesmo com erro silencioso)
            let def = parse_pipeline_yaml(&raw);
            Some(summarize_def(&def, &name, source))
        })
        .collect()
}

fn summarize_def(def: &Value, fallback_name: &str, source: &str) -> Value {
    let id = def
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or(fallback_name);
    let label = def
        .get("label")
        .and_then(|v| v.as_str())
        .unwrap_or(fallback_name);
    let description = def
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let tags = def.get("tags").cloned().unwrap_or(Value::Array(Vec::new()));
    let steps = def
        .get("steps")
        .cloned()
        .unwrap_or(Value::Array(Vec::new()));
    let step_count = steps.as_array().map(|a| a.len()).unwrap_or(0);
    json!({
        "id": id,
        "label": label,
        "description": description,
        "tags": tags,
        "steps": steps,
        "stepCount": step_count,
        "source": source,
    })
}

/// Combina installed + templates com mesmo id — installed vence. Paridade
/// com `list` em `pipelines.cjs:142`.
pub fn list(templates_dir: &Path, mem_dir: &Path) -> Vec<Value> {
    let templates = list_templates(templates_dir);
    let installed = list_installed(mem_dir);
    let installed_ids: std::collections::HashSet<String> = installed
        .iter()
        .filter_map(|p| p.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect();
    let mut out = installed;
    for t in templates {
        let id = t
            .get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_default();
        if !installed_ids.contains(&id) {
            out.push(t);
        }
    }
    out
}

/// Carrega a definição completa (parsed YAML) de um pipeline — prefere
/// installed, fallback para template. Paridade com `loadDef` em
/// `pipelines.cjs:153`.
pub fn load_def(templates_dir: &Path, mem_dir: &Path, id: &str) -> Option<Value> {
    let installed_file = mem_dir.join("pipelines").join(id).join("pipeline.yaml");
    let tpl_file = templates_dir.join(id).join("pipeline.yaml");
    let file = if installed_file.exists() {
        installed_file
    } else {
        tpl_file
    };
    let raw = fs::read_to_string(&file).ok()?;
    Some(parse_pipeline_yaml(&raw))
}

// ── Install ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum InstallResult {
    Installed,
    Exists,
}

/// Copia o template do `templates_dir` para `<mem_dir>/pipelines/<id>/`.
/// Paridade com `install` em `pipelines.cjs:163`.
pub fn install(
    templates_dir: &Path,
    mem_dir: &Path,
    id: &str,
    overwrite: bool,
) -> Result<InstallResult> {
    let src = templates_dir.join(id).join("pipeline.yaml");
    if !src.exists() {
        return Err(Error::TemplateMissing(id.to_string()));
    }
    let dst_dir = mem_dir.join("pipelines").join(id);
    fs::create_dir_all(&dst_dir)?;
    let dst = dst_dir.join("pipeline.yaml");
    if dst.exists() && !overwrite {
        return Ok(InstallResult::Exists);
    }
    fs::copy(&src, &dst)?;
    Ok(InstallResult::Installed)
}

// ── Create (via UI) ─────────────────────────────────────────────────────

fn id_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[a-z][a-z0-9-]{1,48}[a-z0-9]$").unwrap())
}

fn special_chars_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[:\[\]#]|^-").unwrap())
}

/// Equivalente a `yamlEscape` em `pipelines.cjs:178`. Retorna `None` se o
/// valor precisa de bloco (tem `\n`). Retorna `Some(String)` com o valor
/// "pronto pra uso" no YAML.
pub fn yaml_escape(s: &Value) -> Option<String> {
    let raw = match s {
        Value::Null => return Some(String::new()),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    };
    if raw.contains('\n') {
        return None;
    }
    if special_chars_re().is_match(&raw) {
        return Some(serde_json::to_string(&raw).unwrap());
    }
    Some(raw)
}

fn yaml_escape_fallback_str(s: &str) -> Option<String> {
    yaml_escape(&Value::String(s.to_string()))
}

/// Equivalente a `esc !== null ? esc : JSON.stringify(...)` que o `.cjs`
/// usa várias vezes — com fallback para quoted quando há `\n`.
fn yaml_scalar_or_quoted(s: &str) -> String {
    match yaml_escape_fallback_str(s) {
        Some(v) => v,
        None => serde_json::to_string(s).unwrap(),
    }
}

/// Serializa um pipeline para YAML conforme schema. Paridade com
/// `serializePipeline` em `pipelines.cjs:187`.
pub fn serialize_pipeline(def: &Value) -> String {
    let mut lines: Vec<String> = Vec::new();
    let id = def.get("id").and_then(|v| v.as_str()).unwrap_or("");
    lines.push(format!("id: {id}"));

    if let Some(label) = def.get("label").and_then(|v| v.as_str()) {
        if !label.is_empty() {
            lines.push(format!("label: {}", yaml_scalar_or_quoted(label)));
        }
    }

    if let Some(description) = def.get("description").and_then(|v| v.as_str()) {
        if !description.is_empty() {
            if description.contains('\n') {
                lines.push("description: |".to_string());
                for l in description.split('\n') {
                    lines.push(format!("  {l}"));
                }
            } else {
                lines.push(format!(
                    "description: {}",
                    yaml_scalar_or_quoted(description)
                ));
            }
        }
    }

    if let Some(tags) = def.get("tags").and_then(|v| v.as_array()) {
        if !tags.is_empty() {
            let joined = tags
                .iter()
                .filter_map(|t| t.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("tags: [{joined}]"));
        }
    }

    lines.push("steps:".to_string());
    if let Some(steps) = def.get("steps").and_then(|v| v.as_array()) {
        for s in steps {
            let s_id = s.get("id").and_then(|v| v.as_str()).unwrap_or("");
            lines.push(format!("  - id: {s_id}"));
            if let Some(t) = s.get("type").and_then(|v| v.as_str()) {
                if !t.is_empty() && t != "agent" {
                    lines.push(format!("    type: {t}"));
                }
            }
            if let Some(a) = s.get("agent").and_then(|v| v.as_str()) {
                if !a.is_empty() {
                    lines.push(format!("    agent: {a}"));
                }
            }
            if let Some(lbl) = s.get("label").and_then(|v| v.as_str()) {
                if !lbl.is_empty() {
                    lines.push(format!("    label: {}", yaml_scalar_or_quoted(lbl)));
                }
            }
            if let Some(d) = s.get("description").and_then(|v| v.as_str()) {
                if !d.is_empty() {
                    if d.contains('\n') {
                        lines.push("    description: |".to_string());
                        for l in d.split('\n') {
                            lines.push(format!("      {l}"));
                        }
                    } else {
                        lines.push(format!("    description: {}", yaml_scalar_or_quoted(d)));
                    }
                }
            }
            if let Some(o) = s.get("owner").and_then(|v| v.as_str()) {
                if !o.is_empty() {
                    lines.push(format!("    owner: {o}"));
                }
            }
            if let Some(bps) = s.get("best_practices").and_then(|v| v.as_array()) {
                if !bps.is_empty() {
                    let joined = bps
                        .iter()
                        .filter_map(|t| t.as_str().map(|s| s.trim().to_string()))
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(", ");
                    lines.push(format!("    best_practices: [{joined}]"));
                }
            }
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Valida input de pipeline (vindo da UI ou API). Paridade com
/// `validatePipelineInput` em `pipelines.cjs:229`.
pub fn validate_pipeline_input(input: &Value) -> Result<()> {
    let obj = input
        .as_object()
        .ok_or_else(|| Error::Validation("payload inválido".into()))?;
    let id = obj.get("id").and_then(|v| v.as_str()).ok_or_else(|| {
        Error::Validation("id deve ser slug a-z/0-9/- (3-50 chars, começa com letra)".into())
    })?;
    if !id_re().is_match(id) {
        return Err(Error::Validation(
            "id deve ser slug a-z/0-9/- (3-50 chars, começa com letra)".into(),
        ));
    }
    let label = obj
        .get("label")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    // JS: `!label || typeof label !== 'string' || label.length > 120`.
    // UTF-16 length — usa encode_utf16.
    if label.is_empty() || label.encode_utf16().count() > 120 {
        return Err(Error::Validation(
            "label é obrigatório (máx 120 chars)".into(),
        ));
    }
    let steps = obj
        .get("steps")
        .and_then(|v| v.as_array())
        .ok_or_else(|| Error::Validation("pipeline precisa de pelo menos 1 step".into()))?;
    if steps.is_empty() {
        return Err(Error::Validation(
            "pipeline precisa de pelo menos 1 step".into(),
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for s in steps {
        let so = s
            .as_object()
            .ok_or_else(|| Error::Validation("step inválido".into()))?;
        let step_id = so.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        if !id_re().is_match(step_id) {
            return Err(Error::Validation(format!(
                "step.id \"{step_id}\" inválido (use slug a-z/0-9/-)"
            )));
        }
        if seen.contains(step_id) {
            return Err(Error::Validation(format!(
                "step.id \"{step_id}\" duplicado"
            )));
        }
        seen.insert(step_id.to_string());
        let typ = so.get("type").and_then(|v| v.as_str()).unwrap_or("agent");
        if typ != "agent" && typ != "checkpoint" {
            return Err(Error::Validation(format!(
                "step.type inválido em \"{step_id}\" (use agent ou checkpoint)"
            )));
        }
        if typ == "agent"
            && so
                .get("agent")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty()
        {
            return Err(Error::Validation(format!(
                "step \"{step_id}\" do tipo agent precisa de agent"
            )));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreateResult {
    pub id: String,
    pub scope: String,
    pub path: PathBuf,
}

/// Cria um pipeline novo. `scope` = "template" escreve em `templates_dir`,
/// qualquer outro valor cai em "project" → `<mem_dir>/pipelines/`. Paridade
/// com `create` em `pipelines.cjs:259`.
pub fn create(
    templates_dir: &Path,
    mem_dir: &Path,
    scope: &str,
    input: &Value,
    overwrite: bool,
) -> Result<CreateResult> {
    validate_pipeline_input(input)?;
    let scope_name = if scope == "template" {
        "template"
    } else {
        "project"
    };
    let base_dir = if scope_name == "template" {
        templates_dir.to_path_buf()
    } else {
        mem_dir.join("pipelines")
    };
    let id = input
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let dst_dir = base_dir.join(&id);
    let dst = dst_dir.join("pipeline.yaml");
    if dst.exists() && !overwrite {
        return Err(Error::AlreadyExists(id, scope_name.to_string()));
    }
    fs::create_dir_all(&dst_dir)?;
    fs::write(&dst, serialize_pipeline(input))?;
    Ok(CreateResult {
        id,
        scope: scope_name.to_string(),
        path: dst,
    })
}

// ── Runtime state ───────────────────────────────────────────────────────

pub fn state_path(mem_dir: &Path) -> PathBuf {
    mem_dir.join("pipeline-state.json")
}

pub fn runs_path(mem_dir: &Path) -> PathBuf {
    mem_dir.join("pipeline-runs.jsonl")
}

/// Extrai um resumo da run para gravar em `pipeline-runs.jsonl`. Paridade
/// com `summarizeRun` em `pipelines.cjs:285`.
pub fn summarize_run(state: &Value) -> Value {
    let steps = state
        .get("steps")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let done_count = steps
        .iter()
        .filter(|s| s.get("status").and_then(|v| v.as_str()) == Some("done"))
        .count();
    let rejected_count = steps
        .iter()
        .filter(|s| s.get("status").and_then(|v| v.as_str()) == Some("rejected"))
        .count();
    let started_ms = state
        .get("started_at")
        .and_then(|v| v.as_str())
        .and_then(parse_iso_to_ms);
    let ended_ms = state
        .get("updated_at")
        .and_then(|v| v.as_str())
        .and_then(parse_iso_to_ms);
    let duration_ms = match (started_ms, ended_ms) {
        (Some(s), Some(e)) => Value::from(std::cmp::max(0, e - s)),
        _ => Value::Null,
    };
    json!({
        "run_id": state.get("run_id").cloned().unwrap_or(Value::Null),
        "pipeline_id": state.get("pipeline_id").cloned().unwrap_or(Value::Null),
        "title": state.get("title").cloned().unwrap_or(Value::Null),
        "status": state.get("status").cloned().unwrap_or(Value::Null),
        "started_at": state.get("started_at").cloned().unwrap_or(Value::Null),
        "ended_at": state.get("updated_at").cloned().unwrap_or(Value::Null),
        "started_by": state.get("started_by").cloned().unwrap_or(Value::Null),
        "total_steps": state.get("total_steps").cloned().unwrap_or(Value::Null),
        "steps_done": done_count,
        "steps_rejected": rejected_count,
        "duration_ms": duration_ms,
        "cancel_reason": state.get("cancel_reason").cloned().unwrap_or(Value::Null),
    })
}

fn parse_iso_to_ms(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    if bytes.len() < 10 {
        return None;
    }
    let year: i64 = s.get(..4)?.parse().ok()?;
    let month: i64 = s.get(5..7)?.parse().ok()?;
    let day: i64 = s.get(8..10)?.parse().ok()?;
    let (hour, min, sec, ms) = if bytes.len() >= 19 {
        let h: i64 = s.get(11..13)?.parse().ok()?;
        let mi: i64 = s.get(14..16)?.parse().ok()?;
        let sec: i64 = s.get(17..19)?.parse().ok()?;
        let ms: i64 = if bytes.len() >= 23 && &bytes[19..20] == b"." {
            s.get(20..23).and_then(|x| x.parse().ok()).unwrap_or(0)
        } else {
            0
        };
        (h, mi, sec, ms)
    } else {
        (0, 0, 0, 0)
    };
    let days = days_from_civil(year, month, day);
    Some(days * 86_400_000 + hour * 3_600_000 + min * 60_000 + sec * 1_000 + ms)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn append_run(mem_dir: &Path, state: &Value) -> Result<()> {
    let run_id = state
        .get("run_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if run_id.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(mem_dir)?;
    let entry = summarize_run(state);
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(runs_path(mem_dir))?;
    let mut line = serde_json::to_string(&entry).unwrap();
    line.push('\n');
    f.write_all(line.as_bytes())?;
    Ok(())
}

/// Lista runs mais recentes (reverse + cap). Paridade com `listRuns` em
/// `pipelines.cjs:316`.
pub fn list_runs(mem_dir: &Path, limit: usize) -> Vec<Value> {
    let f = runs_path(mem_dir);
    let Ok(raw) = fs::read_to_string(&f) else {
        return Vec::new();
    };
    let mut runs: Vec<Value> = raw
        .split('\n')
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    runs.reverse();
    let cap = limit.clamp(1, 500);
    runs.into_iter().take(cap).collect()
}

pub fn load_state(mem_dir: &Path) -> Option<Value> {
    let f = state_path(mem_dir);
    let raw = fs::read_to_string(&f).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn save_state(mem_dir: &Path, state: &Value) -> Result<()> {
    fs::create_dir_all(mem_dir)?;
    let mut body = serde_json::to_string_pretty(state).unwrap();
    body.push('\n');
    fs::write(state_path(mem_dir), body)?;
    Ok(())
}

// ── State machine ───────────────────────────────────────────────────────

/// Converte ISO `2026-01-02T12:34:56.789Z` para o sufixo usado em `run_id`
/// (`2026-01-02T12-34-56`). Paridade com
/// `now.replace(/[:.]/g, '-').slice(0, 19)`.
fn run_id_suffix(iso: &str) -> String {
    let replaced: String = iso
        .chars()
        .map(|c| if c == ':' || c == '.' { '-' } else { c })
        .collect();
    slice_utf16(&replaced, 19)
}

fn slice_utf16(s: &str, n: usize) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    if units.len() <= n {
        return s.to_string();
    }
    String::from_utf16_lossy(&units[..n])
}

pub fn start(
    templates_dir: &Path,
    mem_dir: &Path,
    id: &str,
    title: Option<&str>,
    started_by: Option<&str>,
    clock: &dyn Clock,
) -> Result<Value> {
    let def = load_def(templates_dir, mem_dir, id)
        .ok_or_else(|| Error::PipelineMissing(id.to_string()))?;
    if let Some(existing) = load_state(mem_dir) {
        if existing.get("status").and_then(|v| v.as_str()) == Some("running") {
            let pid = existing
                .get("pipeline_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            return Err(Error::AlreadyRunning(pid));
        }
    }
    let now = clock.now_iso();
    let run_id = format!("{id}-{}", run_id_suffix(&now));
    let def_label = def.get("label").and_then(|v| v.as_str()).unwrap_or(id);
    let resolved_title = title.unwrap_or("");
    let final_title = if resolved_title.is_empty() {
        def_label
    } else {
        resolved_title
    };
    let empty_steps: Vec<Value> = Vec::new();
    let def_steps = def
        .get("steps")
        .and_then(|v| v.as_array())
        .unwrap_or(&empty_steps);
    let total_steps = def_steps.len();
    let mut steps_out: Vec<Value> = Vec::with_capacity(total_steps);
    for (i, s) in def_steps.iter().enumerate() {
        let s_id = s.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let s_label = s.get("label").and_then(|v| v.as_str()).unwrap_or(s_id);
        let s_desc = s.get("description").and_then(|v| v.as_str()).unwrap_or("");
        let s_agent = s.get("agent").cloned().unwrap_or(Value::Null);
        let s_agent = if matches!(s_agent, Value::String(ref a) if a.is_empty()) {
            Value::Null
        } else {
            s_agent
        };
        let s_type = s
            .get("type")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("agent")
            .to_string();
        let s_owner = s.get("owner").cloned().unwrap_or(Value::Null);
        let s_owner = if matches!(s_owner, Value::String(ref a) if a.is_empty()) {
            Value::Null
        } else {
            s_owner
        };
        let s_bps = s
            .get("best_practices")
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        let (status, started_at) = if i == 0 {
            ("active", Value::String(now.clone()))
        } else {
            ("pending", Value::Null)
        };
        steps_out.push(json!({
            "index": i,
            "id": s_id,
            "label": s_label,
            "description": s_desc,
            "agent": s_agent,
            "type": s_type,
            "owner": s_owner,
            "best_practices": s_bps,
            "status": status,
            "started_at": started_at,
            "finished_at": Value::Null,
            "note": Value::Null,
            "decided_by": Value::Null,
        }));
    }
    let state = json!({
        "run_id": run_id,
        "pipeline_id": id,
        "title": final_title,
        "status": "running",
        "started_at": now,
        "started_by": started_by.map(Value::from).unwrap_or(Value::Null),
        "updated_at": now,
        "current_step": 0,
        "total_steps": total_steps,
        "steps": steps_out,
    });
    save_state(mem_dir, &state)?;
    Ok(state)
}

pub fn advance(
    mem_dir: &Path,
    step_id: &str,
    note: Option<&str>,
    decided_by: Option<&str>,
    decision: Option<&str>,
    clock: &dyn Clock,
) -> Result<Value> {
    let mut state = load_state(mem_dir).ok_or(Error::NoRun)?;
    let status = state
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if status != "running" {
        return Err(Error::NotRunning(status));
    }
    let current_idx = state
        .get("current_step")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let steps_len = state
        .get("steps")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let idx = state
        .get("steps")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter()
                .position(|s| s.get("id").and_then(|v| v.as_str()) == Some(step_id))
        })
        .ok_or_else(|| Error::StepMissing(step_id.to_string()))?;
    if idx != current_idx {
        let current_id = state
            .get("steps")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.get(current_idx))
            .and_then(|s| s.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        return Err(Error::StepNotCurrent {
            requested: step_id.to_string(),
            current: current_id,
        });
    }

    let now = clock.now_iso();
    let note_val = note
        .filter(|s| !s.is_empty())
        .map(|s| Value::String(s.to_string()))
        .unwrap_or(Value::Null);
    let decided_by_val = decided_by
        .filter(|s| !s.is_empty())
        .map(|s| Value::String(s.to_string()))
        .unwrap_or(Value::Null);

    let steps = state
        .get_mut("steps")
        .and_then(|v| v.as_array_mut())
        .unwrap();
    let step = &mut steps[idx];

    if decision == Some("reject") {
        step["status"] = Value::String("rejected".into());
        step["finished_at"] = Value::String(now.clone());
        step["note"] = note_val;
        step["decided_by"] = decided_by_val;
        state["status"] = Value::String("aborted".into());
        state["updated_at"] = Value::String(now.clone());
        save_state(mem_dir, &state)?;
        append_run(mem_dir, &state)?;
        return Ok(state);
    }

    step["status"] = Value::String("done".into());
    step["finished_at"] = Value::String(now.clone());
    step["note"] = note_val;
    step["decided_by"] = decided_by_val;

    let next_idx = idx + 1;
    if next_idx >= steps_len {
        state["status"] = Value::String("done".into());
    } else {
        state["current_step"] = Value::from(next_idx);
        let steps = state
            .get_mut("steps")
            .and_then(|v| v.as_array_mut())
            .unwrap();
        steps[next_idx]["status"] = Value::String("active".into());
        steps[next_idx]["started_at"] = Value::String(now.clone());
    }
    state["updated_at"] = Value::String(now.clone());
    save_state(mem_dir, &state)?;
    if state.get("status").and_then(|v| v.as_str()) == Some("done") {
        append_run(mem_dir, &state)?;
    }
    Ok(state)
}

pub fn cancel(mem_dir: &Path, reason: Option<&str>, clock: &dyn Clock) -> Result<Option<Value>> {
    let Some(mut state) = load_state(mem_dir) else {
        return Ok(None);
    };
    let was_running = state.get("status").and_then(|v| v.as_str()) == Some("running");
    state["status"] = Value::String("cancelled".into());
    state["updated_at"] = Value::String(clock.now_iso());
    if let Some(r) = reason {
        if !r.is_empty() {
            state["cancel_reason"] = Value::String(r.to_string());
        }
    }
    save_state(mem_dir, &state)?;
    if was_running {
        append_run(mem_dir, &state)?;
    }
    Ok(Some(state))
}

pub fn clear(mem_dir: &Path) -> Result<()> {
    let f = state_path(mem_dir);
    if f.exists() {
        fs::remove_file(f)?;
    }
    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // ── parse_pipeline_yaml ─────────────────────────────────────────────

    #[test]
    fn parse_yaml_basic() {
        let yaml = "id: p1\nlabel: Pipeline 1\nsteps:\n  - id: s1\n    agent: frontend\n";
        let v = parse_pipeline_yaml(yaml);
        assert_eq!(v["id"], "p1");
        assert_eq!(v["label"], "Pipeline 1");
        assert_eq!(v["steps"][0]["id"], "s1");
        assert_eq!(v["steps"][0]["agent"], "frontend");
    }

    #[test]
    fn parse_yaml_tags_inline_array() {
        let yaml = "id: p1\ntags: [a, b, c]\nsteps:\n  - id: s1\n";
        let v = parse_pipeline_yaml(yaml);
        assert_eq!(v["tags"], json!(["a", "b", "c"]));
    }

    #[test]
    fn parse_yaml_block_scalar() {
        let yaml = "id: p1\ndescription: |\n  linha 1\n  linha 2\nsteps:\n  - id: s1\n";
        let v = parse_pipeline_yaml(yaml);
        assert_eq!(v["description"], "linha 1\nlinha 2");
    }

    #[test]
    fn parse_yaml_quoted_strings() {
        let yaml = "id: p1\nlabel: \"hello: world\"\nsteps:\n  - id: s1\n";
        let v = parse_pipeline_yaml(yaml);
        assert_eq!(v["label"], "hello: world");
    }

    #[test]
    fn parse_yaml_comments_ignored() {
        let yaml = "# comentário\nid: p1\n# mais comentário\nsteps:\n  - id: s1\n";
        let v = parse_pipeline_yaml(yaml);
        assert_eq!(v["id"], "p1");
    }

    #[test]
    fn parse_yaml_multiple_steps() {
        let yaml = "id: p\nsteps:\n  - id: a\n    agent: x\n  - id: b\n    type: checkpoint\n";
        let v = parse_pipeline_yaml(yaml);
        let steps = v["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0]["id"], "a");
        assert_eq!(steps[1]["type"], "checkpoint");
    }

    // ── serialize_pipeline ──────────────────────────────────────────────

    #[test]
    fn serialize_roundtrip_minimal() {
        let def = json!({
            "id": "p1",
            "label": "label",
            "steps": [{"id": "s1", "agent": "frontend"}]
        });
        let yaml = serialize_pipeline(&def);
        let parsed = parse_pipeline_yaml(&yaml);
        assert_eq!(parsed["id"], "p1");
        assert_eq!(parsed["label"], "label");
        assert_eq!(parsed["steps"][0]["id"], "s1");
        assert_eq!(parsed["steps"][0]["agent"], "frontend");
    }

    #[test]
    fn serialize_ends_with_newline() {
        let def = json!({"id": "p", "label": "l", "steps": [{"id": "s"}]});
        let yaml = serialize_pipeline(&def);
        assert!(yaml.ends_with('\n'));
    }

    #[test]
    fn serialize_omits_type_agent() {
        let def = json!({
            "id": "p", "label": "l",
            "steps": [{"id": "s1", "type": "agent", "agent": "x"}]
        });
        let yaml = serialize_pipeline(&def);
        assert!(!yaml.contains("type: agent"));
    }

    #[test]
    fn serialize_keeps_type_checkpoint() {
        let def = json!({
            "id": "p", "label": "l",
            "steps": [{"id": "s1", "type": "checkpoint"}]
        });
        let yaml = serialize_pipeline(&def);
        assert!(yaml.contains("type: checkpoint"));
    }

    #[test]
    fn serialize_multiline_description_block() {
        let def = json!({
            "id": "p", "label": "l",
            "description": "linha 1\nlinha 2",
            "steps": [{"id": "s1"}]
        });
        let yaml = serialize_pipeline(&def);
        assert!(yaml.contains("description: |"));
        assert!(yaml.contains("  linha 1"));
        assert!(yaml.contains("  linha 2"));
    }

    #[test]
    fn serialize_tags_inline() {
        let def = json!({
            "id": "p", "label": "l",
            "tags": ["a", "b"],
            "steps": [{"id": "s1"}]
        });
        let yaml = serialize_pipeline(&def);
        assert!(yaml.contains("tags: [a, b]"));
    }

    // ── validate_pipeline_input ─────────────────────────────────────────

    #[test]
    fn validate_rejects_bad_id() {
        let r = validate_pipeline_input(&json!({"id": "AB"}));
        assert!(r.is_err());
    }

    #[test]
    fn validate_rejects_missing_label() {
        let r = validate_pipeline_input(&json!({"id": "abc"}));
        assert!(r.is_err());
    }

    #[test]
    fn validate_rejects_empty_steps() {
        let r = validate_pipeline_input(&json!({"id": "abc", "label": "L", "steps": []}));
        assert!(r.is_err());
    }

    #[test]
    fn validate_rejects_duplicate_step_ids() {
        let r = validate_pipeline_input(&json!({
            "id": "abc", "label": "L",
            "steps": [
                {"id": "dup", "agent": "a"},
                {"id": "dup", "agent": "b"},
            ]
        }));
        assert!(r.is_err());
        assert!(format!("{}", r.unwrap_err()).contains("duplicado"));
    }

    #[test]
    fn validate_rejects_agent_step_without_agent() {
        let r = validate_pipeline_input(&json!({
            "id": "abc", "label": "L",
            "steps": [{"id": "step-one"}]
        }));
        assert!(r.is_err());
        assert!(format!("{}", r.unwrap_err()).contains("precisa de agent"));
    }

    #[test]
    fn validate_accepts_checkpoint_without_agent() {
        let r = validate_pipeline_input(&json!({
            "id": "abc", "label": "L",
            "steps": [{"id": "step-one", "type": "checkpoint"}]
        }));
        assert!(r.is_ok());
    }

    #[test]
    fn validate_rejects_label_over_120_chars() {
        let long = "a".repeat(121);
        let r = validate_pipeline_input(&json!({
            "id": "abc", "label": long,
            "steps": [{"id": "step-one", "agent": "x"}]
        }));
        assert!(r.is_err());
    }

    // ── State machine ──────────────────────────────────────────────────

    fn write_template(dir: &Path, id: &str, yaml: &str) {
        let p = dir.join(id);
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("pipeline.yaml"), yaml).unwrap();
    }

    #[test]
    fn start_creates_running_state() {
        let tpl = tempdir().unwrap();
        let mem = tempdir().unwrap();
        write_template(
            tpl.path(),
            "p1",
            "id: p1\nlabel: L\nsteps:\n  - id: s1\n    agent: a\n  - id: s2\n    type: checkpoint\n",
        );
        let clock = FixedClock(0);
        let state = start(tpl.path(), mem.path(), "p1", None, Some("tiago"), &clock).unwrap();
        assert_eq!(state["status"], "running");
        assert_eq!(state["pipeline_id"], "p1");
        assert_eq!(state["total_steps"], 2);
        assert_eq!(state["steps"][0]["status"], "active");
        assert_eq!(state["steps"][1]["status"], "pending");
        assert_eq!(state["started_by"], "tiago");
    }

    #[test]
    fn start_errors_when_already_running() {
        let tpl = tempdir().unwrap();
        let mem = tempdir().unwrap();
        write_template(
            tpl.path(),
            "p1",
            "id: p1\nlabel: L\nsteps:\n  - id: s1\n    agent: a\n",
        );
        let clock = FixedClock(0);
        start(tpl.path(), mem.path(), "p1", None, None, &clock).unwrap();
        let err = start(tpl.path(), mem.path(), "p1", None, None, &clock).unwrap_err();
        assert!(matches!(err, Error::AlreadyRunning(_)));
    }

    #[test]
    fn advance_marks_done_and_activates_next() {
        let tpl = tempdir().unwrap();
        let mem = tempdir().unwrap();
        write_template(
            tpl.path(),
            "p1",
            "id: p1\nlabel: L\nsteps:\n  - id: s1\n    agent: a\n  - id: s2\n    agent: b\n",
        );
        let clock = FixedClock(1_700_000_000_000);
        start(tpl.path(), mem.path(), "p1", None, None, &clock).unwrap();
        let s = advance(mem.path(), "s1", Some("ok"), Some("tiago"), None, &clock).unwrap();
        assert_eq!(s["status"], "running");
        assert_eq!(s["current_step"], 1);
        assert_eq!(s["steps"][0]["status"], "done");
        assert_eq!(s["steps"][1]["status"], "active");
    }

    #[test]
    fn advance_finishes_pipeline_on_last_step() {
        let tpl = tempdir().unwrap();
        let mem = tempdir().unwrap();
        write_template(
            tpl.path(),
            "p1",
            "id: p1\nlabel: L\nsteps:\n  - id: s1\n    agent: a\n",
        );
        let clock = FixedClock(0);
        start(tpl.path(), mem.path(), "p1", None, None, &clock).unwrap();
        let s = advance(mem.path(), "s1", None, None, None, &clock).unwrap();
        assert_eq!(s["status"], "done");
        // run registrada
        let runs = list_runs(mem.path(), 10);
        assert_eq!(runs.len(), 1);
    }

    #[test]
    fn advance_reject_aborts_and_logs_run() {
        let tpl = tempdir().unwrap();
        let mem = tempdir().unwrap();
        write_template(
            tpl.path(),
            "p1",
            "id: p1\nlabel: L\nsteps:\n  - id: s1\n    agent: a\n  - id: s2\n    agent: b\n",
        );
        let clock = FixedClock(0);
        start(tpl.path(), mem.path(), "p1", None, None, &clock).unwrap();
        let s = advance(
            mem.path(),
            "s1",
            Some("nope"),
            Some("tiago"),
            Some("reject"),
            &clock,
        )
        .unwrap();
        assert_eq!(s["status"], "aborted");
        assert_eq!(s["steps"][0]["status"], "rejected");
        assert_eq!(list_runs(mem.path(), 10).len(), 1);
    }

    #[test]
    fn advance_rejects_out_of_order_step() {
        let tpl = tempdir().unwrap();
        let mem = tempdir().unwrap();
        write_template(
            tpl.path(),
            "p1",
            "id: p1\nlabel: L\nsteps:\n  - id: s1\n    agent: a\n  - id: s2\n    agent: b\n",
        );
        let clock = FixedClock(0);
        start(tpl.path(), mem.path(), "p1", None, None, &clock).unwrap();
        let err = advance(mem.path(), "s2", None, None, None, &clock).unwrap_err();
        assert!(matches!(err, Error::StepNotCurrent { .. }));
    }

    #[test]
    fn cancel_marks_cancelled_and_logs_when_running() {
        let tpl = tempdir().unwrap();
        let mem = tempdir().unwrap();
        write_template(
            tpl.path(),
            "p1",
            "id: p1\nlabel: L\nsteps:\n  - id: s1\n    agent: a\n",
        );
        let clock = FixedClock(0);
        start(tpl.path(), mem.path(), "p1", None, None, &clock).unwrap();
        let s = cancel(mem.path(), Some("user aborted"), &clock)
            .unwrap()
            .unwrap();
        assert_eq!(s["status"], "cancelled");
        assert_eq!(s["cancel_reason"], "user aborted");
        assert_eq!(list_runs(mem.path(), 10).len(), 1);
    }

    #[test]
    fn cancel_returns_none_without_state() {
        let mem = tempdir().unwrap();
        let clock = FixedClock(0);
        assert!(cancel(mem.path(), None, &clock).unwrap().is_none());
    }

    #[test]
    fn clear_removes_state_file() {
        let tpl = tempdir().unwrap();
        let mem = tempdir().unwrap();
        write_template(
            tpl.path(),
            "p1",
            "id: p1\nlabel: L\nsteps:\n  - id: s1\n    agent: a\n",
        );
        let clock = FixedClock(0);
        start(tpl.path(), mem.path(), "p1", None, None, &clock).unwrap();
        assert!(state_path(mem.path()).exists());
        clear(mem.path()).unwrap();
        assert!(!state_path(mem.path()).exists());
    }

    // ── install ─────────────────────────────────────────────────────────

    #[test]
    fn install_copies_template() {
        let tpl = tempdir().unwrap();
        let mem = tempdir().unwrap();
        write_template(
            tpl.path(),
            "p1",
            "id: p1\nlabel: L\nsteps:\n  - id: s1\n    agent: a\n",
        );
        let r = install(tpl.path(), mem.path(), "p1", false).unwrap();
        assert_eq!(r, InstallResult::Installed);
        assert!(mem.path().join("pipelines/p1/pipeline.yaml").exists());
    }

    #[test]
    fn install_returns_exists_when_not_overwrite() {
        let tpl = tempdir().unwrap();
        let mem = tempdir().unwrap();
        write_template(
            tpl.path(),
            "p1",
            "id: p1\nlabel: L\nsteps:\n  - id: s1\n    agent: a\n",
        );
        install(tpl.path(), mem.path(), "p1", false).unwrap();
        let r = install(tpl.path(), mem.path(), "p1", false).unwrap();
        assert_eq!(r, InstallResult::Exists);
    }

    #[test]
    fn install_missing_template_errors() {
        let tpl = tempdir().unwrap();
        let mem = tempdir().unwrap();
        let err = install(tpl.path(), mem.path(), "nope", false).unwrap_err();
        assert!(matches!(err, Error::TemplateMissing(_)));
    }

    // ── list / load_def ─────────────────────────────────────────────────

    #[test]
    fn list_prefers_installed_over_template() {
        let tpl = tempdir().unwrap();
        let mem = tempdir().unwrap();
        write_template(
            tpl.path(),
            "p1",
            "id: p1\nlabel: Tpl\nsteps:\n  - id: s1\n    agent: a\n",
        );
        fs::create_dir_all(mem.path().join("pipelines/p1")).unwrap();
        fs::write(
            mem.path().join("pipelines/p1/pipeline.yaml"),
            "id: p1\nlabel: Installed\nsteps:\n  - id: s1\n    agent: a\n",
        )
        .unwrap();
        let items = list(tpl.path(), mem.path());
        // Só uma entrada com id p1, e é a installed.
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["source"], "installed");
        assert_eq!(items[0]["label"], "Installed");
    }

    #[test]
    fn load_def_prefers_installed() {
        let tpl = tempdir().unwrap();
        let mem = tempdir().unwrap();
        write_template(
            tpl.path(),
            "p1",
            "id: p1\nlabel: Tpl\nsteps:\n  - id: s1\n    agent: a\n",
        );
        fs::create_dir_all(mem.path().join("pipelines/p1")).unwrap();
        fs::write(
            mem.path().join("pipelines/p1/pipeline.yaml"),
            "id: p1\nlabel: Installed\nsteps:\n  - id: s1\n    agent: a\n",
        )
        .unwrap();
        let def = load_def(tpl.path(), mem.path(), "p1").unwrap();
        assert_eq!(def["label"], "Installed");
    }

    // ── summarize_run ──────────────────────────────────────────────────

    #[test]
    fn summarize_run_counts_and_duration() {
        let state = json!({
            "run_id": "r1",
            "pipeline_id": "p1",
            "title": "t",
            "status": "done",
            "started_at": "2026-04-19T12:00:00.000Z",
            "updated_at": "2026-04-19T12:00:05.000Z",
            "total_steps": 3,
            "steps": [
                {"id": "a", "status": "done"},
                {"id": "b", "status": "rejected"},
                {"id": "c", "status": "done"},
            ],
        });
        let s = summarize_run(&state);
        assert_eq!(s["steps_done"], 2);
        assert_eq!(s["steps_rejected"], 1);
        assert_eq!(s["duration_ms"], 5000);
    }

    #[test]
    fn list_runs_reverse_and_limit() {
        let mem = tempdir().unwrap();
        for i in 0..5 {
            let state = json!({
                "run_id": format!("r{i}"),
                "pipeline_id": "p",
                "title": "t",
                "status": "done",
                "started_at": "2026-04-19T00:00:00.000Z",
                "updated_at": "2026-04-19T00:00:01.000Z",
                "total_steps": 1,
                "steps": [{"id": "a", "status": "done"}],
            });
            append_run(mem.path(), &state).unwrap();
        }
        let runs = list_runs(mem.path(), 3);
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0]["run_id"], "r4");
        assert_eq!(runs[1]["run_id"], "r3");
    }

    // ── yaml_escape ─────────────────────────────────────────────────────

    #[test]
    fn yaml_escape_plain() {
        assert_eq!(
            yaml_escape(&Value::String("hello".into())),
            Some("hello".into())
        );
    }

    #[test]
    fn yaml_escape_special_chars_quoted() {
        assert_eq!(
            yaml_escape(&Value::String("hello: world".into())),
            Some("\"hello: world\"".into())
        );
    }

    #[test]
    fn yaml_escape_newline_returns_none() {
        assert_eq!(yaml_escape(&Value::String("a\nb".into())), None);
    }

    #[test]
    fn yaml_escape_null_returns_empty() {
        assert_eq!(yaml_escape(&Value::Null), Some(String::new()));
    }

    #[test]
    fn run_id_suffix_replaces_colons_and_dots() {
        assert_eq!(
            run_id_suffix("2026-04-19T12:34:56.789Z"),
            "2026-04-19T12-34-56"
        );
    }

    // ── create ─────────────────────────────────────────────────────────

    #[test]
    fn create_project_scope_writes_yaml() {
        let tpl = tempdir().unwrap();
        let mem = tempdir().unwrap();
        let input = json!({
            "id": "novo-pipe",
            "label": "Novo Pipe",
            "steps": [{"id": "step-one", "agent": "a"}]
        });
        let r = create(tpl.path(), mem.path(), "project", &input, false).unwrap();
        assert_eq!(r.id, "novo-pipe");
        assert_eq!(r.scope, "project");
        assert!(r.path.exists());
    }

    #[test]
    fn create_refuses_overwrite_by_default() {
        let tpl = tempdir().unwrap();
        let mem = tempdir().unwrap();
        let input = json!({
            "id": "novo-pipe",
            "label": "L",
            "steps": [{"id": "step-one", "agent": "a"}]
        });
        create(tpl.path(), mem.path(), "project", &input, false).unwrap();
        let err = create(tpl.path(), mem.path(), "project", &input, false).unwrap_err();
        assert!(matches!(err, Error::AlreadyExists(_, _)));
    }
}
