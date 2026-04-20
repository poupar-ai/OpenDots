//! Paridade v1 vs v2 para `buildTriagePrompt` — byte-idêntica.
//! `buildTriagePrompt` não é exportada do `.cjs`, extraímos a função por
//! regex-match, mesma técnica de `parity_args.rs`.

use domain_firefighter::{build_triage_prompt, WatchCfg};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const NODE_SCRIPT: &str = r#"
const fs = require('fs');
const src = fs.readFileSync(process.argv[1], 'utf8');
const m = src.match(/function buildTriagePrompt\([\s\S]*?^\}\n/m);
if (!m) { throw new Error('buildTriagePrompt not found'); }
const fn = new Function(`${m[0]}\nreturn buildTriagePrompt;`)();

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const { name, cfg, mini, logs } = JSON.parse(input);
    process.stdout.write(fn(name, cfg, mini, logs));
  } catch (e) {
    process.stderr.write('THROW: ' + e.message);
    process.exit(3);
  }
});
"#;

fn cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/firefighter.cjs")
        .canonicalize()
        .expect("firefighter.cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_node(name: &str, cfg: Value, mini: &str, logs: Value) -> String {
    let input = json!({
        "name": name,
        "cfg": cfg,
        "mini": mini,
        "logs": logs,
    });
    let mut child = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(cjs_path())
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
        panic!("node exited: {}", String::from_utf8_lossy(&out.stderr));
    }
    String::from_utf8(out.stdout).unwrap()
}

fn cfg_value(w: &WatchCfg) -> Value {
    json!({
        "watch": {
            "query": w.query,
            "threshold": w.threshold,
            "window_minutes": w.window_minutes,
            "cooldown_seconds": w.cooldown_seconds,
            "max_triages_per_hour": w.max_triages_per_hour,
        }
    })
}

#[test]
fn prompt_empty_logs_byte_equal() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let w = WatchCfg {
        query: "service:foo status:error",
        threshold: 5,
        window_minutes: 10,
        cooldown_seconds: 300,
        max_triages_per_hour: 3,
    };
    let v1 = run_node("manager", cfg_value(&w), "runbook do manager", json!([]));
    let v2 = build_triage_prompt("manager", &w, "runbook do manager", &[]);
    assert_eq!(v1, v2);
}

#[test]
fn prompt_logs_with_message_and_attrs() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let w = WatchCfg {
        query: "*",
        threshold: 1,
        window_minutes: 5,
        cooldown_seconds: 60,
        max_triages_per_hour: 3,
    };
    let logs = json!([
        {"attributes":{"message":"boom","timestamp":"2026-04-19T11:59:00.000Z","attributes":{"service":"api","host":"h1"}}},
        {"attributes":{"message":"zap","timestamp":"2026-04-19T11:58:00.000Z","attributes":{"service":"api","host":"h2"}}},
    ]);
    let v1 = run_node("fire", cfg_value(&w), "mini", logs.clone());
    let v2 = build_triage_prompt("fire", &w, "mini", logs.as_array().unwrap());
    assert_eq!(v1, v2);
}

#[test]
fn prompt_logs_without_message_uses_attrs_stringified() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let w = WatchCfg {
        query: "q",
        threshold: 1,
        window_minutes: 1,
        cooldown_seconds: 60,
        max_triages_per_hour: 3,
    };
    // Sem `message` → cai em JSON.stringify(attrs).slice(0,200)
    let logs = json!([
        {"attributes":{"timestamp":"2026-04-19T11:59:00.000Z","attributes":{"service":"svc","host":"h","extra":"x"}}},
    ]);
    let v1 = run_node("f", cfg_value(&w), "", logs.clone());
    let v2 = build_triage_prompt("f", &w, "", logs.as_array().unwrap());
    assert_eq!(v1, v2);
}

#[test]
fn prompt_truncates_logs_at_10() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let w = WatchCfg {
        query: "q",
        threshold: 1,
        window_minutes: 1,
        cooldown_seconds: 60,
        max_triages_per_hour: 3,
    };
    let logs: Value = (0..15)
        .map(|i| {
            json!({"attributes": {
                "message": format!("m{i}"),
                "timestamp": "2026-04-19T11:59:00.000Z",
                "attributes": {"service":"s","host":"h"},
            }})
        })
        .collect::<Vec<_>>()
        .into();
    let v1 = run_node("f", cfg_value(&w), "", logs.clone());
    let v2 = build_triage_prompt("f", &w, "", logs.as_array().unwrap());
    assert_eq!(v1, v2);
    // Events counter ainda é 15 (logs.length), mas só 10 no resumo.
    assert!(v2.contains("Eventos encontrados: 15"));
    assert!(v2.contains("m9\n"));
    assert!(!v2.contains("m10"));
}

#[test]
fn prompt_long_message_truncated_utf16() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let w = WatchCfg {
        query: "q",
        threshold: 1,
        window_minutes: 1,
        cooldown_seconds: 60,
        max_triages_per_hour: 3,
    };
    let long = "á".repeat(500); // 500 UTF-16 units (1 each in BMP)
    let logs = json!([
        {"attributes":{"message": long, "timestamp":"2026-04-19T11:59:00.000Z","attributes":{"service":"s","host":"h"}}},
    ]);
    let v1 = run_node("f", cfg_value(&w), "", logs.clone());
    let v2 = build_triage_prompt("f", &w, "", logs.as_array().unwrap());
    assert_eq!(v1, v2);
}

#[test]
fn prompt_missing_timestamp_and_service() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let w = WatchCfg {
        query: "q",
        threshold: 1,
        window_minutes: 1,
        cooldown_seconds: 60,
        max_triages_per_hour: 3,
    };
    // Sem timestamp/service/host → defaults "?"
    let logs = json!([
        {"attributes":{"message":"no-attrs","attributes":{}}},
    ]);
    let v1 = run_node("f", cfg_value(&w), "", logs.clone());
    let v2 = build_triage_prompt("f", &w, "", logs.as_array().unwrap());
    assert_eq!(v1, v2);
}

#[test]
fn prompt_query_with_backticks_not_escaped() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    // JS usa template literal: backtick na query sai cru (achado latente).
    let w = WatchCfg {
        query: "service:`foo`",
        threshold: 1,
        window_minutes: 1,
        cooldown_seconds: 60,
        max_triages_per_hour: 3,
    };
    let v1 = run_node("f", cfg_value(&w), "", json!([]));
    let v2 = build_triage_prompt("f", &w, "", &[]);
    assert_eq!(v1, v2);
}
