//! Rotas `/api/seeds/*` — paridade com `server.cjs:2365-2407`.
//!
//! 2 rotas:
//! - `GET  /api/seeds` — lista categorias + packs + agent names
//! - `POST /api/seeds/apply` — aplica pack no `mem_dir` do projeto

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use domain_seed::{apply, list_categories, list_packs, ApplyInput, SkillsFilter};
use serde_json::{json, Map, Value};

use crate::{json_error, json_response, AppState};

const MEM_DIR_NAME: &str = ".claude-project-memory";

pub fn seeds_router() -> Router<AppState> {
    Router::new()
        .route("/api/seeds", get(seeds_list))
        .route("/api/seeds/apply", post(seeds_apply))
}

fn seeds_dir_of(state: &AppState) -> std::path::PathBuf {
    state.plugin_dir.join("seeds")
}

fn bp_dir_of(state: &AppState) -> std::path::PathBuf {
    state.plugin_dir.join("best-practices")
}

async fn seeds_list(State(st): State<AppState>) -> Response {
    let seeds_dir = seeds_dir_of(&st);
    let cats: Vec<Value> = list_categories(&seeds_dir)
        .into_iter()
        .map(|c| {
            let label = c
                .meta
                .get("label")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or(&c.name)
                .to_string();
            let description = c
                .meta
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let packs: Vec<Value> = list_packs(&seeds_dir, &c.name)
                .into_iter()
                .map(|p| {
                    let plabel = p
                        .meta
                        .get("label")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .unwrap_or(&p.name)
                        .to_string();
                    let pdesc = p
                        .meta
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    json!({
                        "name": p.name,
                        "label": plabel,
                        "description": pdesc,
                        "agents": p.agent_names,
                    })
                })
                .collect();
            json!({
                "name": c.name,
                "label": label,
                "description": description,
                "packs": packs,
            })
        })
        .collect();
    json_response(
        StatusCode::OK,
        json!({
            "categories": cats,
            "project_root": st.project_root.to_string_lossy().as_ref(),
        }),
    )
}

async fn seeds_apply(State(st): State<AppState>, body: Bytes) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let reference = v.get("ref").and_then(Value::as_str).unwrap_or_default();
    if reference.is_empty() || !reference.contains('/') {
        return json_error(
            StatusCode::BAD_REQUEST,
            "need ref as \"<categoria>/<pack>\"",
        );
    }
    // `only`/`exclude`: aceita array OR string.
    let only = csv_field(v.get("only"));
    let exclude = csv_field(v.get("exclude"));
    let overwrite = v.get("overwrite").and_then(Value::as_bool).unwrap_or(false);
    // `addNew`: quando true, agentes que colidem com nomes existentes são
    // instalados com suffix (manager → manager-2/3/...). Mutuamente exclusivo
    // com `overwrite` na UI; backend prioriza addNew se ambos vierem true.
    let add_new = v.get("addNew").and_then(Value::as_bool).unwrap_or(false);
    // `skills`: pode ser array, bool, ou ausente.
    let skills_val = v.get("skills");
    let skills_names_owned: Option<Vec<String>> = skills_val.and_then(|sv| match sv {
        Value::Array(arr) => Some(
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect(),
        ),
        _ => None,
    });
    let skills = match skills_val {
        None | Some(Value::Null) => None,
        Some(Value::Bool(false)) => None,
        Some(Value::Array(_)) => skills_names_owned
            .as_ref()
            .map(|v| SkillsFilter::Only(v.clone())),
        _ => Some(SkillsFilter::All),
    };
    let ide = v
        .get("ide")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());

    let seeds_dir = seeds_dir_of(&st);
    let bp_dir = bp_dir_of(&st);

    let input = ApplyInput {
        seeds_dir: &seeds_dir,
        bp_dir: &bp_dir,
        reference,
        project_root: &st.project_root,
        mem_dir_name: MEM_DIR_NAME,
        only: only.as_deref(),
        exclude: exclude.as_deref(),
        overwrite,
        add_new,
        skills,
        ide,
    };
    match apply(input) {
        Ok(r) => json_response(StatusCode::OK, apply_result_to_json(&r)),
        Err(e) => json_error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

/// `only`/`exclude` aceitam array ou string. Retorna `None` se vazio.
fn csv_field(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Array(arr)) => {
            let s = arr
                .iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(",");
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        }
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// Serializa [`domain_seed::ApplyResult`] no shape do `.cjs`:
/// `{ ok, added, replaced, skipped, mdCopied, mdSkipped, memDir, practices,
///    skills, ide }`.
fn apply_result_to_json(r: &domain_seed::ApplyResult) -> Value {
    let mut out = Map::new();
    out.insert("ok".into(), Value::Bool(true));
    out.insert("added".into(), to_str_array(&r.added));
    out.insert("replaced".into(), to_str_array(&r.replaced));
    out.insert("skipped".into(), to_str_array(&r.skipped));
    out.insert("mdCopied".into(), to_str_array(&r.md_copied));
    out.insert("mdSkipped".into(), to_str_array(&r.md_skipped));
    out.insert(
        "memDir".into(),
        Value::String(r.mem_dir.to_string_lossy().into_owned()),
    );
    out.insert(
        "practices".into(),
        json!({
            "copied": r.practices.copied,
            "preserved": r.practices.preserved,
            "missing": r.practices.missing,
        }),
    );
    out.insert(
        "skills".into(),
        json!({
            "copied": r.skills.copied,
            "preserved": r.skills.preserved,
            "mcp_added": r.skills.mcp_added,
            "mcp_preserved": r.skills.mcp_preserved,
        }),
    );
    out.insert(
        "ide".into(),
        match &r.ide {
            Some(i) => json!({ "written": i.written }),
            None => Value::Null,
        },
    );
    Value::Object(out)
}

fn to_str_array(v: &[String]) -> Value {
    Value::Array(v.iter().cloned().map(Value::String).collect())
}
