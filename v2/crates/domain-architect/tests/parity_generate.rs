//! Paridade v1 (`architect.cjs::generateSeed`) vs v2.
//!
//! Desafio: `new Date().toISOString()` do .cjs é não-determinístico.
//! Solução: patch `Date.prototype.toISOString` no Node pra devolver um
//! timestamp fixo passado via CLI arg, e usar [`FixedClock`] no Rust
//! com a mesma string.

use domain_architect::{generate_seed, FixedClock};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

const FIXED_ISO: &str = "2026-04-19T12:00:00.000Z";

/// Node script: monkey-patch `Date.prototype.toISOString` pra devolver
/// `FIXED_ISO`; chama `generateSeed` com payload do stdin; spec usa um
/// `seedsDir` custom passado via `process.env.PM_SEEDS_DIR` e o script
/// relativiza o `SEEDS_DIR` reescrevendo `require.cache`.
///
/// Como o `.cjs` calcula `SEEDS_DIR` em tempo de load usando `__dirname`,
/// sobreescrevemos o módulo monkey-patching a função após require,
/// chamando uma versão injetada via `process.env.PM_SEEDS_DIR`. Mais
/// simples: criamos um shim que re-exporta `generateSeed` mas apontando
/// pra um seeds dir custom via wrapper que reescreve `path.join`.
///
/// Na prática, o arquivo `architect.cjs` tem `SEEDS_DIR = path.join(
/// __dirname, '..', 'seeds')`. Pra paridade de teste, copiamos o .cjs
/// pra dentro de uma pasta onde `../seeds/` = o dir temporário desejado.
const NODE_SCRIPT: &str = r#"
// Argumentos: [cjsPath, isoFixed]
const cjsPath = process.argv[1];
const isoFixed = process.argv[2];

// Monkey-patch Date.prototype.toISOString *antes* do require.
Date.prototype.toISOString = function () { return isoFixed; };

const lib = require(cjsPath);
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const spec = JSON.parse(input);
    const out = lib.generateSeed(spec);
    process.stdout.write(JSON.stringify(out));
  } catch (e) {
    process.stderr.write('THROW: ' + e.message);
    process.exit(3);
  }
});
"#;

fn plugin_scripts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts")
        .canonicalize()
        .expect("plugin/scripts missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Pra enganar o `SEEDS_DIR = path.join(__dirname, '..', 'seeds')` do
/// .cjs, criamos um symlink: `<tmp>/scripts/architect.cjs` aponta pro
/// .cjs real, e o seeds dir alvo fica em `<tmp>/seeds/`. Como __dirname
/// do require é o path do caller (do symlink resolvido), precisamos
/// copiar o arquivo de verdade — symlinks no Node resolvem ao path
/// real. Solução mais segura: copiar o .cjs pro temp.
fn stage_cjs(tmp: &Path) -> PathBuf {
    let scripts = tmp.join("scripts");
    fs::create_dir_all(&scripts).unwrap();
    let src = plugin_scripts_dir().join("architect.cjs");
    let dst = scripts.join("architect.cjs");
    fs::copy(src, &dst).unwrap();
    fs::create_dir_all(tmp.join("seeds")).unwrap();
    dst
}

fn call_v1(cjs: &Path, spec: &Value) -> Result<String, String> {
    let mut child = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(cjs)
        .arg(FIXED_ISO)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn node");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(spec.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("wait");
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(String::from_utf8(out.stdout).expect("utf8"))
}

/// Compara byte-a-byte cada arquivo gerado dentro de cada seeds-dir.
fn compare_trees(a: &Path, b: &Path) {
    let list_a = walk(a);
    let list_b = walk(b);
    assert_eq!(
        list_a, list_b,
        "estrutura de arquivos diverge\nA: {list_a:?}\nB: {list_b:?}"
    );
    for rel in &list_a {
        let ca = fs::read(a.join(rel)).unwrap();
        let cb = fs::read(b.join(rel)).unwrap();
        assert_eq!(
            ca,
            cb,
            "conteúdo diverge em {rel:?}\nA:\n{}\nB:\n{}",
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

fn twin_setup() -> (TempDir, PathBuf, PathBuf, PathBuf) {
    let tmp = TempDir::new().unwrap();
    // V1: <tmp>/a/scripts/architect.cjs, seeds em <tmp>/a/seeds/
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    let cjs = stage_cjs(&a);
    let b_seeds = b.join("seeds");
    fs::create_dir_all(&b_seeds).unwrap();
    let a_seeds = a.join("seeds");
    (tmp, cjs, a_seeds, b_seeds)
}

#[test]
fn generate_minimal_spec() {
    if !node_available() {
        return;
    }
    let (_tmp, cjs, a_seeds, b_seeds) = twin_setup();
    let spec = serde_json::json!({
        "slug": "x",
        "category": "custom",
        "agents": []
    });
    call_v1(&cjs, &spec).unwrap();
    generate_seed(&b_seeds, &spec, &FixedClock(FIXED_ISO.into())).unwrap();
    compare_trees(&a_seeds, &b_seeds);
}

#[test]
fn generate_full_spec() {
    if !node_available() {
        return;
    }
    let (_tmp, cjs, a_seeds, b_seeds) = twin_setup();
    let spec = serde_json::json!({
        "slug": "my-biz",
        "category": "retail",
        "category_label": "Varejo",
        "category_description": "Seeds para comércio.",
        "label": "Minha Loja",
        "description": "Loja pequena de roupas.",
        "best_practices": ["seo", "ux"],
        "agents": [
            {
                "name": "seller",
                "label": "Vendedor",
                "icon": "🛒",
                "description": "Fecha pedidos.",
                "category": "motor",
                "best_practices": ["async", "seo"]
            },
            {
                "name": "support",
                "prompt": "# Suporte\n\nFala com cliente.\n"
            }
        ]
    });
    call_v1(&cjs, &spec).unwrap();
    generate_seed(&b_seeds, &spec, &FixedClock(FIXED_ISO.into())).unwrap();
    compare_trees(&a_seeds, &b_seeds);
}

#[test]
fn generate_agents_with_name_normalization() {
    if !node_available() {
        return;
    }
    let (_tmp, cjs, a_seeds, b_seeds) = twin_setup();
    let spec = serde_json::json!({
        "slug": "y",
        "category": "custom",
        "agents": [
            {"name": "Bob-Smith"},
            {"name": "FOO.bar"},
            {"name": "agent 42"},
        ]
    });
    call_v1(&cjs, &spec).unwrap();
    generate_seed(&b_seeds, &spec, &FixedClock(FIXED_ISO.into())).unwrap();
    compare_trees(&a_seeds, &b_seeds);
}

#[test]
fn generate_skips_nameless_agents() {
    if !node_available() {
        return;
    }
    let (_tmp, cjs, a_seeds, b_seeds) = twin_setup();
    let spec = serde_json::json!({
        "slug": "y",
        "category": "custom",
        "agents": [
            {"name": "bob"},
            {"label": "no name"},
            {"name": ""},
            {"name": null},
            {"name": 0},
        ]
    });
    call_v1(&cjs, &spec).unwrap();
    generate_seed(&b_seeds, &spec, &FixedClock(FIXED_ISO.into())).unwrap();
    compare_trees(&a_seeds, &b_seeds);
}

#[test]
fn generate_preserves_category_meta_if_exists() {
    if !node_available() {
        return;
    }
    let (_tmp, cjs, a_seeds, b_seeds) = twin_setup();
    // Pré-cria meta.json em ambos os lados.
    fs::create_dir_all(a_seeds.join("custom")).unwrap();
    fs::create_dir_all(b_seeds.join("custom")).unwrap();
    fs::write(a_seeds.join("custom").join("meta.json"), "EXISTING").unwrap();
    fs::write(b_seeds.join("custom").join("meta.json"), "EXISTING").unwrap();

    let spec = serde_json::json!({
        "slug": "z",
        "category": "custom",
        "category_label": "Ignored",
        "agents": []
    });
    call_v1(&cjs, &spec).unwrap();
    generate_seed(&b_seeds, &spec, &FixedClock(FIXED_ISO.into())).unwrap();
    compare_trees(&a_seeds, &b_seeds);
}

#[test]
fn generate_duplicate_errors_on_both_sides() {
    if !node_available() {
        return;
    }
    let (_tmp, cjs, _a_seeds, b_seeds) = twin_setup();
    let spec = serde_json::json!({
        "slug": "dup",
        "category": "custom",
        "agents": []
    });
    call_v1(&cjs, &spec).unwrap();
    generate_seed(&b_seeds, &spec, &FixedClock(FIXED_ISO.into())).unwrap();

    // Segunda chamada → erro.
    let e1 = call_v1(&cjs, &spec).unwrap_err();
    assert!(
        e1.contains("já existe"),
        "v1 erro inesperado: {e1}"
    );
    let e2 = generate_seed(&b_seeds, &spec, &FixedClock(FIXED_ISO.into())).unwrap_err();
    assert!(matches!(
        e2,
        domain_architect::GenerateSeedError::AlreadyExists { .. }
    ));
}

#[test]
fn generate_invalid_slug_errors_on_both_sides() {
    if !node_available() {
        return;
    }
    let (_tmp, cjs, _a_seeds, b_seeds) = twin_setup();
    let spec = serde_json::json!({
        "slug": "NotKebab",
        "category": "custom",
        "agents": []
    });
    let e1 = call_v1(&cjs, &spec).unwrap_err();
    assert!(e1.contains("kebab-case"), "v1 erro inesperado: {e1}");
    let e2 = generate_seed(&b_seeds, &spec, &FixedClock(FIXED_ISO.into())).unwrap_err();
    assert!(matches!(
        e2,
        domain_architect::GenerateSeedError::InvalidKebabCase
    ));
}

#[test]
fn generate_missing_slug_errors_on_both_sides() {
    if !node_available() {
        return;
    }
    let (_tmp, cjs, _a_seeds, b_seeds) = twin_setup();
    let spec = serde_json::json!({"category":"custom"});
    let e1 = call_v1(&cjs, &spec).unwrap_err();
    assert!(e1.contains("inválido") || e1.contains("invalido"), "v1 erro: {e1}");
    let e2 = generate_seed(&b_seeds, &spec, &FixedClock(FIXED_ISO.into())).unwrap_err();
    assert!(matches!(
        e2,
        domain_architect::GenerateSeedError::MissingSlugOrCategory
    ));
}
