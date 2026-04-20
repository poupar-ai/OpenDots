//! `buildInjectionBlock` — porta de `agents.cjs:486-547`.
//!
//! Produz o bloco markdown de agentes que o session-init do plugin
//! injeta no contexto do Claude Code. Entrada: `agents.json` +
//! (opcional) mini-contextos. Saída: `Vec<String>` (uma linha por
//! entry).
//!
//! ## Estrutura do bloco
//!
//! 1. `## Agentes do projeto` (sempre primeiro se cfg existir)
//! 2. (opcional) `> **Regra de ouro:** <rule>` + linha vazia
//! 3. Intro `Delegue via Task tool com \`subagent_type\` apropriado. ...`
//! 4. 5 grupos, na ordem fixa: core, specialist, integrator, motor,
//!    firefighter. Cada grupo só aparece se tiver ≥1 agente.
//! 5. Por agente: emoji, nome (com tag Manager/Oracle se for), role,
//!    modelo (opcional), MCPs, watch (firefighter), can_talk_to e
//!    snippet de mini-contexto.
//! 6. Footer com regra final (uma linha vazia antes).
//!
//! ## Detalhes de paridade
//!
//! - **Agente com `type` desconhecido** cai em `specialist` (linha 509).
//! - **Snippet de mini-contexto:** `ctx.split('\n').slice(0,4).join(' ').replace(/#+\s*/g,'').slice(0,140)`.
//!   Replicamos:
//!   - `split('\n')`: split por `\n` apenas (não `\r\n`).
//!   - `slice(0,4)`: primeiras 4 linhas (pode ser menos).
//!   - `join(' ')`: concatena com espaço.
//!   - `replace(/#+\s*/g,'')`: strip `#+` seguidos por whitespace opcional.
//!   - `slice(0,140)`: primeiras 140 UTF-16 code units (paridade com
//!     `String.prototype.slice` do JS).
//! - **Truthy check** no `cfg.rule`, `a.emoji`, `a.model`, `a.mcps`,
//!   `a.watch`, `a.can_talk_to`: para inputs realistas (string/array),
//!   match simples. Valores exóticos (objeto truthy sem `.length`)
//!   são tratados conforme semântica JS.
//! - **`a.watch && a.watch.query`**: ambos truthy. Query vazia skipa.

use crate::{is_valid_agent_type, load_manifest, load_mini_context};
use serde_json::Value;
use std::path::Path;

const INTRO: &str =
    "Delegue via Task tool com `subagent_type` apropriado. Respeite o tipo de cada agente:";

const FOOTER: &str = "> Antes de finalizar uma tarefa complexa: delegue pro agente certo (Task) e reporte resultado consolidado ao Manager. Motores nunca executam sem confirmação.";

const GROUP_ORDER: [&str; 5] = ["core", "specialist", "integrator", "motor", "firefighter"];

fn group_title(ty: &str) -> &'static str {
    match ty {
        "core" => "### Core (sistema)",
        "specialist" => "### Especialistas (conhecimento interno)",
        "integrator" => "### Integradores (conhecimento externo — consultam, não agem)",
        "motor" => "### Motores (ação no mundo — requer confirmação do Manager)",
        "firefighter" => "### Bombeiros (reativos — escutam logs, diagnosticam, escalam)",
        _ => unreachable!("group_title só é chamado com tipos válidos"),
    }
}

/// Coerce um `Option<&Value>` para string no estilo de template
/// literal JS: `${x}`. Missing → `"undefined"`; null → `"null"`;
/// primitivos → representação textual; array → join(","); objeto →
/// `"[object Object]"`.
fn js_template_str(v: Option<&Value>) -> String {
    match v {
        None => "undefined".to_string(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| js_template_str(Some(x)))
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".to_string(),
    }
}

/// `true` se `value` é truthy no sentido JS.
///
/// Usado para reproduzir `if (a.model)`, `if (cfg.rule)` etc.
fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|x| x != 0.0 && !x.is_nan()).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// Slice `s` pelos primeiros `max` UTF-16 code units, preservando
/// apenas code points completos. Paridade aproximada com
/// `String.prototype.slice(0, max)` do JS, que pode cortar surrogate
/// pairs pela metade — mas como strings realistas aqui não contêm
/// code points fora do BMP em posição de corte, a diferença não se
/// manifesta em tests de paridade.
fn slice_utf16(s: &str, max: usize) -> String {
    let mut total = 0;
    let mut out = String::new();
    for c in s.chars() {
        let len = c.len_utf16();
        if total + len > max {
            break;
        }
        out.push(c);
        total += len;
    }
    out
}

/// Strip `#+\s*` globalmente (equivalente a `/#+\s*/g` do JS).
///
/// - `#+` — um ou mais `#` consecutivos.
/// - `\s*` — zero ou mais whitespace (usa `char::is_whitespace` para
///   cobrir a maioria dos whitespace Unicode que `\s` do JS match).
fn strip_headings(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '#' {
            while i < chars.len() && chars[i] == '#' {
                i += 1;
            }
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Produz o snippet curto de mini-contexto (linha `  · _<snippet>…_`).
///
/// Retorna `None` se o resultado final for vazio (JS: `if (short)`).
fn mini_snippet(ctx: &str) -> Option<String> {
    let joined = ctx
        .split('\n')
        .take(4)
        .collect::<Vec<_>>()
        .join(" ");
    let stripped = strip_headings(&joined);
    let short = slice_utf16(&stripped, 140);
    if short.is_empty() {
        None
    } else {
        Some(short)
    }
}

/// Formata lista de strings como `` `a`, `b`, `c` ``.
fn backtick_join(items: &[Value]) -> String {
    items
        .iter()
        .map(|v| format!("`{}`", v.as_str().unwrap_or("")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Constrói o bloco injetável.
///
/// Retorna `[]` se:
/// - manifest inexistente;
/// - manifest sem campo `agents` (ou `agents` não é objeto).
pub fn build_injection_block(mem_dir: &Path) -> Vec<String> {
    let cfg = match load_manifest(mem_dir) {
        Some(c) => c,
        None => return vec![],
    };
    let agents = match cfg.get("agents").and_then(Value::as_object) {
        Some(m) if !m.is_empty() => m,
        // JS: `!cfg.agents` — qualquer falsy retorna []. Objeto vazio
        // é truthy em JS (passa no check), mas produz 0 grupos e sai
        // com só header + intro. Preservamos isso retornando o shape
        // resultante abaixo. Para null/missing, retornamos [].
        Some(m) => m,
        None => return vec![],
    };

    let mut lines = vec!["## Agentes do projeto".to_string()];

    // Rule
    if let Some(rule_val) = cfg.get("rule") {
        if is_truthy(rule_val) {
            // JS `${cfg.rule}` coerce para string.
            let rule_str = rule_val.as_str().map(String::from).unwrap_or_else(|| {
                // Para non-string truthy (obj/array/number), serialize como JSON.
                // Este caminho é raro; paridade aproximada.
                serde_json::to_string(rule_val).unwrap_or_default()
            });
            lines.push(format!("> **Regra de ouro:** {rule_str}"));
            lines.push(String::new());
        }
    }

    // Manager / oracle para tag
    let manager = cfg
        .get("manager")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("manager");
    let oracle = cfg.get("oracle").and_then(Value::as_str).unwrap_or("");

    // Agrupa por tipo (unknown → specialist).
    let mut groups: std::collections::HashMap<&'static str, Vec<(String, Value)>> =
        std::collections::HashMap::new();
    for t in GROUP_ORDER {
        groups.insert(t, Vec::new());
    }

    for (name, a) in agents {
        let ty_raw = a.get("type").and_then(Value::as_str).unwrap_or("");
        let ty: &'static str = if is_valid_agent_type(ty_raw) {
            // Resolve para &'static str estável.
            match ty_raw {
                "core" => "core",
                "specialist" => "specialist",
                "integrator" => "integrator",
                "motor" => "motor",
                "firefighter" => "firefighter",
                _ => "specialist",
            }
        } else {
            "specialist"
        };
        groups.get_mut(ty).unwrap().push((name.clone(), a.clone()));
    }

    lines.push(INTRO.to_string());

    for key in GROUP_ORDER {
        let list = &groups[key];
        if list.is_empty() {
            continue;
        }
        lines.push(String::new());
        lines.push(group_title(key).to_string());

        for (name, a) in list {
            // emoji com fallback '•'
            let emoji = a
                .get("emoji")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("•");

            // tag Manager / Oracle
            let is_manager = name == manager;
            let is_oracle = !is_manager && !oracle.is_empty() && name == oracle;
            let tag = if is_manager {
                " **(Manager)**"
            } else if is_oracle {
                " _(Oracle)_"
            } else {
                ""
            };

            // role — JS `${a.role}` coage undefined → "undefined",
            // null → "null", etc. Preservamos com js_template_str.
            let role = js_template_str(a.get("role"));

            lines.push(format!("- {emoji} **{name}**{tag} — {role}"));

            // modelo
            if let Some(m) = a.get("model").and_then(Value::as_str) {
                if !m.is_empty() {
                    lines.push(format!("  · modelo: `{m}`"));
                }
            }

            // MCPs: `a.mcps && a.mcps.length`
            if let Some(mcps) = a.get("mcps").and_then(Value::as_array) {
                if !mcps.is_empty() {
                    lines.push(format!("  · MCPs: {}", backtick_join(mcps)));
                }
            }

            // watch (firefighter)
            if let Some(watch) = a.get("watch").and_then(Value::as_object) {
                let query = watch.get("query").and_then(Value::as_str).unwrap_or("");
                if !query.is_empty() {
                    let source = watch
                        .get("source")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("datadog");
                    let threshold = js_template_str(watch.get("threshold"));
                    let window = js_template_str(watch.get("window_minutes"));
                    lines.push(format!(
                        "  · observa: `{query}` ({source}, threshold {threshold}/{window}min)"
                    ));
                }
            }

            // can_talk_to
            if let Some(ctt) = a.get("can_talk_to").and_then(Value::as_array) {
                if !ctt.is_empty() {
                    lines.push(format!("  · fala com: {}", backtick_join(ctt)));
                }
            }

            // mini-context snippet
            if let Some(context_file) = a.get("context_file").and_then(Value::as_str) {
                if let Some(ctx) = load_mini_context(mem_dir, context_file) {
                    if let Some(short) = mini_snippet(&ctx) {
                        lines.push(format!("  · _{short}…_"));
                    }
                }
            }
        }
    }

    lines.push(String::new());
    lines.push(FOOTER.to_string());

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{default_manifest, save_manifest};
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    fn setup_default(tmp: &TempDir) {
        fs::create_dir_all(tmp.path().join("agents")).unwrap();
        save_manifest(tmp.path(), &default_manifest()).unwrap();
    }

    #[test]
    fn returns_empty_vec_when_no_manifest() {
        let tmp = TempDir::new().unwrap();
        assert!(build_injection_block(tmp.path()).is_empty());
    }

    #[test]
    fn returns_empty_vec_when_no_agents_key() {
        let tmp = TempDir::new().unwrap();
        save_manifest(tmp.path(), &json!({"version": 1})).unwrap();
        assert!(build_injection_block(tmp.path()).is_empty());
    }

    #[test]
    fn header_is_first_line() {
        let tmp = TempDir::new().unwrap();
        setup_default(&tmp);
        let lines = build_injection_block(tmp.path());
        assert_eq!(lines[0], "## Agentes do projeto");
    }

    #[test]
    fn rule_line_appears_when_present() {
        let tmp = TempDir::new().unwrap();
        setup_default(&tmp);
        let lines = build_injection_block(tmp.path());
        assert!(lines.iter().any(|l| l.contains("Regra de ouro")));
    }

    #[test]
    fn all_default_agents_listed() {
        let tmp = TempDir::new().unwrap();
        setup_default(&tmp);
        let joined = build_injection_block(tmp.path()).join("\n");
        for name in [
            "manager",
            "oracle",
            "architect",
            "reviewer",
            "tester",
            "designer",
            "pipeline_writer",
        ] {
            assert!(
                joined.contains(&format!("**{name}**")),
                "missing agent {name}"
            );
        }
    }

    #[test]
    fn manager_has_manager_tag() {
        let tmp = TempDir::new().unwrap();
        setup_default(&tmp);
        let joined = build_injection_block(tmp.path()).join("\n");
        assert!(joined.contains("**manager** **(Manager)**"));
    }

    #[test]
    fn oracle_has_oracle_tag() {
        let tmp = TempDir::new().unwrap();
        setup_default(&tmp);
        let joined = build_injection_block(tmp.path()).join("\n");
        assert!(joined.contains("**oracle** _(Oracle)_"));
    }

    #[test]
    fn group_order_is_fixed() {
        let tmp = TempDir::new().unwrap();
        setup_default(&tmp);
        let lines = build_injection_block(tmp.path());
        // core aparece antes de specialist
        let core_pos = lines.iter().position(|l| l.contains("Core")).unwrap();
        let spec_pos = lines
            .iter()
            .position(|l| l.contains("Especialistas"))
            .unwrap();
        assert!(core_pos < spec_pos);
    }

    #[test]
    fn unknown_type_falls_into_specialist() {
        let tmp = TempDir::new().unwrap();
        save_manifest(
            tmp.path(),
            &json!({
                "version": 1,
                "manager": "manager",
                "agents": {
                    "weirdo": {"type": "ghost", "role": "I am unknown"}
                }
            }),
        )
        .unwrap();
        let joined = build_injection_block(tmp.path()).join("\n");
        assert!(joined.contains("Especialistas"));
        assert!(joined.contains("**weirdo**"));
    }

    #[test]
    fn mcps_list_rendered_with_backticks() {
        let tmp = TempDir::new().unwrap();
        save_manifest(
            tmp.path(),
            &json!({
                "version": 1,
                "manager": "manager",
                "agents": {
                    "bot": {"type": "motor", "role": "x", "mcps": ["a", "b"]}
                }
            }),
        )
        .unwrap();
        let joined = build_injection_block(tmp.path()).join("\n");
        assert!(joined.contains("MCPs: `a`, `b`"));
    }

    #[test]
    fn empty_mcps_not_rendered() {
        let tmp = TempDir::new().unwrap();
        save_manifest(
            tmp.path(),
            &json!({
                "version": 1,
                "manager": "manager",
                "agents": {
                    "bot": {"type": "core", "role": "x", "mcps": []}
                }
            }),
        )
        .unwrap();
        let joined = build_injection_block(tmp.path()).join("\n");
        assert!(!joined.contains("MCPs:"));
    }

    #[test]
    fn watch_rendered_for_firefighter() {
        let tmp = TempDir::new().unwrap();
        save_manifest(
            tmp.path(),
            &json!({
                "version": 1,
                "manager": "manager",
                "agents": {
                    "smoke": {
                        "type": "firefighter",
                        "role": "x",
                        "watch": {
                            "source": "datadog",
                            "query": "service:api status:error",
                            "threshold": 10,
                            "window_minutes": 15
                        }
                    }
                }
            }),
        )
        .unwrap();
        let joined = build_injection_block(tmp.path()).join("\n");
        assert!(joined
            .contains("observa: `service:api status:error` (datadog, threshold 10/15min)"));
    }

    #[test]
    fn snippet_strips_headings() {
        assert_eq!(strip_headings("# Title"), "Title");
        assert_eq!(strip_headings("## Sub ### Nested"), "Sub Nested");
        assert_eq!(strip_headings("no hash"), "no hash");
        assert_eq!(strip_headings("####Heading"), "Heading");
    }

    #[test]
    fn slice_utf16_respects_limit() {
        assert_eq!(slice_utf16("hello", 3), "hel");
        assert_eq!(slice_utf16("hello", 100), "hello");
        assert_eq!(slice_utf16("", 5), "");
    }

    #[test]
    fn footer_last() {
        let tmp = TempDir::new().unwrap();
        setup_default(&tmp);
        let lines = build_injection_block(tmp.path());
        let last = lines.last().unwrap();
        assert!(last.contains("reporte resultado consolidado"));
    }
}
