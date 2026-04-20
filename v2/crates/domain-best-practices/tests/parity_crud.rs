//! Paridade `createLocalPractice`/`updateLocalPractice`/
//! `deleteLocalPractice`/`listLocalPractices` vs Rust.

use domain_best_practices::{
    create_local_practice, delete_local_practice, list_local_practices, update_local_practice,
    CreateLocalInput, UpdateLocalInput,
};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const cjsPath = process.argv[1];
const op = process.argv[2];
const arg1 = process.argv[3] || '';
const arg2 = process.argv[4] || '';
const arg3 = process.argv[5] || '';

const lib = require(cjsPath);

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    let out;
    if (op === 'create') {
      const memDir = arg1;
      const overwrite = arg2 === 'true';
      const payload = JSON.parse(input);
      out = lib.createLocalPractice({ memDir, input: payload, overwrite });
    } else if (op === 'update') {
      const memDir = arg1;
      const id = arg2;
      const payload = JSON.parse(input);
      out = lib.updateLocalPractice({ memDir, id, input: payload });
    } else if (op === 'delete') {
      const memDir = arg1;
      const id = arg2;
      out = lib.deleteLocalPractice({ memDir, id });
    } else if (op === 'list') {
      const memDir = arg1;
      out = lib.listLocalPractices(memDir);
    } else {
      process.stderr.write('op desconhecida: ' + op);
      process.exit(2);
    }
    process.stdout.write(JSON.stringify(out));
  } catch (e) {
    process.stderr.write('THROW: ' + e.message);
    process.exit(3);
  }
});
"#;

fn cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/best-practices.cjs")
        .canonicalize()
        .expect(".cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn call_v1(op: &str, arg1: &str, arg2: &str, arg3: &str, payload: &Value) -> Result<String, String> {
    let mut child = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(cjs_path())
        .arg(op)
        .arg(arg1)
        .arg(arg2)
        .arg(arg3)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn node");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("wait");
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(String::from_utf8(out.stdout).expect("utf8"))
}

fn twin(tmp: &TempDir) -> (PathBuf, PathBuf) {
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    (a, b)
}

fn compare_trees(a: &Path, b: &Path) {
    let la = walk(a);
    let lb = walk(b);
    assert_eq!(la, lb, "tree diff");
    for rel in &la {
        let ca = fs::read(a.join(rel)).unwrap();
        let cb = fs::read(b.join(rel)).unwrap();
        assert_eq!(
            ca, cb,
            "diff em {rel:?}\nA:\n{}\nB:\n{}",
            String::from_utf8_lossy(&ca),
            String::from_utf8_lossy(&cb)
        );
    }
}

fn walk(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !root.exists() {
        return out;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        for entry in fs::read_dir(&p).unwrap() {
            let e = entry.unwrap();
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path.strip_prefix(root).unwrap().to_path_buf());
            }
        }
    }
    out.sort();
    out
}

#[test]
fn create_happy_path() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin(&tmp);
    let payload = json!({
        "id": "foo",
        "name": "Foo Practice",
        "scope": "engineering",
        "tags": ["a", "b"],
        "when_to_use": "always",
        "not_for": "never",
        "content": "body"
    });
    call_v1("create", a.to_str().unwrap(), "false", "", &payload).unwrap();
    create_local_practice(
        &b,
        &CreateLocalInput {
            id: "foo",
            name: "Foo Practice",
            scope: Some("engineering"),
            tags: Some(&[Value::String("a".into()), Value::String("b".into())]),
            when_to_use: Some("always"),
            not_for: Some("never"),
            content: "body",
        },
        false,
    )
    .unwrap();
    compare_trees(&a, &b);
}

#[test]
fn create_trims_fields() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin(&tmp);
    let payload = json!({
        "id": "foo",
        "name": "  Trimmed  ",
        "scope": "  eng  ",
        "tags": ["  a  ", "", "b"],
        "when_to_use": "  use  ",
        "not_for": "  not  ",
        "content": "body"
    });
    call_v1("create", a.to_str().unwrap(), "false", "", &payload).unwrap();
    create_local_practice(
        &b,
        &CreateLocalInput {
            id: "foo",
            name: "  Trimmed  ",
            scope: Some("  eng  "),
            tags: Some(&[
                Value::String("  a  ".into()),
                Value::String("".into()),
                Value::String("b".into()),
            ]),
            when_to_use: Some("  use  "),
            not_for: Some("  not  "),
            content: "body",
        },
        false,
    )
    .unwrap();
    compare_trees(&a, &b);
}

#[test]
fn create_adds_trailing_newline() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin(&tmp);
    let payload = json!({"id": "foo", "name": "n", "content": "no newline"});
    call_v1("create", a.to_str().unwrap(), "false", "", &payload).unwrap();
    create_local_practice(
        &b,
        &CreateLocalInput {
            id: "foo",
            name: "n",
            content: "no newline",
            ..Default::default()
        },
        false,
    )
    .unwrap();
    compare_trees(&a, &b);
}

#[test]
fn create_keeps_existing_newline() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin(&tmp);
    let payload = json!({"id": "foo", "name": "n", "content": "has newline\n"});
    call_v1("create", a.to_str().unwrap(), "false", "", &payload).unwrap();
    create_local_practice(
        &b,
        &CreateLocalInput {
            id: "foo",
            name: "n",
            content: "has newline\n",
            ..Default::default()
        },
        false,
    )
    .unwrap();
    compare_trees(&a, &b);
}

#[test]
fn create_invalid_id_both_error() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin(&tmp);
    let payload = json!({"id": "Bad-ID", "name": "n", "content": "x"});
    let v1 = call_v1("create", a.to_str().unwrap(), "false", "", &payload).unwrap_err();
    assert!(v1.contains("id inválido"), "v1 msg: {v1}");
    let v2 = create_local_practice(
        &b,
        &CreateLocalInput {
            id: "Bad-ID",
            name: "n",
            content: "x",
            ..Default::default()
        },
        false,
    )
    .unwrap_err();
    assert!(matches!(
        v2,
        domain_best_practices::LocalPracticeError::InvalidId
    ));
}

#[test]
fn update_partial_preserves_rest() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin(&tmp);
    let create_payload = json!({
        "id": "foo",
        "name": "Old",
        "scope": "old",
        "tags": ["a"],
        "content": "body"
    });
    call_v1("create", a.to_str().unwrap(), "false", "", &create_payload).unwrap();
    create_local_practice(
        &b,
        &CreateLocalInput {
            id: "foo",
            name: "Old",
            scope: Some("old"),
            tags: Some(&[Value::String("a".into())]),
            content: "body",
            ..Default::default()
        },
        false,
    )
    .unwrap();

    let update_payload = json!({"name": "New"});
    call_v1("update", a.to_str().unwrap(), "foo", "", &update_payload).unwrap();
    update_local_practice(
        &b,
        "foo",
        &UpdateLocalInput {
            name: Some("New"),
            ..Default::default()
        },
    )
    .unwrap();
    compare_trees(&a, &b);
}

#[test]
fn update_content_rewrites_md() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin(&tmp);
    let create_payload = json!({"id": "foo", "name": "n", "content": "old body"});
    call_v1("create", a.to_str().unwrap(), "false", "", &create_payload).unwrap();
    create_local_practice(
        &b,
        &CreateLocalInput {
            id: "foo",
            name: "n",
            content: "old body",
            ..Default::default()
        },
        false,
    )
    .unwrap();

    let update_payload = json!({"content": "new body"});
    call_v1("update", a.to_str().unwrap(), "foo", "", &update_payload).unwrap();
    update_local_practice(
        &b,
        "foo",
        &UpdateLocalInput {
            content: Some("new body"),
            ..Default::default()
        },
    )
    .unwrap();
    compare_trees(&a, &b);
}

#[test]
fn update_missing_both_error() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin(&tmp);
    let payload = json!({"name":"x"});
    let v1 = call_v1("update", a.to_str().unwrap(), "ghost", "", &payload).unwrap_err();
    assert!(v1.contains("não existe"), "v1 msg: {v1}");
    let v2 = update_local_practice(
        &b,
        "ghost",
        &UpdateLocalInput {
            name: Some("x"),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(matches!(
        v2,
        domain_best_practices::LocalPracticeError::NotFoundLocal { .. }
    ));
}

#[test]
fn delete_local_happy_path() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin(&tmp);
    let payload = json!({"id": "foo", "name": "n", "content": "body"});
    call_v1("create", a.to_str().unwrap(), "false", "", &payload).unwrap();
    create_local_practice(
        &b,
        &CreateLocalInput {
            id: "foo",
            name: "n",
            content: "body",
            ..Default::default()
        },
        false,
    )
    .unwrap();

    call_v1("delete", a.to_str().unwrap(), "foo", "", &json!({})).unwrap();
    delete_local_practice(&b, "foo").unwrap();
    compare_trees(&a, &b);
    assert!(!a.join("best-practices/foo").exists());
    assert!(!b.join("best-practices/foo").exists());
}

#[test]
fn delete_refuses_non_local_both() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin(&tmp);
    for m in [&a, &b] {
        fs::create_dir_all(m.join("best-practices/foo")).unwrap();
        fs::write(m.join("best-practices/foo/practice.md"), "x").unwrap();
        fs::write(
            m.join("best-practices/foo/meta.json"),
            r#"{"id":"foo","source":"catalog"}"#,
        )
        .unwrap();
    }
    let v1 = call_v1("delete", a.to_str().unwrap(), "foo", "", &json!({})).unwrap_err();
    assert!(v1.contains("catálogo"), "v1 msg: {v1}");
    let v2 = delete_local_practice(&b, "foo").unwrap_err();
    assert!(matches!(
        v2,
        domain_best_practices::LocalPracticeError::NotLocalSource { .. }
    ));
}

#[test]
fn list_empty() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin(&tmp);
    let v1 = call_v1("list", a.to_str().unwrap(), "", "", &json!({})).unwrap();
    let v2 = list_local_practices(&b);
    let v2s = serde_json::to_string(&v2.into_iter().map(Value::Object).collect::<Vec<_>>()).unwrap();
    assert_eq!(v1, v2s);
}

#[test]
fn list_with_entries() {
    if !node_available() {
        return;
    }
    // Ordem: o .cjs usa readdirSync; Rust fs::read_dir. Para garantir
    // uma ordem consistente nos dois lados, criamos o mesmo conteúdo
    // e iteramos ambos — a ordem do filesystem pode bater, mas se não,
    // comparamos como conjuntos (por id).
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin(&tmp);
    let payload = json!({"id": "alpha", "name": "Alpha", "content": "a"});
    call_v1("create", a.to_str().unwrap(), "false", "", &payload).unwrap();
    create_local_practice(
        &b,
        &CreateLocalInput {
            id: "alpha",
            name: "Alpha",
            content: "a",
            ..Default::default()
        },
        false,
    )
    .unwrap();

    let payload2 = json!({"id": "beta", "name": "Beta", "content": "b"});
    call_v1("create", a.to_str().unwrap(), "false", "", &payload2).unwrap();
    create_local_practice(
        &b,
        &CreateLocalInput {
            id: "beta",
            name: "Beta",
            content: "b",
            ..Default::default()
        },
        false,
    )
    .unwrap();

    // Não conseguimos garantir ordem do filesystem → comparamos como set.
    let v1_raw = call_v1("list", a.to_str().unwrap(), "", "", &json!({})).unwrap();
    let v1: Vec<Value> = serde_json::from_str(&v1_raw).unwrap();
    let v2 = list_local_practices(&b);

    let mut v1_sorted: Vec<Value> = v1;
    v1_sorted.sort_by_key(|v| v["id"].as_str().unwrap_or_default().to_string());
    let mut v2_sorted: Vec<Value> = v2.into_iter().map(Value::Object).collect();
    v2_sorted.sort_by_key(|v| v["id"].as_str().unwrap_or_default().to_string());

    let s1 = serde_json::to_string(&v1_sorted).unwrap();
    let s2 = serde_json::to_string(&v2_sorted).unwrap();
    assert_eq!(s1, s2);
}
