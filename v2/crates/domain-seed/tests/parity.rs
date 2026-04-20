//! Parity tests — compara a porta Rust com `plugin/scripts/seed.cjs`
//! invocado via `node` em subprocess. Foco nas funções de
//! discovery/filter/apply usando manifests sem best_practices/skills/ide —
//! essas dependências viriam de outros `.cjs` cujos `__dirname` não são
//! interceptáveis sem duplicar o plugin tree inteiro em tmpdir.

use domain_seed::{
    apply, filter_agents, list_categories, list_packs, load_pack_ref, read_meta, ApplyInput,
};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::tempdir;

const NODE_SCRIPT: &str = r#"
const path = require('path');
const fs = require('fs');
const vm = require('vm');

const cjsPath = process.argv[1];
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const req = JSON.parse(input);
    const src = fs.readFileSync(cjsPath, 'utf8');
    // SEEDS_DIR é path.join(__dirname, '..', 'seeds') — injetamos fake
    // __dirname pra apontar pro seeds do teste.
    const fakeDirname = req.seedsDir
      ? path.join(req.seedsDir, '..', 'scripts')
      : path.dirname(cjsPath);
    const ctx = {
      module: { exports: {} }, exports: {}, require,
      __dirname: fakeDirname, __filename: cjsPath, process,
    };
    ctx.module.exports = ctx.exports;
    ctx.global = ctx; ctx.globalThis = ctx;
    vm.createContext(ctx);
    vm.runInContext(src, ctx);
    const mod = ctx.module.exports;

    let r;
    if (req.op === 'listCategories') {
      r = mod.listCategories();
    } else if (req.op === 'listPacks') {
      r = mod.listPacks(req.category);
    } else if (req.op === 'loadPackRef') {
      try { r = mod.loadPackRef(req.ref); }
      catch (e) { r = { _throw: e.message }; }
    } else if (req.op === 'readMeta') {
      // readMeta não é exportada; extrai via vm
      vm.runInContext('globalThis.__rm = readMeta;', ctx);
      r = ctx.__rm(req.dir);
    } else if (req.op === 'filterAgents') {
      vm.runInContext('globalThis.__fa = filterAgents;', ctx);
      r = ctx.__fa(req.agents, req.only, req.exclude);
    } else if (req.op === 'apply') {
      try {
        r = mod.apply({
          ref: req.ref,
          projectRoot: req.projectRoot,
          only: req.only,
          exclude: req.exclude,
          overwrite: !!req.overwrite,
        });
      } catch (e) { r = { _throw: e.message }; }
    }
    // loadPackRef retorna uma ref com dir (absoluto): comparação só pelo shape relevante
    if (r && req.op === 'loadPackRef' && !r._throw) {
      r = { category: r.category, pack: r.pack, hasManifest: !!r.manifest, metaKeys: Object.keys(r.meta || {}) };
    }
    // apply retorna memDir absoluto e subcampos pesados — normaliza
    if (r && req.op === 'apply' && !r._throw) {
      r = {
        added: r.added, replaced: r.replaced, skipped: r.skipped,
        mdCopied: r.mdCopied, mdSkipped: r.mdSkipped,
        practicesCopied: (r.practices && r.practices.copied) || [],
        practicesPreserved: (r.practices && r.practices.preserved) || [],
        practicesMissing: (r.practices && r.practices.missing) || [],
      };
    }
    process.stdout.write(JSON.stringify(r === undefined ? null : r));
  } catch (e) {
    process.stderr.write('THROW: ' + e.message + '\n' + e.stack);
    process.exit(3);
  }
});
"#;

fn cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/seed.cjs")
        .canonicalize()
        .expect("seed.cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_node(input: Value) -> Value {
    let cjs = cjs_path();
    let mut child = Command::new("node")
        .current_dir(cjs.parent().unwrap())
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(&cjs)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    if !out.status.success() {
        panic!("node failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    serde_json::from_slice(&out.stdout).unwrap()
}

fn write(p: &Path, content: &str) {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(p, content).unwrap();
}

fn seed_min(tmp: &Path) {
    write(
        &tmp.join("seeds/dev/meta.json"),
        r#"{"label":"Dev","description":"Engenharia"}"#,
    );
    let manifest = json!({
        "manager": "manager",
        "oracle": "oracle",
        "rule": "rule-x",
        "agents": {
            "coder": { "name": "coder", "model": "sonnet" },
            "reviewer": { "name": "reviewer", "model": "opus" }
        }
    });
    write(
        &tmp.join("seeds/dev/base/agents.json"),
        &serde_json::to_string_pretty(&manifest).unwrap(),
    );
    write(&tmp.join("seeds/dev/base/agents/coder.md"), "# coder");
    write(&tmp.join("seeds/dev/base/agents/reviewer.md"), "# reviewer");
    write(&tmp.join("seeds/dev/base/meta.json"), r#"{"label":"Base"}"#);
}

// ── readMeta ─────────────────────────────────────────────────────────────

#[test]
fn parity_read_meta_missing() {
    if !node_available() {
        return;
    }
    let tmp = tempdir().unwrap();
    let js = run_node(json!({"op": "readMeta", "dir": tmp.path().to_string_lossy()}));
    let rs = read_meta(tmp.path());
    assert_eq!(js, serde_json::to_value(&rs).unwrap());
}

#[test]
fn parity_read_meta_parses_json() {
    if !node_available() {
        return;
    }
    let tmp = tempdir().unwrap();
    write(
        &tmp.path().join("meta.json"),
        r#"{"label":"x","tags":["a","b"]}"#,
    );
    let js = run_node(json!({"op": "readMeta", "dir": tmp.path().to_string_lossy()}));
    let rs = read_meta(tmp.path());
    assert_eq!(js, serde_json::to_value(&rs).unwrap());
}

#[test]
fn parity_read_meta_malformed() {
    if !node_available() {
        return;
    }
    let tmp = tempdir().unwrap();
    write(&tmp.path().join("meta.json"), "not json");
    let js = run_node(json!({"op": "readMeta", "dir": tmp.path().to_string_lossy()}));
    let rs = read_meta(tmp.path());
    assert_eq!(js, serde_json::to_value(&rs).unwrap());
}

// ── listCategories ──────────────────────────────────────────────────────

#[test]
fn parity_list_categories_with_meta() {
    if !node_available() {
        return;
    }
    let tmp = tempdir().unwrap();
    seed_min(tmp.path());
    let seeds = tmp.path().join("seeds");
    let js = run_node(json!({
        "op": "listCategories",
        "seedsDir": seeds.to_string_lossy(),
    }));
    let rs = list_categories(&seeds);
    let rs_v: Value = serde_json::to_value(&rs).unwrap();
    assert_eq!(js, rs_v);
}

#[test]
fn parity_list_categories_empty() {
    if !node_available() {
        return;
    }
    let tmp = tempdir().unwrap();
    let seeds = tmp.path().join("seeds");
    fs::create_dir_all(&seeds).unwrap();
    let js = run_node(json!({
        "op": "listCategories",
        "seedsDir": seeds.to_string_lossy(),
    }));
    let rs = list_categories(&seeds);
    let rs_v: Value = serde_json::to_value(&rs).unwrap();
    assert_eq!(js, rs_v);
}

// ── listPacks ───────────────────────────────────────────────────────────

fn strip_pack_for_compare(v: Value) -> Value {
    // normaliza: remove `dir` (path absoluto) que diverge entre JS/RS tmpdirs,
    // comparando apenas `name`, `meta`, `manifest`, `agentNames`.
    if let Value::Array(items) = v {
        Value::Array(
            items
                .into_iter()
                .map(|mut item| {
                    if let Value::Object(ref mut obj) = item {
                        obj.remove("dir");
                    }
                    item
                })
                .collect(),
        )
    } else {
        v
    }
}

#[test]
fn parity_list_packs() {
    if !node_available() {
        return;
    }
    let tmp = tempdir().unwrap();
    seed_min(tmp.path());
    let seeds = tmp.path().join("seeds");
    let js_raw = run_node(json!({
        "op": "listPacks",
        "seedsDir": seeds.to_string_lossy(),
        "category": "dev",
    }));
    let js = strip_pack_for_compare(js_raw);

    let rs = list_packs(&seeds, "dev");
    // Converte Pack → Value compatível com o shape do .cjs (camelCase agentNames)
    let rs_v = Value::Array(
        rs.iter()
            .map(|p| {
                json!({
                    "name": p.name,
                    "meta": p.meta,
                    "manifest": p.manifest,
                    "agentNames": p.agent_names,
                })
            })
            .collect(),
    );
    assert_eq!(js, rs_v);
}

// ── loadPackRef ─────────────────────────────────────────────────────────

#[test]
fn parity_load_pack_ref_ok() {
    if !node_available() {
        return;
    }
    let tmp = tempdir().unwrap();
    seed_min(tmp.path());
    let seeds = tmp.path().join("seeds");
    let js = run_node(json!({
        "op": "loadPackRef",
        "seedsDir": seeds.to_string_lossy(),
        "ref": "dev/base",
    }));
    let rs = load_pack_ref(&seeds, "dev/base").unwrap();
    let rs_v = json!({
        "category": rs.category,
        "pack": rs.pack,
        "hasManifest": true,
        "metaKeys": rs.meta.keys().cloned().collect::<Vec<_>>(),
    });
    assert_eq!(js, rs_v);
}

#[test]
fn parity_load_pack_ref_invalid_format() {
    if !node_available() {
        return;
    }
    let tmp = tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("seeds")).unwrap();
    let seeds = tmp.path().join("seeds");
    let js = run_node(json!({
        "op": "loadPackRef",
        "seedsDir": seeds.to_string_lossy(),
        "ref": "invalid",
    }));
    // Ambos lados erram com mensagens distintas mas o shape `{_throw: ...}`
    // é o que importa pra semântica.
    assert!(js.get("_throw").is_some());
    let rs = load_pack_ref(&seeds, "invalid");
    assert!(rs.is_err());
}

// ── filterAgents ────────────────────────────────────────────────────────

fn cmp_filter(only: Option<&str>, exclude: Option<&str>) {
    let agents = json!({
        "a": { "v": 1 },
        "b": { "v": 2 },
        "c": { "v": 3 }
    });
    let js = run_node(json!({
        "op": "filterAgents",
        "agents": agents,
        "only": only,
        "exclude": exclude,
    }));
    let map = agents.as_object().unwrap().clone();
    let rs = filter_agents(&map, only, exclude);
    assert_eq!(js, serde_json::to_value(&rs).unwrap());
}

#[test]
fn parity_filter_agents_passthrough() {
    if !node_available() {
        return;
    }
    cmp_filter(None, None);
}

#[test]
fn parity_filter_agents_only() {
    if !node_available() {
        return;
    }
    cmp_filter(Some("a,c"), None);
}

#[test]
fn parity_filter_agents_exclude() {
    if !node_available() {
        return;
    }
    cmp_filter(None, Some("b"));
}

#[test]
fn parity_filter_agents_both() {
    if !node_available() {
        return;
    }
    cmp_filter(Some("a,b"), Some("b"));
}

#[test]
fn parity_filter_agents_empty_strings() {
    if !node_available() {
        return;
    }
    cmp_filter(Some(""), Some(""));
}

// ── apply ───────────────────────────────────────────────────────────────
// Nota: só testamos cenários sem best_practices/skills/ide — as deps cross-
// module ficam no mock do plugin real e não rodamos no tmpdir.

fn normalize_apply_rs(r: &domain_seed::ApplyResult) -> Value {
    json!({
        "added": r.added,
        "replaced": r.replaced,
        "skipped": r.skipped,
        "mdCopied": r.md_copied,
        "mdSkipped": r.md_skipped,
        "practicesCopied": r.practices.copied,
        "practicesPreserved": r.practices.preserved,
        "practicesMissing": r.practices.missing,
    })
}

#[test]
fn parity_apply_fresh() {
    if !node_available() {
        return;
    }
    // Setup JS
    let tmp_js = tempdir().unwrap();
    seed_min(tmp_js.path());
    let proj_js = tmp_js.path().join("proj");
    fs::create_dir_all(&proj_js).unwrap();
    let js = run_node(json!({
        "op": "apply",
        "seedsDir": tmp_js.path().join("seeds").to_string_lossy(),
        "ref": "dev/base",
        "projectRoot": proj_js.to_string_lossy(),
    }));

    // Setup RS
    let tmp_rs = tempdir().unwrap();
    seed_min(tmp_rs.path());
    let bp_rs = tmp_rs.path().join("best-practices");
    fs::create_dir_all(&bp_rs).unwrap();
    let proj_rs = tmp_rs.path().join("proj");
    fs::create_dir_all(&proj_rs).unwrap();
    let r = apply(ApplyInput {
        seeds_dir: &tmp_rs.path().join("seeds"),
        bp_dir: &bp_rs,
        reference: "dev/base",
        project_root: &proj_rs,
        mem_dir_name: ".claude-project-memory",
        only: None,
        exclude: None,
        overwrite: false,
        skills: None,
        ide: None,
    })
    .unwrap();
    assert_eq!(js, normalize_apply_rs(&r));
}

#[test]
fn parity_apply_with_filter() {
    if !node_available() {
        return;
    }
    let tmp_js = tempdir().unwrap();
    seed_min(tmp_js.path());
    let proj_js = tmp_js.path().join("proj");
    fs::create_dir_all(&proj_js).unwrap();
    let js = run_node(json!({
        "op": "apply",
        "seedsDir": tmp_js.path().join("seeds").to_string_lossy(),
        "ref": "dev/base",
        "projectRoot": proj_js.to_string_lossy(),
        "only": "coder",
    }));

    let tmp_rs = tempdir().unwrap();
    seed_min(tmp_rs.path());
    let bp_rs = tmp_rs.path().join("best-practices");
    fs::create_dir_all(&bp_rs).unwrap();
    let proj_rs = tmp_rs.path().join("proj");
    fs::create_dir_all(&proj_rs).unwrap();
    let r = apply(ApplyInput {
        seeds_dir: &tmp_rs.path().join("seeds"),
        bp_dir: &bp_rs,
        reference: "dev/base",
        project_root: &proj_rs,
        mem_dir_name: ".claude-project-memory",
        only: Some("coder"),
        exclude: None,
        overwrite: false,
        skills: None,
        ide: None,
    })
    .unwrap();
    assert_eq!(js, normalize_apply_rs(&r));
}

#[test]
fn parity_apply_preserves_existing() {
    if !node_available() {
        return;
    }
    let tmp_js = tempdir().unwrap();
    seed_min(tmp_js.path());
    let proj_js = tmp_js.path().join("proj");
    fs::create_dir_all(&proj_js).unwrap();
    // primeiro run (JS)
    let _ = run_node(json!({
        "op": "apply",
        "seedsDir": tmp_js.path().join("seeds").to_string_lossy(),
        "ref": "dev/base",
        "projectRoot": proj_js.to_string_lossy(),
    }));
    // segundo run (JS)
    let js = run_node(json!({
        "op": "apply",
        "seedsDir": tmp_js.path().join("seeds").to_string_lossy(),
        "ref": "dev/base",
        "projectRoot": proj_js.to_string_lossy(),
    }));

    let tmp_rs = tempdir().unwrap();
    seed_min(tmp_rs.path());
    let bp_rs = tmp_rs.path().join("best-practices");
    fs::create_dir_all(&bp_rs).unwrap();
    let proj_rs = tmp_rs.path().join("proj");
    fs::create_dir_all(&proj_rs).unwrap();
    let _ = apply(ApplyInput {
        seeds_dir: &tmp_rs.path().join("seeds"),
        bp_dir: &bp_rs,
        reference: "dev/base",
        project_root: &proj_rs,
        mem_dir_name: ".claude-project-memory",
        only: None,
        exclude: None,
        overwrite: false,
        skills: None,
        ide: None,
    })
    .unwrap();
    let r = apply(ApplyInput {
        seeds_dir: &tmp_rs.path().join("seeds"),
        bp_dir: &bp_rs,
        reference: "dev/base",
        project_root: &proj_rs,
        mem_dir_name: ".claude-project-memory",
        only: None,
        exclude: None,
        overwrite: false,
        skills: None,
        ide: None,
    })
    .unwrap();
    assert_eq!(js, normalize_apply_rs(&r));
}

#[test]
fn parity_apply_generates_identical_agents_json() {
    if !node_available() {
        return;
    }
    let tmp_js = tempdir().unwrap();
    seed_min(tmp_js.path());
    let proj_js = tmp_js.path().join("proj");
    fs::create_dir_all(&proj_js).unwrap();
    let _ = run_node(json!({
        "op": "apply",
        "seedsDir": tmp_js.path().join("seeds").to_string_lossy(),
        "ref": "dev/base",
        "projectRoot": proj_js.to_string_lossy(),
    }));
    let js_manifest: Value = serde_json::from_str(
        &fs::read_to_string(proj_js.join(".claude-project-memory/agents.json")).unwrap(),
    )
    .unwrap();

    let tmp_rs = tempdir().unwrap();
    seed_min(tmp_rs.path());
    let bp_rs = tmp_rs.path().join("best-practices");
    fs::create_dir_all(&bp_rs).unwrap();
    let proj_rs = tmp_rs.path().join("proj");
    fs::create_dir_all(&proj_rs).unwrap();
    let _ = apply(ApplyInput {
        seeds_dir: &tmp_rs.path().join("seeds"),
        bp_dir: &bp_rs,
        reference: "dev/base",
        project_root: &proj_rs,
        mem_dir_name: ".claude-project-memory",
        only: None,
        exclude: None,
        overwrite: false,
        skills: None,
        ide: None,
    })
    .unwrap();
    let rs_manifest: Value = serde_json::from_str(
        &fs::read_to_string(proj_rs.join(".claude-project-memory/agents.json")).unwrap(),
    )
    .unwrap();

    assert_eq!(js_manifest, rs_manifest);
}
