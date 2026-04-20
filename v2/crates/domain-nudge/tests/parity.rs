//! Parity tests — compara a porta Rust com `plugin/scripts/nudge.cjs`
//! invocado via `node` em subprocess. Pula silenciosamente se `node` não
//! está no PATH. Não testa `scheduleNudge`/`runNudgeClaude` (spawn do CLI
//! real) porque estão fora do escopo do domínio.

use domain_nudge::{
    build_nudge_prompt, extract_json, render_decisions_md, render_gotchas_md, within_cooldown,
    MIN_GAP_MS, MIN_RESPONSE_LEN,
};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tempfile::tempdir;

const NODE_SCRIPT: &str = r#"
const cjsPath = process.argv[1];
const mod = require(cjsPath);
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const req = JSON.parse(input);
    let r;
    if (req.op === 'buildNudgePrompt') {
      // Não exportado — mas `module.exports` só cobre parte. Acessamos via
      // re-require para pegar funções internas? Não. Em vez disso,
      // replicamos o prompt aqui via chamada ao mod.buildNudgePrompt se
      // existir, senão via exec do próprio arquivo dentro de um novo
      // contexto.
      const fs = require('fs');
      const vm = require('vm');
      const src = fs.readFileSync(cjsPath, 'utf8');
      const ctx = { module: {}, exports: {}, require, __dirname: require('path').dirname(cjsPath), __filename: cjsPath, process };
      ctx.global = ctx; ctx.globalThis = ctx;
      vm.createContext(ctx);
      vm.runInContext(src, ctx);
      // buildNudgePrompt é uma função de módulo (não exportada) — extraímos
      // via eval no mesmo contexto.
      vm.runInContext('globalThis.__bnp = buildNudgePrompt;', ctx);
      r = ctx.__bnp(req.agent, req.responseText, req.trigger);
    } else if (req.op === 'extractJson') {
      const fs = require('fs');
      const vm = require('vm');
      const src = fs.readFileSync(cjsPath, 'utf8');
      const ctx = { module: {}, exports: {}, require, __dirname: require('path').dirname(cjsPath), __filename: cjsPath, process };
      ctx.global = ctx; ctx.globalThis = ctx;
      vm.createContext(ctx);
      vm.runInContext(src, ctx);
      vm.runInContext('globalThis.__ej = extractJson;', ctx);
      const v = ctx.__ej(req.text);
      r = v === null ? { _null: true } : { value: v };
    } else if (req.op === 'withinCooldown') {
      const fs = require('fs');
      const vm = require('vm');
      const src = fs.readFileSync(cjsPath, 'utf8');
      const ctx = { module: {}, exports: {}, require, __dirname: require('path').dirname(cjsPath), __filename: cjsPath, process };
      ctx.global = ctx; ctx.globalThis = ctx;
      vm.createContext(ctx);
      vm.runInContext(src, ctx);
      vm.runInContext('globalThis.__wc = withinCooldown;', ctx);
      r = ctx.__wc(req.state, req.agent, req.now);
    } else if (req.op === 'renderDecisionsMd') {
      mod.renderDecisionsMd(req.memDir);
      const fs = require('fs');
      const path = require('path');
      const p = path.join(req.memDir, 'DECISIONS.md');
      r = fs.existsSync(p) ? fs.readFileSync(p, 'utf8') : null;
    } else if (req.op === 'renderGotchasMd') {
      mod.renderGotchasMd(req.memDir);
      const fs = require('fs');
      const path = require('path');
      const p = path.join(req.memDir, 'GOTCHAS.md');
      r = fs.existsSync(p) ? fs.readFileSync(p, 'utf8') : null;
    } else if (req.op === 'constants') {
      r = { MIN_GAP_MS: mod.MIN_GAP_MS, MIN_RESPONSE_LEN: mod.MIN_RESPONSE_LEN };
    }
    process.stdout.write(JSON.stringify(r));
  } catch (e) {
    process.stderr.write('THROW: ' + e.message);
    process.exit(3);
  }
});
"#;

fn cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/nudge.cjs")
        .canonicalize()
        .expect("nudge.cjs missing")
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
    let cwd = cjs.parent().unwrap().to_path_buf();
    let mut child = Command::new("node")
        .current_dir(&cwd)
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

#[test]
fn parity_constants() {
    if !node_available() {
        return;
    }
    let js = run_node(json!({"op": "constants"}));
    assert_eq!(js["MIN_GAP_MS"].as_i64().unwrap(), MIN_GAP_MS);
    assert_eq!(
        js["MIN_RESPONSE_LEN"].as_i64().unwrap(),
        MIN_RESPONSE_LEN as i64
    );
}

#[test]
fn parity_build_nudge_prompt_short() {
    if !node_available() {
        return;
    }
    let js = run_node(json!({
        "op": "buildNudgePrompt",
        "agent": "frontend",
        "responseText": "Atualizei o preço do produto.",
        "trigger": "auto_handoff"
    }));
    let rs = build_nudge_prompt("frontend", "Atualizei o preço do produto.", "auto_handoff");
    assert_eq!(js.as_str().unwrap(), rs);
}

#[test]
fn parity_build_nudge_prompt_long_truncation() {
    if !node_available() {
        return;
    }
    // Texto longo com caracteres multi-byte (é 2 bytes UTF-8 mas 1 UTF-16).
    let long = "éóá ".repeat(600); // ~2400 chars UTF-16
    let js = run_node(json!({
        "op": "buildNudgePrompt",
        "agent": "x",
        "responseText": long,
        "trigger": "t"
    }));
    let rs = build_nudge_prompt("x", &long, "t");
    assert_eq!(js.as_str().unwrap(), rs);
}

#[test]
fn parity_build_nudge_prompt_empty_response() {
    if !node_available() {
        return;
    }
    let js = run_node(json!({
        "op": "buildNudgePrompt",
        "agent": "y",
        "responseText": "",
        "trigger": "t"
    }));
    let rs = build_nudge_prompt("y", "", "t");
    assert_eq!(js.as_str().unwrap(), rs);
}

#[test]
fn parity_extract_json_simple() {
    if !node_available() {
        return;
    }
    let text = "blabla {\"kind\":\"none\"} blabla";
    let js = run_node(json!({"op": "extractJson", "text": text}));
    let rs = extract_json(text);
    let js_val = js.get("value").cloned();
    let rs_val = rs;
    assert_eq!(js_val, rs_val);
}

#[test]
fn parity_extract_json_multiline() {
    if !node_available() {
        return;
    }
    let text = "lixo\n{\"kind\":\"decision\",\n\"what\":\"x\",\n\"why\":\"y\"}\nmais lixo";
    let js = run_node(json!({"op": "extractJson", "text": text}));
    let rs = extract_json(text);
    let js_val = js.get("value").cloned();
    assert_eq!(js_val, rs);
}

#[test]
fn parity_extract_json_missing_returns_null() {
    if !node_available() {
        return;
    }
    let js = run_node(json!({"op": "extractJson", "text": "sem json"}));
    assert!(js.get("_null").and_then(|v| v.as_bool()).unwrap_or(false));
    assert!(extract_json("sem json").is_none());
}

#[test]
fn parity_extract_json_invalid_returns_null() {
    if !node_available() {
        return;
    }
    let js = run_node(json!({"op": "extractJson", "text": "{not json}"}));
    assert!(js.get("_null").and_then(|v| v.as_bool()).unwrap_or(false));
    assert!(extract_json("{not json}").is_none());
}

#[test]
fn parity_extract_json_greedy_to_last_brace() {
    if !node_available() {
        return;
    }
    // Greedy: vai do primeiro `{` até o ÚLTIMO `}`.
    let text = "{\"a\":1} ruído {\"b\":2}";
    let js = run_node(json!({"op": "extractJson", "text": text}));
    let rs = extract_json(text);
    let js_val = js.get("value").cloned();
    // Neste caso o greedy captura `{"a":1} ruído {"b":2}` que NÃO é JSON válido,
    // então ambos retornam null.
    assert!(js.get("_null").and_then(|v| v.as_bool()).unwrap_or(false));
    assert!(rs.is_none());
    let _ = js_val;
}

#[test]
fn parity_within_cooldown_matches_js() {
    if !node_available() {
        return;
    }
    let cases = vec![
        (json!({}), "frontend", 10_000_i64),
        (json!({"x": {}}), "x", 10_000),
        (json!({"x": {"last_ts": "nao-iso"}}), "x", 10_000),
        (
            json!({"x": {"last_ts": "1970-01-01T00:00:00.000Z"}}),
            "x",
            59 * 60 * 1000,
        ),
        (
            json!({"x": {"last_ts": "1970-01-01T00:00:00.000Z"}}),
            "x",
            61 * 60 * 1000,
        ),
        (
            json!({"x": {"last_ts": "1970-01-01T00:30:00.000Z"}}),
            "x",
            90 * 60 * 1000,
        ),
    ];
    for (state, agent, now) in cases {
        let js = run_node(json!({
            "op": "withinCooldown",
            "state": state,
            "agent": agent,
            "now": now
        }));
        let rs = within_cooldown(&state, agent, now);
        assert_eq!(
            js.as_bool().unwrap(),
            rs,
            "divergiu: state={state} agent={agent} now={now}"
        );
    }
}

#[test]
fn parity_render_decisions_md() {
    if !node_available() {
        return;
    }
    let js_dir = tempdir().unwrap();
    let rs_dir = tempdir().unwrap();
    let content = "{\"what\":\"A\",\"why\":\"a\",\"ts\":\"2026-01-01T00:00:00.000Z\"}\n\
                   {\"what\":\"B\",\"ts\":\"2026-01-02T00:00:00.000Z\"}\n\
                   {\"text\":\"old-style\",\"why\":\"fallback\",\"ts\":\"2026-01-03T00:00:00.000Z\"}\n";
    fs::write(js_dir.path().join("decisions.jsonl"), content).unwrap();
    fs::write(rs_dir.path().join("decisions.jsonl"), content).unwrap();

    let js: Value = run_node(json!({
        "op": "renderDecisionsMd",
        "memDir": js_dir.path().to_str().unwrap()
    }));
    render_decisions_md(rs_dir.path());
    let rs_md = fs::read_to_string(rs_dir.path().join("DECISIONS.md")).unwrap();
    assert_eq!(js.as_str().unwrap(), rs_md);
}

#[test]
fn parity_render_decisions_md_empty() {
    if !node_available() {
        return;
    }
    let js_dir = tempdir().unwrap();
    let rs_dir = tempdir().unwrap();
    // Nenhum arquivo → no-op.
    let js = run_node(json!({
        "op": "renderDecisionsMd",
        "memDir": js_dir.path().to_str().unwrap()
    }));
    render_decisions_md(rs_dir.path());
    assert!(js.is_null());
    assert!(!rs_dir.path().join("DECISIONS.md").exists());
}

#[test]
fn parity_render_gotchas_md() {
    if !node_available() {
        return;
    }
    let js_dir = tempdir().unwrap();
    let rs_dir = tempdir().unwrap();
    let content = "{\"file\":\"a.rs\",\"description\":\"dup\",\"ts\":\"2026-01-01T00:00:00.000Z\"}\n\
                   {\"file\":\"a.rs\",\"description\":\"dup\",\"failure_hint\":\"boom\",\"ts\":\"2026-01-02T00:00:00.000Z\"}\n\
                   {\"file\":null,\"description\":\"sem arquivo\",\"ts\":\"2026-01-03T00:00:00.000Z\"}\n";
    fs::write(js_dir.path().join("gotchas.jsonl"), content).unwrap();
    fs::write(rs_dir.path().join("gotchas.jsonl"), content).unwrap();

    let js = run_node(json!({
        "op": "renderGotchasMd",
        "memDir": js_dir.path().to_str().unwrap()
    }));
    render_gotchas_md(rs_dir.path());
    let rs_md = fs::read_to_string(rs_dir.path().join("GOTCHAS.md")).unwrap();
    assert_eq!(js.as_str().unwrap(), rs_md);
}

#[test]
fn parity_render_gotchas_md_empty() {
    if !node_available() {
        return;
    }
    let js_dir = tempdir().unwrap();
    let rs_dir = tempdir().unwrap();
    let js = run_node(json!({
        "op": "renderGotchasMd",
        "memDir": js_dir.path().to_str().unwrap()
    }));
    render_gotchas_md(rs_dir.path());
    assert!(js.is_null());
    assert!(!rs_dir.path().join("GOTCHAS.md").exists());
}
