//! Paridade v1 (`agents.cjs::integratorTemplate`/`firefighterTemplate`/
//! `motorTemplate`) vs v2 (`domain_agents::templates::*`).
//!
//! Comparação byte-a-byte: os templates geram markdown que vai direto
//! pro disco como mini-context, então qualquer diferença de whitespace
//! ou linebreak conta como regressão.

use domain_agents::{firefighter_template, integrator_template, motor_template};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const NODE_SCRIPT: &str = r#"
const lib = require(process.argv[1]);
const op = process.argv[2];

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  const payload = input ? JSON.parse(input) : {};
  let out;
  if (op === 'integratorTemplate') out = lib.integratorTemplate(payload);
  else if (op === 'firefighterTemplate') out = lib.firefighterTemplate(payload);
  else if (op === 'motorTemplate') out = lib.motorTemplate(payload);
  else { process.stderr.write('op desconhecida'); process.exit(2); }
  process.stdout.write(out);
});
"#;

fn lib_cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/agents.cjs")
        .canonicalize()
        .expect("agents.cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn call_v1(op: &str, payload: &Value) -> String {
    let mut child = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(lib_cjs_path())
        .arg(op)
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
    assert!(
        out.status.success(),
        "node falhou: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8")
}

fn assert_template_match(op: &str, payload: Value) {
    if !node_available() {
        return;
    }
    let v1 = call_v1(op, &payload);
    let v2 = match op {
        "integratorTemplate" => integrator_template(&payload),
        "firefighterTemplate" => firefighter_template(&payload),
        "motorTemplate" => motor_template(&payload),
        _ => panic!("op"),
    };
    if v1 != v2 {
        // diff rápido — imprime as primeiras divergências
        let v1_bytes = v1.as_bytes();
        let v2_bytes = v2.as_bytes();
        let first_diff = v1_bytes
            .iter()
            .zip(v2_bytes.iter())
            .position(|(a, b)| a != b);
        panic!(
            "Template diverge em {op}.\nprimeira diferença: {first_diff:?}\nlen v1={}, v2={}\n\n--- V1 ---\n{v1}\n\n--- V2 ---\n{v2}",
            v1_bytes.len(),
            v2_bytes.len()
        );
    }
}

// ── integrator_template ────────────────────────────────────────────────────

#[test]
fn integrator_minimal() {
    assert_template_match("integratorTemplate", json!({"name": "gh"}));
}

#[test]
fn integrator_with_service() {
    assert_template_match(
        "integratorTemplate",
        json!({"name": "gh", "service": "GitHub"}),
    );
}

#[test]
fn integrator_full_fields() {
    assert_template_match(
        "integratorTemplate",
        json!({
            "name": "gh",
            "service": "GitHub",
            "description": "Integrator pra GitHub API v3.",
            "base_url": "https://api.github.com",
            "auth": "Bearer GH_TOKEN no header Authorization",
            "env_vars": ["GH_TOKEN", "GH_USER"],
            "endpoints": [
                {"method": "POST", "path": "/repos", "purpose": "Criar repo"},
                {"method": "GET", "path": "/user"},
                {"path": "/search/issues", "purpose": "busca"}
            ],
            "webhooks": [
                {"event": "push", "purpose": "commit"},
                {"event": "pull_request"}
            ],
            "docs_url": "https://docs.github.com/rest",
            "gotchas": ["Rate limit 5k/h", "Tokens PAT expiram"]
        }),
    );
}

#[test]
fn integrator_empty_arrays_use_fallback() {
    assert_template_match(
        "integratorTemplate",
        json!({
            "name": "x",
            "env_vars": [],
            "endpoints": [],
            "webhooks": [],
            "gotchas": []
        }),
    );
}

#[test]
fn integrator_endpoint_no_purpose() {
    assert_template_match(
        "integratorTemplate",
        json!({"name": "x", "endpoints": [{"path": "/foo"}]}),
    );
}

#[test]
fn integrator_webhook_no_purpose() {
    assert_template_match(
        "integratorTemplate",
        json!({"name": "x", "webhooks": [{"event": "ping"}]}),
    );
}

#[test]
fn integrator_empty_docs_url_fallback() {
    // JS: `docs_url ? `- ${docs_url}` : '- _Adicionar...'` — "" é falsy
    assert_template_match(
        "integratorTemplate",
        json!({"name": "x", "docs_url": ""}),
    );
}

#[test]
fn integrator_empty_base_url_fallback() {
    assert_template_match(
        "integratorTemplate",
        json!({"name": "x", "base_url": ""}),
    );
}

// ── firefighter_template ───────────────────────────────────────────────────

#[test]
fn firefighter_minimal() {
    assert_template_match("firefighterTemplate", json!({"name": "alerta"}));
}

#[test]
fn firefighter_with_service() {
    assert_template_match(
        "firefighterTemplate",
        json!({"name": "alerta", "service": "api"}),
    );
}

#[test]
fn firefighter_full_fields() {
    assert_template_match(
        "firefighterTemplate",
        json!({
            "name": "alerta",
            "service": "api",
            "description": "Triage de erros no serviço de pagamentos.",
            "datadog_query": "service:api status:error env:prod",
            "threshold": 10,
            "window_minutes": 15,
            "cooldown_seconds": 1800,
            "max_triages_per_hour": 2,
            "env_vars": ["DD_API_KEY", "CUSTOM"],
            "runbook": [
                "Olhar dashboard X",
                "Checar fila Y",
                "Verificar deploys recentes"
            ],
            "escalation": [
                "Chamar plantão via PagerDuty",
                "Escalar Engenheiro se durar > 30min"
            ]
        }),
    );
}

#[test]
fn firefighter_empty_arrays() {
    assert_template_match(
        "firefighterTemplate",
        json!({"name": "x", "runbook": [], "escalation": []}),
    );
}

#[test]
fn firefighter_zero_threshold_falls_default() {
    // JS: `threshold || 5` — 0 cai no default 5
    assert_template_match(
        "firefighterTemplate",
        json!({"name": "x", "threshold": 0}),
    );
}

// ── motor_template ─────────────────────────────────────────────────────────

#[test]
fn motor_minimal() {
    assert_template_match("motorTemplate", json!({"name": "ship"}));
}

#[test]
fn motor_with_action() {
    assert_template_match(
        "motorTemplate",
        json!({"name": "ship", "action": "deploy"}),
    );
}

#[test]
fn motor_full_fields() {
    assert_template_match(
        "motorTemplate",
        json!({
            "name": "auto",
            "action": "deploy",
            "description": "Motor de deploy via GitHub Actions.",
            "mcps": ["gh", "aws"],
            "tools": ["Read", "Bash"],
            "env_vars": ["AWS_KEY", "AWS_SECRET"],
            "allowed_actions": ["deploy staging", "rollback"],
            "forbidden_actions": ["deploy prod sem PR aprovado"],
            "requires_manager_approval": false
        }),
    );
}

#[test]
fn motor_requires_approval_true() {
    assert_template_match(
        "motorTemplate",
        json!({"name": "x", "requires_manager_approval": true}),
    );
}

#[test]
fn motor_requires_approval_missing() {
    assert_template_match(
        "motorTemplate",
        json!({"name": "x"}),
    );
}

#[test]
fn motor_requires_approval_truthy_string() {
    // JS: `x === false` — só booleano false casa. String não-vazia é
    // truthy mas não === false → qualifier "relevante"
    assert_template_match(
        "motorTemplate",
        json!({"name": "x", "requires_manager_approval": "yes"}),
    );
}

#[test]
fn motor_requires_approval_null() {
    assert_template_match(
        "motorTemplate",
        json!({"name": "x", "requires_manager_approval": null}),
    );
}

#[test]
fn motor_empty_arrays() {
    assert_template_match(
        "motorTemplate",
        json!({
            "name": "x",
            "mcps": [],
            "tools": [],
            "env_vars": [],
            "allowed_actions": [],
            "forbidden_actions": []
        }),
    );
}
