//! Parity tests — compara a porta Rust com
//! `plugin/scripts/skills-install.cjs` invocado via `node` em subprocess.

use domain_skills_install::{apply_skills, list_skills_in_pack, parse_frontmatter, ApplyInput};
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
    // SEEDS_DIR no .cjs é path.join(__dirname, '..', 'seeds').
    // Para overridar, criamos fake __dirname em req.seedsDir/../scripts
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
    if (req.op === 'parseFrontmatter') {
      vm.runInContext('globalThis.__pf = parseFrontmatter;', ctx);
      r = ctx.__pf(req.md);
    } else if (req.op === 'listSkills') {
      r = mod.listSkillsInPack(req.category, req.pack);
    } else if (req.op === 'apply') {
      r = mod.applySkills({
        category: req.category,
        pack: req.pack,
        names: req.names,
        memDir: req.memDir,
        projectRoot: req.projectRoot,
        overwrite: !!req.overwrite,
      });
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
        .join("../../../plugin/scripts/skills-install.cjs")
        .canonicalize()
        .expect("skills-install.cjs missing")
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

fn seed_pack(tmp: &Path) {
    write(
        &tmp.join("seeds/cat/pack/skills/s1/SKILL.md"),
        "---\ndescription: Skill 1\nrequires_mcp: true\nenv: [A, B]\n---\nbody1",
    );
    write(
        &tmp.join("seeds/cat/pack/skills/s1/mcp.json"),
        r#"{"command": "node", "args": ["s1.js"]}"#,
    );
    write(
        &tmp.join("seeds/cat/pack/skills/s2/SKILL.md"),
        "---\ndescription: Skill 2\n---\nbody2",
    );
}

// ── parse_frontmatter ────────────────────────────────────────────────────

fn cmp_fm(md: &str) {
    let js = run_node(json!({"op": "parseFrontmatter", "md": md}));
    let rs_map = parse_frontmatter(md);
    let rs: Value = serde_json::to_value(&rs_map).unwrap();
    assert_eq!(js, rs, "parseFrontmatter diverge para:\n{md}");
}

#[test]
fn parity_fm_empty() {
    if !node_available() {
        return;
    }
    cmp_fm("no frontmatter here");
}

#[test]
fn parity_fm_basic() {
    if !node_available() {
        return;
    }
    cmp_fm("---\ndescription: Hello\n---\nbody");
}

#[test]
fn parity_fm_bools() {
    if !node_available() {
        return;
    }
    cmp_fm("---\nrequires_mcp: true\nactive: false\n---");
}

#[test]
fn parity_fm_array() {
    if !node_available() {
        return;
    }
    cmp_fm("---\nenv: [FOO, BAR]\n---");
}

#[test]
fn parity_fm_empty_array() {
    if !node_available() {
        return;
    }
    cmp_fm("---\nenv: []\n---");
}

#[test]
fn parity_fm_quoted() {
    if !node_available() {
        return;
    }
    cmp_fm("---\na: \"hello\"\nb: 'world'\n---");
}

#[test]
fn parity_fm_hyphen_key_ignored() {
    if !node_available() {
        return;
    }
    cmp_fm("---\nskill-name: foo\nname: bar\n---");
}

// ── list_skills_in_pack ──────────────────────────────────────────────────

#[test]
fn parity_list_skills_empty() {
    if !node_available() {
        return;
    }
    let tmp = tempdir().unwrap();
    let seeds = tmp.path().join("seeds");
    fs::create_dir_all(&seeds).unwrap();
    let js = run_node(json!({
        "op": "listSkills",
        "seedsDir": seeds.to_string_lossy(),
        "category": "cat",
        "pack": "pack",
    }));
    let rs = list_skills_in_pack(&seeds, "cat", "pack");
    let rs_v: Value = serde_json::to_value(&rs).unwrap();
    assert_eq!(js, rs_v);
}

#[test]
fn parity_list_skills_with_data() {
    if !node_available() {
        return;
    }
    let tmp = tempdir().unwrap();
    seed_pack(tmp.path());
    let seeds = tmp.path().join("seeds");
    let js = run_node(json!({
        "op": "listSkills",
        "seedsDir": seeds.to_string_lossy(),
        "category": "cat",
        "pack": "pack",
    }));
    let rs = list_skills_in_pack(&seeds, "cat", "pack");
    let rs_v: Value = serde_json::to_value(&rs).unwrap();
    assert_eq!(js, rs_v);
}

// ── apply_skills ─────────────────────────────────────────────────────────

fn read_mcp(p: &Path) -> Option<Value> {
    fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

#[test]
fn parity_apply_fresh_install() {
    if !node_available() {
        return;
    }
    // JS side
    let tmp_js = tempdir().unwrap();
    seed_pack(tmp_js.path());
    let mem_js = tmp_js.path().join("mem");
    let proj_js = tmp_js.path().join("proj");
    fs::create_dir_all(&proj_js).unwrap();
    let js = run_node(json!({
        "op": "apply",
        "seedsDir": tmp_js.path().join("seeds").to_string_lossy(),
        "category": "cat",
        "pack": "pack",
        "memDir": mem_js.to_string_lossy(),
        "projectRoot": proj_js.to_string_lossy(),
        "overwrite": false,
    }));
    let js_mcp = read_mcp(&proj_js.join(".mcp.json"));

    // RS side
    let tmp_rs = tempdir().unwrap();
    seed_pack(tmp_rs.path());
    let mem_rs = tmp_rs.path().join("mem");
    let proj_rs = tmp_rs.path().join("proj");
    fs::create_dir_all(&proj_rs).unwrap();
    let rs = apply_skills(ApplyInput {
        seeds_dir: &tmp_rs.path().join("seeds"),
        category: "cat",
        pack: "pack",
        names: None,
        mem_dir: &mem_rs,
        project_root: &proj_rs,
        overwrite: false,
    })
    .unwrap();
    let rs_mcp = read_mcp(&proj_rs.join(".mcp.json"));

    assert_eq!(js, serde_json::to_value(&rs).unwrap(), "apply result");
    assert_eq!(js_mcp, rs_mcp, ".mcp.json shape");
}

#[test]
fn parity_apply_preserves_existing_mcp() {
    if !node_available() {
        return;
    }
    let existing = r#"{"mcpServers":{"s1":{"command":"existing"}}}"#;

    let tmp_js = tempdir().unwrap();
    seed_pack(tmp_js.path());
    let mem_js = tmp_js.path().join("mem");
    let proj_js = tmp_js.path().join("proj");
    fs::create_dir_all(&proj_js).unwrap();
    write(&proj_js.join(".mcp.json"), existing);
    let js = run_node(json!({
        "op": "apply",
        "seedsDir": tmp_js.path().join("seeds").to_string_lossy(),
        "category": "cat",
        "pack": "pack",
        "memDir": mem_js.to_string_lossy(),
        "projectRoot": proj_js.to_string_lossy(),
        "overwrite": false,
    }));
    let js_mcp = read_mcp(&proj_js.join(".mcp.json"));

    let tmp_rs = tempdir().unwrap();
    seed_pack(tmp_rs.path());
    let mem_rs = tmp_rs.path().join("mem");
    let proj_rs = tmp_rs.path().join("proj");
    fs::create_dir_all(&proj_rs).unwrap();
    write(&proj_rs.join(".mcp.json"), existing);
    let rs = apply_skills(ApplyInput {
        seeds_dir: &tmp_rs.path().join("seeds"),
        category: "cat",
        pack: "pack",
        names: None,
        mem_dir: &mem_rs,
        project_root: &proj_rs,
        overwrite: false,
    })
    .unwrap();
    let rs_mcp = read_mcp(&proj_rs.join(".mcp.json"));

    assert_eq!(js, serde_json::to_value(&rs).unwrap());
    assert_eq!(js_mcp, rs_mcp);
}

#[test]
fn parity_apply_with_names_filter() {
    if !node_available() {
        return;
    }
    let tmp_js = tempdir().unwrap();
    seed_pack(tmp_js.path());
    let mem_js = tmp_js.path().join("mem");
    let proj_js = tmp_js.path().join("proj");
    fs::create_dir_all(&proj_js).unwrap();
    let js = run_node(json!({
        "op": "apply",
        "seedsDir": tmp_js.path().join("seeds").to_string_lossy(),
        "category": "cat",
        "pack": "pack",
        "names": ["s2"],
        "memDir": mem_js.to_string_lossy(),
        "projectRoot": proj_js.to_string_lossy(),
        "overwrite": false,
    }));

    let tmp_rs = tempdir().unwrap();
    seed_pack(tmp_rs.path());
    let mem_rs = tmp_rs.path().join("mem");
    let proj_rs = tmp_rs.path().join("proj");
    fs::create_dir_all(&proj_rs).unwrap();
    let names = ["s2".to_string()];
    let rs = apply_skills(ApplyInput {
        seeds_dir: &tmp_rs.path().join("seeds"),
        category: "cat",
        pack: "pack",
        names: Some(&names),
        mem_dir: &mem_rs,
        project_root: &proj_rs,
        overwrite: false,
    })
    .unwrap();

    assert_eq!(js, serde_json::to_value(&rs).unwrap());
}

#[test]
fn parity_apply_overwrite_mcp() {
    if !node_available() {
        return;
    }
    let existing = r#"{"mcpServers":{"s1":{"command":"existing"}}}"#;

    let tmp_js = tempdir().unwrap();
    seed_pack(tmp_js.path());
    let mem_js = tmp_js.path().join("mem");
    let proj_js = tmp_js.path().join("proj");
    fs::create_dir_all(&proj_js).unwrap();
    write(&proj_js.join(".mcp.json"), existing);
    let js = run_node(json!({
        "op": "apply",
        "seedsDir": tmp_js.path().join("seeds").to_string_lossy(),
        "category": "cat",
        "pack": "pack",
        "memDir": mem_js.to_string_lossy(),
        "projectRoot": proj_js.to_string_lossy(),
        "overwrite": true,
    }));
    let js_mcp = read_mcp(&proj_js.join(".mcp.json"));

    let tmp_rs = tempdir().unwrap();
    seed_pack(tmp_rs.path());
    let mem_rs = tmp_rs.path().join("mem");
    let proj_rs = tmp_rs.path().join("proj");
    fs::create_dir_all(&proj_rs).unwrap();
    write(&proj_rs.join(".mcp.json"), existing);
    let rs = apply_skills(ApplyInput {
        seeds_dir: &tmp_rs.path().join("seeds"),
        category: "cat",
        pack: "pack",
        names: None,
        mem_dir: &mem_rs,
        project_root: &proj_rs,
        overwrite: true,
    })
    .unwrap();
    let rs_mcp = read_mcp(&proj_rs.join(".mcp.json"));

    assert_eq!(js, serde_json::to_value(&rs).unwrap());
    assert_eq!(js_mcp, rs_mcp);
}

#[test]
fn parity_apply_no_mcp_does_not_create_file() {
    if !node_available() {
        return;
    }
    let tmp_js = tempdir().unwrap();
    write(
        &tmp_js.path().join("seeds/cat/pack/skills/only/SKILL.md"),
        "---\ndescription: only\n---",
    );
    let mem_js = tmp_js.path().join("mem");
    let proj_js = tmp_js.path().join("proj");
    fs::create_dir_all(&proj_js).unwrap();
    let js = run_node(json!({
        "op": "apply",
        "seedsDir": tmp_js.path().join("seeds").to_string_lossy(),
        "category": "cat",
        "pack": "pack",
        "memDir": mem_js.to_string_lossy(),
        "projectRoot": proj_js.to_string_lossy(),
        "overwrite": false,
    }));
    let js_exists = proj_js.join(".mcp.json").exists();

    let tmp_rs = tempdir().unwrap();
    write(
        &tmp_rs.path().join("seeds/cat/pack/skills/only/SKILL.md"),
        "---\ndescription: only\n---",
    );
    let mem_rs = tmp_rs.path().join("mem");
    let proj_rs = tmp_rs.path().join("proj");
    fs::create_dir_all(&proj_rs).unwrap();
    let rs = apply_skills(ApplyInput {
        seeds_dir: &tmp_rs.path().join("seeds"),
        category: "cat",
        pack: "pack",
        names: None,
        mem_dir: &mem_rs,
        project_root: &proj_rs,
        overwrite: false,
    })
    .unwrap();
    let rs_exists = proj_rs.join(".mcp.json").exists();

    assert_eq!(js, serde_json::to_value(&rs).unwrap());
    assert_eq!(js_exists, rs_exists);
}
