//! Parity tests — compara a porta Rust com `plugin/scripts/pipelines.cjs`
//! invocado via `node` em subprocess. Pula silenciosamente se `node` não
//! está no PATH.

use domain_pipelines::{
    advance, cancel, create, install, list, list_installed, list_runs, load_def,
    parse_pipeline_yaml, serialize_pipeline, start, summarize_run, validate_pipeline_input,
    yaml_escape, FixedClock, InstallResult,
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
const cjsPath = process.argv[1];
// Bootstrapping: carregar pipelines.cjs permitindo que templatesDir seja
// sobreposto para o teste isolado. O .cjs deriva PIPE_DIR de __dirname no
// load-time, então precisamos interceptar via copy pro caminho templatesDir
// informado.
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const req = JSON.parse(input);
    // Se op exige override de PIPE_DIR, carregamos via vm com __dirname
    // manipulado.
    const vm = require('vm');
    const src = fs.readFileSync(cjsPath, 'utf8');
    const fakeDirname = req.templatesDir
      ? path.join(req.templatesDir, '..', 'scripts')
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
    const op = req.op;
    if (op === 'parseYaml') {
      vm.runInContext('globalThis.__p = parsePipelineYaml;', ctx);
      r = ctx.__p(req.text);
    } else if (op === 'serialize') {
      vm.runInContext('globalThis.__s = serializePipeline;', ctx);
      r = ctx.__s(req.def);
    } else if (op === 'yamlEscape') {
      vm.runInContext('globalThis.__ye = yamlEscape;', ctx);
      const v = ctx.__ye(req.value);
      r = v === null ? { _null: true } : { value: v };
    } else if (op === 'validate') {
      vm.runInContext('globalThis.__v = validatePipelineInput;', ctx);
      try { ctx.__v(req.input); r = { ok: true }; }
      catch (e) { r = { ok: false, message: e.message }; }
    } else if (op === 'listTemplates') {
      r = mod.listTemplates();
    } else if (op === 'listInstalled') {
      r = mod.listInstalled(req.memDir);
    } else if (op === 'list') {
      r = mod.list(req.memDir);
    } else if (op === 'loadDef') {
      r = mod.loadDef(req.memDir, req.id);
    } else if (op === 'install') {
      try {
        r = mod.install({ id: req.id, memDir: req.memDir, overwrite: !!req.overwrite });
      } catch (e) { r = { _throw: e.message }; }
    } else if (op === 'create') {
      try {
        r = mod.create({ memDir: req.memDir, scope: req.scope, input: req.input, overwrite: !!req.overwrite });
      } catch (e) { r = { _throw: e.message }; }
    } else if (op === 'start') {
      try {
        r = mod.start({ memDir: req.memDir, id: req.id, title: req.title, startedBy: req.startedBy });
      } catch (e) { r = { _throw: e.message }; }
    } else if (op === 'advance') {
      try {
        r = mod.advance({ memDir: req.memDir, stepId: req.stepId, note: req.note, decidedBy: req.decidedBy, decision: req.decision });
      } catch (e) { r = { _throw: e.message }; }
    } else if (op === 'cancel') {
      r = mod.cancel({ memDir: req.memDir, reason: req.reason });
    } else if (op === 'listRuns') {
      r = mod.listRuns(req.memDir, req.limit);
    } else if (op === 'summarizeState') {
      // Usar o estado em disco que acabamos de salvar.
      const state = JSON.parse(fs.readFileSync(path.join(req.memDir, 'pipeline-state.json'), 'utf8'));
      // summarizeRun não é exportada — extrair via vm.
      vm.runInContext('globalThis.__sr = summarizeRun;', ctx);
      r = ctx.__sr(state);
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
        .join("../../../plugin/scripts/pipelines.cjs")
        .canonicalize()
        .expect("pipelines.cjs missing")
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

// ── parse_pipeline_yaml ─────────────────────────────────────────────────

fn cmp_parse(yaml: &str) {
    let js = run_node(json!({"op": "parseYaml", "text": yaml}));
    let rs = parse_pipeline_yaml(yaml);
    assert_eq!(js, rs, "parseYaml diverge para:\n{yaml}");
}

#[test]
fn parity_parse_yaml_basic() {
    if !node_available() {
        return;
    }
    cmp_parse("id: p1\nlabel: Pipeline 1\nsteps:\n  - id: step-a\n    agent: frontend\n");
}

#[test]
fn parity_parse_yaml_tags_inline() {
    if !node_available() {
        return;
    }
    cmp_parse("id: p1\ntags: [a, b, c]\nsteps:\n  - id: step-a\n");
}

#[test]
fn parity_parse_yaml_block_scalar() {
    if !node_available() {
        return;
    }
    cmp_parse("id: p1\ndescription: |\n  linha 1\n  linha 2\nsteps:\n  - id: step-a\n");
}

#[test]
fn parity_parse_yaml_quoted_strings() {
    if !node_available() {
        return;
    }
    cmp_parse("id: p1\nlabel: \"hello: world\"\nsteps:\n  - id: step-a\n");
}

#[test]
fn parity_parse_yaml_comments_and_empty_lines() {
    if !node_available() {
        return;
    }
    cmp_parse(
        "# topo\n\nid: p1\n# meio\n\nsteps:\n  - id: step-a\n    agent: x\n  - id: step-b\n    type: checkpoint\n",
    );
}

#[test]
fn parity_parse_yaml_step_block_scalar() {
    if !node_available() {
        return;
    }
    cmp_parse(
        "id: p1\nsteps:\n  - id: step-a\n    agent: x\n    description: |\n      detalhe 1\n      detalhe 2\n",
    );
}

#[test]
fn parity_parse_yaml_step_inline_array() {
    if !node_available() {
        return;
    }
    cmp_parse("id: p1\nsteps:\n  - id: step-a\n    agent: x\n    best_practices: [bp1, bp2]\n");
}

// ── serialize_pipeline ──────────────────────────────────────────────────

#[test]
fn parity_serialize_minimal() {
    if !node_available() {
        return;
    }
    let def = json!({
        "id": "p1",
        "label": "Label",
        "steps": [{"id": "step-a", "agent": "frontend"}]
    });
    let js = run_node(json!({"op": "serialize", "def": def}));
    let rs = serialize_pipeline(&def);
    assert_eq!(js.as_str().unwrap(), rs);
}

#[test]
fn parity_serialize_all_fields() {
    if !node_available() {
        return;
    }
    let def = json!({
        "id": "p1",
        "label": "Label com : dois pontos",
        "description": "linha 1\nlinha 2\nlinha 3",
        "tags": ["tag-a", "tag-b"],
        "steps": [
            {
                "id": "step-a",
                "type": "checkpoint",
                "label": "Passo A",
                "description": "desc\nem blocos",
                "owner": "tiago",
                "best_practices": ["bp-1", "bp-2"]
            },
            {
                "id": "step-b",
                "agent": "frontend",
                "description": "simples"
            }
        ]
    });
    let js = run_node(json!({"op": "serialize", "def": def}));
    let rs = serialize_pipeline(&def);
    assert_eq!(js.as_str().unwrap(), rs);
}

// ── yaml_escape ─────────────────────────────────────────────────────────

#[test]
fn parity_yaml_escape_cases() {
    if !node_available() {
        return;
    }
    let cases = vec![
        json!("plain"),
        json!("with: colon"),
        json!("with[bracket]"),
        json!("starts-with-letter"),
        json!("-starts-with-dash"),
        json!("has # hash"),
        json!(""),
        json!(null),
        json!("multi\nline"),
    ];
    for v in cases {
        let js = run_node(json!({"op": "yamlEscape", "value": v}));
        let rs = yaml_escape(&v);
        match rs {
            None => assert!(
                js.get("_null").and_then(|v| v.as_bool()).unwrap_or(false),
                "rs=None mas js={js:?} para value={v:?}"
            ),
            Some(ref s) => assert_eq!(
                js["value"].as_str().unwrap(),
                s,
                "yamlEscape diverge para {v:?}"
            ),
        }
    }
}

// ── validate_pipeline_input ─────────────────────────────────────────────

#[test]
fn parity_validate_various_cases() {
    if !node_available() {
        return;
    }
    let cases = vec![
        (json!({}), false),
        (json!({"id": "BAD"}), false),
        (json!({"id": "good-id"}), false), // sem label
        (
            json!({
                "id": "good-id",
                "label": "OK",
                "steps": [{"id": "step-a", "agent": "frontend"}]
            }),
            true,
        ),
        (
            json!({
                "id": "good-id",
                "label": "OK",
                "steps": [{"id": "step-a", "type": "checkpoint"}]
            }),
            true,
        ),
        (
            json!({
                "id": "good-id",
                "label": "OK",
                "steps": [
                    {"id": "step-a", "agent": "x"},
                    {"id": "step-a", "agent": "y"}
                ]
            }),
            false,
        ),
    ];
    for (input, should_ok) in cases {
        let js = run_node(json!({"op": "validate", "input": input}));
        let rs = validate_pipeline_input(&input).is_ok();
        let js_ok = js["ok"].as_bool().unwrap();
        assert_eq!(js_ok, should_ok, "JS contradiz expectativa: input={input}");
        assert_eq!(js_ok, rs, "Rust vs JS: input={input}");
    }
}

// ── Runtime flow (start/advance/cancel) ────────────────────────────────

fn setup_dirs_with_template(yaml: &str) -> (tempfile::TempDir, tempfile::TempDir) {
    let js_mem = tempdir().unwrap();
    let rs_mem = tempdir().unwrap();
    // Para JS: escrever template dentro do diretório de memória como
    // "installed" para não depender do templates_dir.
    fs::create_dir_all(js_mem.path().join("pipelines/pipe-x")).unwrap();
    fs::write(js_mem.path().join("pipelines/pipe-x/pipeline.yaml"), yaml).unwrap();
    fs::create_dir_all(rs_mem.path().join("pipelines/pipe-x")).unwrap();
    fs::write(rs_mem.path().join("pipelines/pipe-x/pipeline.yaml"), yaml).unwrap();
    (js_mem, rs_mem)
}

fn templates_dir_for_rs() -> PathBuf {
    // Aponta para plugin/pipelines real — mas vamos usar "installed" via
    // mem_dir, então este dir só é usado como fallback. Passamos um dir
    // vazio pra evitar interferência.
    tempdir().unwrap().keep()
}

fn read_state(p: &Path) -> Value {
    let raw = fs::read_to_string(p.join("pipeline-state.json")).unwrap();
    serde_json::from_str(&raw).unwrap()
}

/// Normaliza o state para comparação: zera campos time-dependentes que
/// são controlados pelo clock. Clock-based em Rust usa FixedClock, mas
/// o JS usa `new Date()` real, então normalizamos.
fn strip_timestamps(v: &mut Value) {
    if let Some(obj) = v.as_object_mut() {
        for k in ["started_at", "updated_at", "run_id"] {
            if let Some(x) = obj.get_mut(k) {
                *x = Value::String("<ts>".into());
            }
        }
        if let Some(steps) = obj.get_mut("steps").and_then(|v| v.as_array_mut()) {
            for s in steps {
                if let Some(so) = s.as_object_mut() {
                    for k in ["started_at", "finished_at"] {
                        if let Some(x) = so.get_mut(k) {
                            if !x.is_null() {
                                *x = Value::String("<ts>".into());
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn parity_start_state_shape() {
    if !node_available() {
        return;
    }
    let yaml = "id: pipe-x\nlabel: L\nsteps:\n  - id: step-a\n    agent: frontend\n  - id: step-b\n    type: checkpoint\n";
    let (js_mem, rs_mem) = setup_dirs_with_template(yaml);

    let _ = run_node(json!({
        "op": "start",
        "memDir": js_mem.path().to_str().unwrap(),
        "id": "pipe-x",
        "title": null,
        "startedBy": "tiago"
    }));
    let clock = FixedClock(1_700_000_000_000);
    let tpl = templates_dir_for_rs();
    start(&tpl, rs_mem.path(), "pipe-x", None, Some("tiago"), &clock).unwrap();

    let mut js_state = read_state(js_mem.path());
    let mut rs_state = read_state(rs_mem.path());
    strip_timestamps(&mut js_state);
    strip_timestamps(&mut rs_state);
    assert_eq!(js_state, rs_state);
}

#[test]
fn parity_advance_then_finish() {
    if !node_available() {
        return;
    }
    let yaml =
        "id: pipe-x\nlabel: L\nsteps:\n  - id: step-a\n    agent: x\n  - id: step-b\n    agent: y\n";
    let (js_mem, rs_mem) = setup_dirs_with_template(yaml);

    let _ = run_node(json!({
        "op": "start", "memDir": js_mem.path().to_str().unwrap(),
        "id": "pipe-x", "title": null, "startedBy": null
    }));
    let _ = run_node(json!({
        "op": "advance", "memDir": js_mem.path().to_str().unwrap(),
        "stepId": "step-a", "note": "ok", "decidedBy": "tiago", "decision": null
    }));
    let _ = run_node(json!({
        "op": "advance", "memDir": js_mem.path().to_str().unwrap(),
        "stepId": "step-b", "note": null, "decidedBy": null, "decision": null
    }));

    let clock = FixedClock(1_700_000_000_000);
    let tpl = templates_dir_for_rs();
    start(&tpl, rs_mem.path(), "pipe-x", None, None, &clock).unwrap();
    advance(
        rs_mem.path(),
        "step-a",
        Some("ok"),
        Some("tiago"),
        None,
        &clock,
    )
    .unwrap();
    advance(rs_mem.path(), "step-b", None, None, None, &clock).unwrap();

    let mut js_state = read_state(js_mem.path());
    let mut rs_state = read_state(rs_mem.path());
    strip_timestamps(&mut js_state);
    strip_timestamps(&mut rs_state);
    assert_eq!(js_state, rs_state);

    let js_runs = run_node(json!({
        "op": "listRuns",
        "memDir": js_mem.path().to_str().unwrap(),
        "limit": 50
    }));
    let rs_runs = list_runs(rs_mem.path(), 50);
    // Normalizar runs (timestamps + run_id + duration variam).
    let mut js_runs = js_runs.clone();
    for r in js_runs.as_array_mut().unwrap() {
        if let Some(o) = r.as_object_mut() {
            for k in ["started_at", "ended_at", "run_id", "duration_ms"] {
                if let Some(x) = o.get_mut(k) {
                    *x = Value::Null;
                }
            }
        }
    }
    let mut rs_runs: Value = Value::Array(rs_runs);
    for r in rs_runs.as_array_mut().unwrap() {
        if let Some(o) = r.as_object_mut() {
            for k in ["started_at", "ended_at", "run_id", "duration_ms"] {
                if let Some(x) = o.get_mut(k) {
                    *x = Value::Null;
                }
            }
        }
    }
    assert_eq!(js_runs, rs_runs);
}

#[test]
fn parity_advance_reject_aborts() {
    if !node_available() {
        return;
    }
    let yaml =
        "id: pipe-x\nlabel: L\nsteps:\n  - id: step-a\n    agent: x\n  - id: step-b\n    agent: y\n";
    let (js_mem, rs_mem) = setup_dirs_with_template(yaml);

    let _ = run_node(json!({
        "op": "start", "memDir": js_mem.path().to_str().unwrap(),
        "id": "pipe-x", "title": null, "startedBy": null
    }));
    let _ = run_node(json!({
        "op": "advance", "memDir": js_mem.path().to_str().unwrap(),
        "stepId": "step-a", "note": "nope", "decidedBy": "tiago", "decision": "reject"
    }));

    let clock = FixedClock(1_700_000_000_000);
    let tpl = templates_dir_for_rs();
    start(&tpl, rs_mem.path(), "pipe-x", None, None, &clock).unwrap();
    advance(
        rs_mem.path(),
        "step-a",
        Some("nope"),
        Some("tiago"),
        Some("reject"),
        &clock,
    )
    .unwrap();

    let mut js_state = read_state(js_mem.path());
    let mut rs_state = read_state(rs_mem.path());
    strip_timestamps(&mut js_state);
    strip_timestamps(&mut rs_state);
    assert_eq!(js_state["status"], "aborted");
    assert_eq!(rs_state["status"], "aborted");
    assert_eq!(js_state, rs_state);
}

#[test]
fn parity_cancel_marks_cancelled() {
    if !node_available() {
        return;
    }
    let yaml = "id: pipe-x\nlabel: L\nsteps:\n  - id: step-a\n    agent: x\n";
    let (js_mem, rs_mem) = setup_dirs_with_template(yaml);

    let _ = run_node(json!({
        "op": "start", "memDir": js_mem.path().to_str().unwrap(),
        "id": "pipe-x", "title": null, "startedBy": null
    }));
    let _ = run_node(json!({
        "op": "cancel", "memDir": js_mem.path().to_str().unwrap(),
        "reason": "user aborted"
    }));

    let clock = FixedClock(1_700_000_000_000);
    let tpl = templates_dir_for_rs();
    start(&tpl, rs_mem.path(), "pipe-x", None, None, &clock).unwrap();
    cancel(rs_mem.path(), Some("user aborted"), &clock).unwrap();

    let mut js_state = read_state(js_mem.path());
    let mut rs_state = read_state(rs_mem.path());
    strip_timestamps(&mut js_state);
    strip_timestamps(&mut rs_state);
    assert_eq!(js_state, rs_state);
}

// ── install / list ────────────────────────────────────────────────────

#[test]
fn parity_install_copies_file() {
    if !node_available() {
        return;
    }
    // Para este teste, usamos o PIPE_DIR real do cjs (templates_dir
    // natural do nudge). Escolhemos um pipeline que existe em
    // plugin/pipelines — usamos listTemplates pra descobrir e pular o
    // teste se não houver nenhum.
    let tpls = run_node(json!({"op": "listTemplates"}));
    let tpls = tpls.as_array().unwrap();
    if tpls.is_empty() {
        eprintln!("sem templates reais; pulando install parity test");
        return;
    }
    let id = tpls[0]["id"].as_str().unwrap().to_string();

    let js_mem = tempdir().unwrap();
    let rs_mem = tempdir().unwrap();
    let js = run_node(json!({
        "op": "install", "memDir": js_mem.path().to_str().unwrap(),
        "id": id, "overwrite": false
    }));
    assert_eq!(js["installed"].as_bool(), Some(true));

    // No Rust, usamos o mesmo PIPE_DIR físico do cjs como templates_dir.
    let tpl_dir = cjs_path().parent().unwrap().join("../pipelines");
    let tpl_dir = tpl_dir.canonicalize().unwrap();
    let rs = install(&tpl_dir, rs_mem.path(), &id, false).unwrap();
    assert_eq!(rs, InstallResult::Installed);

    // Arquivos devem ser idênticos em conteúdo.
    let js_file =
        fs::read_to_string(js_mem.path().join(format!("pipelines/{id}/pipeline.yaml"))).unwrap();
    let rs_file =
        fs::read_to_string(rs_mem.path().join(format!("pipelines/{id}/pipeline.yaml"))).unwrap();
    assert_eq!(js_file, rs_file);
}

#[test]
fn parity_create_writes_identical_yaml() {
    if !node_available() {
        return;
    }
    let js_mem = tempdir().unwrap();
    let rs_mem = tempdir().unwrap();
    let input = json!({
        "id": "meu-pipe",
        "label": "Meu Pipe",
        "description": "descrição\nem duas linhas",
        "tags": ["x", "y"],
        "steps": [
            {"id": "step-a", "agent": "frontend", "label": "A"},
            {"id": "step-b", "type": "checkpoint", "description": "cp"}
        ]
    });
    let js = run_node(json!({
        "op": "create", "memDir": js_mem.path().to_str().unwrap(),
        "scope": "project", "input": input, "overwrite": false
    }));
    assert!(js.get("_throw").is_none(), "JS falhou: {js:?}");

    let tpl = templates_dir_for_rs();
    create(&tpl, rs_mem.path(), "project", &input, false).unwrap();

    let js_file =
        fs::read_to_string(js_mem.path().join("pipelines/meu-pipe/pipeline.yaml")).unwrap();
    let rs_file =
        fs::read_to_string(rs_mem.path().join("pipelines/meu-pipe/pipeline.yaml")).unwrap();
    assert_eq!(js_file, rs_file);
}

#[test]
fn parity_list_installed() {
    if !node_available() {
        return;
    }
    let yaml_a = "id: pipe-a\nlabel: A\nsteps:\n  - id: step-a\n    agent: x\n";
    let yaml_b = "id: pipe-b\nlabel: B\nsteps:\n  - id: step-a\n    type: checkpoint\n";
    let js_mem = tempdir().unwrap();
    let rs_mem = tempdir().unwrap();
    for (p, y) in [("pipe-a", yaml_a), ("pipe-b", yaml_b)] {
        for mem in [&js_mem, &rs_mem] {
            let d = mem.path().join(format!("pipelines/{p}"));
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("pipeline.yaml"), y).unwrap();
        }
    }
    let js = run_node(json!({
        "op": "listInstalled", "memDir": js_mem.path().to_str().unwrap()
    }));
    let rs = list_installed(rs_mem.path());
    let rs_val = Value::Array(rs);
    assert_eq!(js, rs_val);
}

#[test]
fn parity_load_def() {
    if !node_available() {
        return;
    }
    let yaml = "id: pipe-x\nlabel: L\ndescription: |\n  multi\n  linha\nsteps:\n  - id: step-a\n    agent: x\n";
    let (js_mem, rs_mem) = setup_dirs_with_template(yaml);

    let js = run_node(json!({
        "op": "loadDef", "memDir": js_mem.path().to_str().unwrap(),
        "id": "pipe-x"
    }));
    let tpl = templates_dir_for_rs();
    let rs = load_def(&tpl, rs_mem.path(), "pipe-x").unwrap();
    assert_eq!(js, rs);
}

// ── summarize_run ───────────────────────────────────────────────────────

#[test]
fn parity_summarize_run_shape() {
    if !node_available() {
        return;
    }
    let yaml = "id: pipe-x\nlabel: L\nsteps:\n  - id: step-a\n    agent: x\n";
    let (js_mem, rs_mem) = setup_dirs_with_template(yaml);

    let _ = run_node(json!({
        "op": "start", "memDir": js_mem.path().to_str().unwrap(),
        "id": "pipe-x", "title": null, "startedBy": null
    }));
    let _ = run_node(json!({
        "op": "advance", "memDir": js_mem.path().to_str().unwrap(),
        "stepId": "step-a", "note": null, "decidedBy": null, "decision": null
    }));
    let js = run_node(json!({
        "op": "summarizeState", "memDir": js_mem.path().to_str().unwrap()
    }));

    let clock = FixedClock(1_700_000_000_000);
    let tpl = templates_dir_for_rs();
    start(&tpl, rs_mem.path(), "pipe-x", None, None, &clock).unwrap();
    advance(rs_mem.path(), "step-a", None, None, None, &clock).unwrap();
    let rs_state = read_state(rs_mem.path());
    let rs_summary = summarize_run(&rs_state);

    // Normalizar timestamps e ids.
    let mut js = js.clone();
    let mut rs = rs_summary.clone();
    for v in [&mut js, &mut rs] {
        for k in ["run_id", "started_at", "ended_at", "duration_ms"] {
            if let Some(x) = v.get_mut(k) {
                *x = Value::Null;
            }
        }
    }
    assert_eq!(js, rs);
}

// ── list with override ──────────────────────────────────────────────────

#[test]
fn parity_list_installed_overrides_template() {
    if !node_available() {
        return;
    }
    // Sem mexer no templates_dir real — só testamos installed (templates
    // vindos do plugin/pipelines não são determinísticos para paridade
    // porque mudam com o repo).
    let yaml_installed =
        "id: pipe-shared\nlabel: Installed\nsteps:\n  - id: step-a\n    agent: x\n";
    let js_mem = tempdir().unwrap();
    let rs_mem = tempdir().unwrap();
    for mem in [&js_mem, &rs_mem] {
        let d = mem.path().join("pipelines/pipe-shared");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("pipeline.yaml"), yaml_installed).unwrap();
    }
    let js = run_node(json!({
        "op": "list", "memDir": js_mem.path().to_str().unwrap()
    }));
    // Filtrar só o pipe-shared (templates do repo podem variar).
    let js_filtered: Vec<Value> = js
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["id"].as_str() == Some("pipe-shared"))
        .cloned()
        .collect();
    let tpl = templates_dir_for_rs();
    let rs = list(&tpl, rs_mem.path());
    let rs_filtered: Vec<Value> = rs
        .into_iter()
        .filter(|v| v["id"].as_str() == Some("pipe-shared"))
        .collect();
    assert_eq!(js_filtered, rs_filtered);
    assert_eq!(js_filtered.len(), 1);
    assert_eq!(js_filtered[0]["source"], "installed");
}
