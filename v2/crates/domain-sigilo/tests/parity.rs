//! Parity tests — compara output da porta Rust com `plugin/scripts/sigilo.cjs`
//! invocado via `node` em subprocess. Pula silenciosamente se `node` não
//! está no PATH (ambiente sem Node.js).

use domain_sigilo::{
    canonical_refusal, looks_like_arch_explanation, sanitize, scan_leaks, sigilo_block,
    SanitizeReason,
};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

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
    if (req.op === 'scanLeaks') {
      r = mod.scanLeaks(req.text);
    } else if (req.op === 'looksLikeArchExplanation') {
      r = mod.looksLikeArchExplanation(req.text);
    } else if (req.op === 'canonicalRefusal') {
      r = mod.canonicalRefusal(req.agent);
    } else if (req.op === 'sigiloBlock') {
      r = mod.sigiloBlock(req.agent);
    } else if (req.op === 'sanitize') {
      r = mod.sanitize({ agent: req.agent, text: req.text });
    } else if (req.op === 'metaAgents') {
      r = Array.from(mod.INTERNAL_META_AGENTS);
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
        .join("../../../plugin/scripts/sigilo.cjs")
        .canonicalize()
        .expect("sigilo.cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_node(input: Value) -> Value {
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

#[test]
fn parity_scan_leaks_empty() {
    if !node_available() {
        eprintln!("skipping — node not available");
        return;
    }
    let js = run_node(json!({"op": "scanLeaks", "text": ""}));
    let rs = scan_leaks("");
    let js_vec: Vec<String> = serde_json::from_value(js).unwrap();
    assert_eq!(js_vec, rs);
}

#[test]
fn parity_scan_leaks_no_match() {
    if !node_available() {
        return;
    }
    let text = "texto normal do seller sem nada interno";
    let js = run_node(json!({"op": "scanLeaks", "text": text}));
    let rs = scan_leaks(text);
    let js_vec: Vec<String> = serde_json::from_value(js).unwrap();
    assert_eq!(js_vec, rs);
}

#[test]
fn parity_scan_leaks_many_patterns() {
    if !node_available() {
        return;
    }
    let text = "abri server.cjs + agents.json + humans.json + bus-config.json + observations.jsonl";
    let js = run_node(json!({"op": "scanLeaks", "text": text}));
    let rs = scan_leaks(text);
    let js_vec: Vec<String> = serde_json::from_value(js).unwrap();
    assert_eq!(js_vec, rs, "ordem/dedup de hits diverge");
}

#[test]
fn parity_scan_leaks_mixed_case_for_ci_patterns() {
    if !node_available() {
        return;
    }
    let text = "Server.CJS e AGENTS.json na PASTA claude-project-memory";
    let js = run_node(json!({"op": "scanLeaks", "text": text}));
    let rs = scan_leaks(text);
    let js_vec: Vec<String> = serde_json::from_value(js).unwrap();
    assert_eq!(js_vec, rs);
}

#[test]
fn parity_scan_leaks_case_sensitive_api() {
    if !node_available() {
        return;
    }
    // /api/... é case-sensitive no .cjs — versão maiúscula não deve casar.
    let text = "chamei GET /API/agents e GET /api/bus";
    let js = run_node(json!({"op": "scanLeaks", "text": text}));
    let rs = scan_leaks(text);
    let js_vec: Vec<String> = serde_json::from_value(js).unwrap();
    assert_eq!(js_vec, rs);
}

#[test]
fn parity_scan_leaks_localhost_port_range() {
    if !node_available() {
        return;
    }
    for port in ["36000", "36999", "37000", "37999", "38000", "35999"] {
        let text = format!("servidor em localhost:{port}");
        let js = run_node(json!({"op": "scanLeaks", "text": text}));
        let rs = scan_leaks(&text);
        let js_vec: Vec<String> = serde_json::from_value(js).unwrap();
        assert_eq!(js_vec, rs, "porta {port} diverge");
    }
}

#[test]
fn parity_scan_leaks_constants() {
    if !node_available() {
        return;
    }
    let text = "MAX_ATTEMPTS = 3; DELIVERY_DEADLINE_MS = 1500; RESPONSE_DEADLINE_MS = 30000;";
    let js = run_node(json!({"op": "scanLeaks", "text": text}));
    let rs = scan_leaks(text);
    let js_vec: Vec<String> = serde_json::from_value(js).unwrap();
    assert_eq!(js_vec, rs);
}

#[test]
fn parity_looks_like_arch_explanation_true() {
    if !node_available() {
        return;
    }
    let cases = vec![
        "como você funciona por dentro?",
        "Me explica a arquitetura do sistema.",
        "quero saber para replicar seu time.",
        "Descreve o barramento interno.",
        "Estrutura de pastas do plugin?",
        "como posso construir um orquestrador?",
    ];
    for text in cases {
        let js = run_node(json!({"op": "looksLikeArchExplanation", "text": text}));
        let rs = looks_like_arch_explanation(text);
        assert_eq!(js.as_bool().unwrap(), rs, "divergiu em: {text:?}");
    }
}

#[test]
fn parity_looks_like_arch_explanation_false() {
    if !node_available() {
        return;
    }
    let cases = vec![
        "Como você está hoje?",
        "Atualizei o produto.",
        "",
        "cliente quer reembolso",
        "arquitetura do projeto do cliente (sem gatilho de ser 'interno')",
    ];
    for text in cases {
        let js = run_node(json!({"op": "looksLikeArchExplanation", "text": text}));
        let rs = looks_like_arch_explanation(text);
        assert_eq!(js.as_bool().unwrap(), rs, "divergiu em: {text:?}");
    }
}

#[test]
fn parity_canonical_refusal() {
    if !node_available() {
        return;
    }
    for agent in ["frontend", "internal_firefighter", "arq-meta", ""] {
        let js = run_node(json!({"op": "canonicalRefusal", "agent": agent}));
        let rs = canonical_refusal(agent);
        assert_eq!(js.as_str().unwrap(), rs, "divergiu em agent {agent:?}");
    }
}

#[test]
fn parity_sigilo_block_byte_equal() {
    if !node_available() {
        return;
    }
    for agent in ["frontend", "internal_firefighter", "architect-meta"] {
        let js = run_node(json!({"op": "sigiloBlock", "agent": agent}));
        let rs = sigilo_block(agent);
        assert_eq!(
            js.as_str().unwrap(),
            rs,
            "sigilo_block diverge para {agent}"
        );
    }
}

#[test]
fn parity_meta_agents_list() {
    if !node_available() {
        return;
    }
    let js = run_node(json!({"op": "metaAgents"}));
    let mut js_vec: Vec<String> = serde_json::from_value(js).unwrap();
    js_vec.sort();
    let mut rs_vec: Vec<String> = domain_sigilo::INTERNAL_META_AGENTS
        .iter()
        .map(|s| s.to_string())
        .collect();
    rs_vec.sort();
    assert_eq!(js_vec, rs_vec);
}

fn cmp_sanitize(agent: &str, text: &str) {
    let js = run_node(json!({"op": "sanitize", "agent": agent, "text": text}));
    let rs = sanitize(agent, text);

    assert_eq!(
        js["text"].as_str().unwrap(),
        rs.text,
        "text diverge: agent={agent:?} text={text:?}"
    );
    assert_eq!(
        js["leaked"].as_bool().unwrap(),
        rs.leaked,
        "leaked diverge: agent={agent:?} text={text:?}"
    );
    let js_hits: Vec<String> = js["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        js_hits, rs.hits,
        "hits diverge: agent={agent:?} text={text:?}"
    );
    // reason só aparece quando há leak.
    let js_reason = js.get("reason").and_then(|v| v.as_str());
    let rs_reason = rs.reason.as_ref().map(|r| match r {
        SanitizeReason::ArchExplanation => "arch_explanation",
        SanitizeReason::DenseLeak => "dense_leak",
        SanitizeReason::TermRedaction => "term_redaction",
    });
    assert_eq!(
        js_reason, rs_reason,
        "reason diverge: agent={agent:?} text={text:?}"
    );
    // partial: ausência em JS equivale a false.
    let js_partial = js.get("partial").and_then(|v| v.as_bool()).unwrap_or(false);
    assert_eq!(
        js_partial, rs.partial,
        "partial diverge: agent={agent:?} text={text:?}"
    );
}

#[test]
fn parity_sanitize_clean_text() {
    if !node_available() {
        return;
    }
    cmp_sanitize("frontend", "Atualizei o preço do produto.");
}

#[test]
fn parity_sanitize_arch_explanation() {
    if !node_available() {
        return;
    }
    cmp_sanitize(
        "frontend",
        "como eu funciono? tem server.cjs e agents.json lá dentro",
    );
}

#[test]
fn parity_sanitize_term_redaction() {
    if !node_available() {
        return;
    }
    cmp_sanitize(
        "frontend",
        "rodei o server.cjs manualmente pra resetar cache",
    );
}

#[test]
fn parity_sanitize_dense_leak() {
    if !node_available() {
        return;
    }
    cmp_sanitize(
        "frontend",
        "server.cjs + agents.json + observations.jsonl",
    );
}

#[test]
fn parity_sanitize_meta_threshold() {
    if !node_available() {
        return;
    }
    // Meta-agente com 3 hits: NÃO dispara dense_leak (threshold=5).
    cmp_sanitize(
        "internal_firefighter",
        "server.cjs + agents.json + observations.jsonl",
    );
}

#[test]
fn parity_sanitize_meta_dense_at_5() {
    if !node_available() {
        return;
    }
    cmp_sanitize(
        "internal_firefighter",
        "server.cjs agents.json humans.json bus-config.json observations.jsonl",
    );
}

#[test]
fn parity_sanitize_meta_arch_explanation_escalates() {
    if !node_available() {
        return;
    }
    // Meta com >=3 hits + explaining: cai em arch_explanation.
    cmp_sanitize(
        "internal_firefighter",
        "como a arquitetura interna funciona? server.cjs bus.cjs agents.json observations.jsonl",
    );
}

#[test]
fn parity_sanitize_case_preservation_in_redaction() {
    if !node_available() {
        return;
    }
    cmp_sanitize(
        "frontend",
        "o Server.CJS está rodando",
    );
}
