//! Paridade de helpers privados do .cjs (compactArgs, escapeRegex).
//! Extrai a fonte das funções via regex + `new Function` no Node.

use domain_indexer::{compact_args, escape_regex};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const NODE_SCRIPT: &str = r#"
const fs = require('fs');
const src = fs.readFileSync(process.argv[1], 'utf8');
const compactM = src.match(/function compactArgs\([\s\S]*?\n\}\n/);
const escapeM  = src.match(/function escapeRegex\([\s\S]*?\n\}\n/);
if (!compactM || !escapeM) { process.stderr.write('extract failed'); process.exit(2); }
// Build a sandbox module with both
const body = compactM[0] + '\n' + escapeM[0] + '\n' +
  'return { compactArgs, escapeRegex };';
const mod = new Function(body)();

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const { fn, arg } = JSON.parse(input);
    const r = mod[fn](arg);
    process.stdout.write(JSON.stringify(r));
  } catch (e) {
    process.stderr.write('THROW: ' + e.message);
    process.exit(3);
  }
});
"#;

fn cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/indexer.cjs")
        .canonicalize()
        .expect("indexer.cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_node(fn_name: &str, arg: &str) -> String {
    let input = serde_json::json!({ "fn": fn_name, "arg": arg });
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
        panic!("node failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let raw = String::from_utf8(out.stdout).unwrap();
    // raw is JSON-stringified — parse to unwrap quotes
    serde_json::from_str::<String>(&raw).unwrap()
}

fn check_compact(args: &str) {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let js = run_node("compactArgs", args);
    let rs = compact_args(args);
    assert_eq!(js, rs, "compactArgs divergence for {args:?}");
}

fn check_escape(s: &str) {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let js = run_node("escapeRegex", s);
    let rs = escape_regex(s);
    assert_eq!(js, rs, "escapeRegex divergence for {s:?}");
}

#[test]
fn compact_args_empty() {
    check_compact("");
}

#[test]
fn compact_args_short() {
    check_compact("a, b");
}

#[test]
fn compact_args_whitespace() {
    check_compact("  a,\n\tb   c  ");
}

#[test]
fn compact_args_exactly_60_chars() {
    check_compact(&"x".repeat(60));
}

#[test]
fn compact_args_61_chars_truncated() {
    check_compact(&"x".repeat(61));
}

#[test]
fn compact_args_80_chars_truncated() {
    check_compact(&"x".repeat(80));
}

#[test]
fn compact_args_mixed_names() {
    check_compact("self, user_id: int, opts: Optional[Dict[str, Any]] = None");
}

#[test]
fn escape_regex_special_chars() {
    check_escape("a.b*c+d?e^f$g{h}i(j)k|l[m]n\\o");
}

#[test]
fn escape_regex_normal() {
    check_escape("foo_bar123");
}

#[test]
fn escape_regex_empty() {
    check_escape("");
}

#[test]
fn escape_regex_brackets_only() {
    check_escape("([{}])");
}
