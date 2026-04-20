//! Paridade v1 vs v2 dos extractors (EXTRACTORS map do .cjs).
//! Compara JSON.stringify com serde_json::to_string byte-a-byte.

use domain_indexer::{extract_go, extract_python, extract_rust, extract_sql, extract_ts};
use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const NODE_SCRIPT: &str = r#"
const cjsPath = process.argv[1];
const { EXTRACTORS } = require(cjsPath);
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const { lang, content } = JSON.parse(input);
    const fn = EXTRACTORS[lang];
    const r = fn(content);
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

fn run_node(lang: &str, content: &str) -> Value {
    let input = serde_json::json!({ "lang": lang, "content": content });
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
    serde_json::from_slice(&out.stdout).unwrap()
}

fn pair(lang: &str, content: &str, rust_symbols: Vec<Value>) {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let node_out = run_node(lang, content);
    let js_json = serde_json::to_string(&node_out).unwrap();
    let rs_json = serde_json::to_string(&Value::Array(rust_symbols.clone())).unwrap();
    assert_eq!(js_json, rs_json, "divergence for lang={lang}\njs:\n{js_json}\nrs:\n{rs_json}");
}

#[test]
fn parity_python_comprehensive() {
    let src = r#"def top_level(x, y):
    pass

async def async_fn(a: int) -> dict:
    return {}

class Foo(Base):
    def method(self):
        pass

@router.get("/items")
async def list_items():
    pass

class Bare:
    pass
"#;
    pair("python", src, extract_python(src));
}

#[test]
fn parity_typescript_comprehensive() {
    let src = r#"export function hello(x: number): void {}
export async function asyncOne() {}
export const add = (a: number, b: number) => a + b;
export const VERSION = "1.0";
export class Foo extends Bar {}
export abstract class Baz {}
export interface Shape { x: number }
export type ID = string;
export enum Color { R, G, B }
export async function GET(req: Request) {}
export function POST() {}
"#;
    pair("typescript", src, extract_ts(src));
}

#[test]
fn parity_rust_comprehensive() {
    let src = r#"pub fn foo(x: i32) -> i32 { x }
fn private_fn() {}
pub async fn pub_async(a: String) {}
pub(crate) fn scoped() {}
pub struct PubStruct { pub x: i32 }
struct Priv;
pub enum Color { R, G }
pub trait Animal {}
trait PrivTrait {}
"#;
    pair("rust", src, extract_rust(src));
}

#[test]
fn parity_sql_comprehensive() {
    let src = r#"CREATE TABLE users (id INT);
CREATE TABLE IF NOT EXISTS "quoted_tbl" (id INT);
create type status as enum ('a', 'b');
CREATE INDEX idx_users_email ON users(email);
create or replace unique index if not exists idx_stuff on foo(bar);
"#;
    pair("sql", src, extract_sql(src));
}

#[test]
fn parity_go_comprehensive() {
    let src = r#"func Exported(x int) int { return x }
func unexported() {}
func (r *Receiver) Method(x int) {}
type Widget struct { x int }
type iface interface { Foo() }
"#;
    pair("go", src, extract_go(src));
}

#[test]
fn parity_javascript() {
    let src = r#"export function hi() {}
export const mul = (a, b) => a * b;
export class Cat {}
"#;
    // js uses extractTs
    pair("javascript", src, extract_ts(src));
}

#[test]
fn parity_python_empty() {
    let src = "";
    pair("python", src, extract_python(src));
}

#[test]
fn parity_python_long_args_truncate() {
    // compactArgs should truncate > 60 chars
    let src = "def foo(a_really_long_argument_name, another_argument, and_one_more_thing_here): pass\n";
    pair("python", src, extract_python(src));
}

#[test]
fn parity_ts_const_no_arrow() {
    let src = "export const X = someExpression();\n";
    pair("typescript", src, extract_ts(src));
}

#[test]
fn parity_ts_typed_const_arrow() {
    let src = "export const fn1: MyType = (x: string) => x;\n";
    pair("typescript", src, extract_ts(src));
}

#[test]
fn parity_rust_generics() {
    let src = "pub fn generic<T: Clone>(x: T) -> T { x }\n";
    pair("rust", src, extract_rust(src));
}
