//! Paridade v1 vs v2 para `parseArgs`. `parseArgs` não é exportado do
//! `.cjs`, então passamos pelo CLI de um mini-script que o invoca via
//! `require` e imprime JSON. Para não executar o loop `watch`, usamos
//! apenas o branch de `require.main` (não rodamos `watch`) — chamamos
//! `parseArgs` diretamente, copiando o mesmo fonte JS aqui como
//! contraprova **ou** reexecutando a função através do arquivo.
//!
//! Abordagem escolhida: reinjeção. O script re-carrega o arquivo como
//! string, extrai `parseArgs`, e chama — assim garantimos byte-idêntico
//! ao comportamento do `.cjs`.

use domain_firefighter::parse_args;
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const NODE_SCRIPT: &str = r#"
const fs = require('fs');
const src = fs.readFileSync(process.argv[1], 'utf8');
// Extrai a função parseArgs do módulo (match ingênuo é suficiente aqui).
const m = src.match(/function parseArgs\([\s\S]*?\n\}\n/);
if (!m) { throw new Error('parseArgs not found'); }
// Avalia a função no escopo global via Function ctor (sem acesso ao require).
const parseArgsFn = new Function(`${m[0]}\nreturn parseArgs;`)();

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const { argv } = JSON.parse(input);
    process.stdout.write(JSON.stringify(parseArgsFn(argv)));
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

fn run_node(argv: &[&str]) -> Value {
    let input = json!({"argv": argv});
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
    serde_json::from_slice(&out.stdout).unwrap()
}

fn to_value(args: &domain_firefighter::WatchArgs) -> Value {
    json!({
        "interval": args.interval,
        "once": args.once,
        "dryRun": args.dry_run,
        "agentName": args.agent_name,
    })
}

#[test]
fn parse_args_defaults() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let v1 = run_node(&[]);
    let v2 = to_value(&parse_args(&[]));
    assert_eq!(v1, v2);
}

#[test]
fn parse_args_all_flags_together() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let argv = &["--once", "--dry-run", "--interval", "30", "--agent", "manager"];
    let v1 = run_node(argv);
    let v2 = to_value(&parse_args(argv));
    assert_eq!(v1, v2);
}

#[test]
fn parse_args_interval_non_numeric_falls_back_to_60() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let argv = &["--interval", "abc"];
    let v1 = run_node(argv);
    let v2 = to_value(&parse_args(argv));
    assert_eq!(v1, v2);
}

#[test]
fn parse_args_interval_zero_falls_back_to_60() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    // JS: parseInt("0",10) || 60 → 60
    let argv = &["--interval", "0"];
    let v1 = run_node(argv);
    let v2 = to_value(&parse_args(argv));
    assert_eq!(v1, v2);
}

#[test]
fn parse_args_interval_with_suffix_takes_leading_digits() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    // JS: parseInt("45xyz",10) → 45
    let argv = &["--interval", "45xyz"];
    let v1 = run_node(argv);
    let v2 = to_value(&parse_args(argv));
    assert_eq!(v1, v2);
}

#[test]
fn parse_args_unknown_flags_ignored() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let argv = &["--bogus", "value", "--once"];
    let v1 = run_node(argv);
    let v2 = to_value(&parse_args(argv));
    assert_eq!(v1, v2);
}

#[test]
fn parse_args_agent_captures_next_token() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let argv = &["--agent", "fire1"];
    let v1 = run_node(argv);
    let v2 = to_value(&parse_args(argv));
    assert_eq!(v1, v2);
}
