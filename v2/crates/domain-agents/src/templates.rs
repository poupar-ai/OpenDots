//! Templates de mini-context — porta de `agents.cjs:286-404`.
//!
//! Três funções puras que recebem um objeto de config e devolvem
//! markdown. Usadas pela UI pra pré-popular o mini-context ao criar
//! um integrator/firefighter/motor.
//!
//! Paridade byte-a-byte com o `.cjs` — mesmas strings de fallback,
//! mesma ordem de campos, mesmas quebras de linha.

use serde_json::Value;

// ── helpers ────────────────────────────────────────────────────────────────

/// `config[key]` como string ou `default` se ausente/vazio.
///
/// Replica `config.key || default` do JS — `""`, `null`, `undefined`
/// caem no default.
fn s_or<'a>(config: &'a Value, key: &str, default: &'a str) -> String {
    config
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .unwrap_or(default)
        .to_string()
}

/// Igual a [`s_or`] mas preserva a string vazia; só preenche default se
/// for `undefined`/`null`. Útil pra paridade quando queremos replicar
/// `?:` em vez de `||` — aqui não é usado, documentado pra referência.
#[allow(dead_code)]
fn s_or_null<'a>(config: &'a Value, key: &str, default: &'a str) -> String {
    match config.get(key) {
        Some(Value::String(s)) => s.clone(),
        _ => default.to_string(),
    }
}

/// Extrai `config[key]` como array (mesmo que esteja ausente — aí `[]`).
fn arr_or_empty<'a>(config: &'a Value, key: &str) -> &'a [Value] {
    static EMPTY: Vec<Value> = Vec::new();
    config
        .get(key)
        .and_then(Value::as_array)
        .map(|a| a.as_slice())
        .unwrap_or(&EMPTY)
}

/// JS `num || default` — 0, null, undefined → default.
fn n_or(config: &Value, key: &str, default: i64) -> i64 {
    match config.get(key) {
        Some(n) if n.is_number() && n.as_f64() != Some(0.0) => {
            n.as_i64().unwrap_or_else(|| n.as_f64().unwrap() as i64)
        }
        _ => default,
    }
}

/// Coage qualquer Value pra string no estilo template literal JS.
/// Replica `"" + value`:
/// - string → ela mesma
/// - número → dígitos
/// - bool → "true"/"false"
/// - null → "null"
/// - array → join(",")
/// - objeto → "[object Object]"
fn to_js_string(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .map(to_js_string)
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

// ── integrator_template ────────────────────────────────────────────────────

/// Porta de `integratorTemplate` em `agents.cjs:286`.
pub fn integrator_template(config: &Value) -> String {
    let name = s_or(config, "name", "");
    let service = s_or(config, "service", &name);
    let description = s_or(
        config,
        "description",
        "Integrator agent. Conhece a API externa a fundo. **Informa, não age.** Quem age é o motor correspondente.",
    );

    let env_list = {
        let arr = arr_or_empty(config, "env_vars");
        if arr.is_empty() {
            "- _(nenhuma)_".to_string()
        } else {
            arr.iter()
                .map(|v| format!("- `{}`", to_js_string(v)))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    let ep_list = {
        let arr = arr_or_empty(config, "endpoints");
        if arr.is_empty() {
            "- _A preencher a partir da documentação oficial._".to_string()
        } else {
            arr.iter()
                .map(|e| {
                    let method = e
                        .get("method")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("GET");
                    let path = e.get("path").and_then(Value::as_str).unwrap_or("");
                    let purpose = e.get("purpose").and_then(Value::as_str);
                    // JS: `purpose ? ' — ' + e.purpose : ''` — empty string é falsy, não adiciona
                    let suffix = match purpose {
                        Some(p) if !p.is_empty() => format!(" — {p}"),
                        _ => String::new(),
                    };
                    format!("- `{method} {path}`{suffix}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    let wh_list = {
        let arr = arr_or_empty(config, "webhooks");
        if arr.is_empty() {
            "- _(sem webhooks documentados ainda)_".to_string()
        } else {
            arr.iter()
                .map(|w| {
                    let event = w.get("event").and_then(Value::as_str).unwrap_or("");
                    let purpose = w.get("purpose").and_then(Value::as_str);
                    let suffix = match purpose {
                        Some(p) if !p.is_empty() => format!(" — {p}"),
                        _ => String::new(),
                    };
                    format!("- `{event}`{suffix}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    let got_list = {
        let arr = arr_or_empty(config, "gotchas");
        if arr.is_empty() {
            "- _A preencher conforme integração avança._".to_string()
        } else {
            arr.iter()
                .map(|g| format!("- {}", to_js_string(g)))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    let docs_url_line = match config.get("docs_url").and_then(Value::as_str) {
        Some(u) if !u.is_empty() => format!("- {u}"),
        _ => "- _Adicionar URL da documentação oficial_".to_string(),
    };

    let base_url_line = match config.get("base_url").and_then(Value::as_str) {
        Some(u) if !u.is_empty() => format!("`{u}`"),
        _ => "_A definir_".to_string(),
    };

    let auth_line = s_or(
        config,
        "auth",
        "_A preencher: tipo (API key / OAuth / JWT), header esperado, formato do token._",
    );

    format!(
        "# {name} — Integrator ({service})

## Papel
{description}

## Documentação oficial
{docs_url_line}

## Base URL
{base_url_line}

## Autenticação
{auth_line}

## Env vars esperadas
{env_list}

> Nunca guarde valores no código ou nos contextos. Só nomes.

## Endpoints principais
{ep_list}

## Webhooks
{wh_list}

## Gotchas conhecidas
{got_list}

## Quando o Manager deve consultar este agente
- Antes de implementar qualquer chamada pra {service}
- Quando aparecer erro de integração
- Ao revisar PRs que mexem com {service}
"
    )
}

// ── firefighter_template ───────────────────────────────────────────────────

/// Porta de `firefighterTemplate` em `agents.cjs:327`.
pub fn firefighter_template(config: &Value) -> String {
    let name = s_or(config, "name", "");
    let service = s_or(config, "service", "monitoramento");
    let description = s_or(
        config,
        "description",
        "Firefighter agent. **Reativo** — escuta logs e dispara triage quando detecta padrão de erro. Diagnostica, propõe correção, **escala ao Manager**. Nunca corrige sozinho.",
    );

    let datadog_query = s_or(config, "datadog_query", "service:? status:error");
    let threshold = n_or(config, "threshold", 5);
    let window_minutes = n_or(config, "window_minutes", 5);
    let cooldown_seconds = n_or(config, "cooldown_seconds", 900);
    let max_triages_per_hour = n_or(config, "max_triages_per_hour", 4);

    // env_vars: `config.env_vars || ['DD_API_KEY','DD_APP_KEY','DD_SITE']`
    let env_list = {
        let default = [
            Value::String("DD_API_KEY".to_string()),
            Value::String("DD_APP_KEY".to_string()),
            Value::String("DD_SITE".to_string()),
        ];
        let arr: &[Value] = config
            .get("env_vars")
            .and_then(Value::as_array)
            .map(|a| a.as_slice())
            .unwrap_or(&default);
        arr.iter()
            .map(|v| format!("- `{}`", to_js_string(v)))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let runbook_list = {
        let arr = arr_or_empty(config, "runbook");
        if arr.is_empty() {
            "- _A preencher: primeiro olhar, comandos úteis, hipóteses típicas._".to_string()
        } else {
            arr.iter()
                .map(|r| format!("- {}", to_js_string(r)))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    let escalation_list = {
        let arr = arr_or_empty(config, "escalation");
        if arr.is_empty() {
            "- Sempre: reportar triage ao Manager\n- Se incidente crítico: notificar o Engenheiro humano".to_string()
        } else {
            arr.iter()
                .map(|e| format!("- {}", to_js_string(e)))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    format!(
        "# {name} — Firefighter ({service})

## Papel
{description}

## O que observa
- **Fonte:** Datadog Logs
- **Query:** `{datadog_query}`
- **Threshold:** {threshold} eventos
- **Janela:** últimos {window_minutes} minutos

## Controles de custo
- **Cooldown:** {cooldown_seconds}s entre triages consecutivas (evita spam no mesmo incidente)
- **Teto:** {max_triages_per_hour} triages/hora (proteção contra storm)

## Env vars esperadas (só NOMES, sem valores)
{env_list}

## Runbook — primeiro olhar
{runbook_list}

## Escalação
{escalation_list}

## Regras obrigatórias
1. **Nunca** corrige código sem aprovação do Manager + Engenheiro.
2. **Sempre** registra a triage como observação (`firefighter_triage`).
3. **Não** re-dispara durante cooldown (dedup por incidente).
4. **Dry-first**: diagnóstico primeiro, ação depois — e só via motor aprovado.

## Quando o Manager consulta
- Após receber triage: validar diagnóstico, decidir se aciona motor de correção ou só monitora.
- Para ajustar threshold/query se estiver ruidoso.
"
    )
}

// ── motor_template ─────────────────────────────────────────────────────────

/// Porta de `motorTemplate` em `agents.cjs:368`.
pub fn motor_template(config: &Value) -> String {
    let name = s_or(config, "name", "");
    let action = s_or(config, "action", &name);
    let description = s_or(
        config,
        "description",
        "Motor agent. **Age no mundo real** via MCPs ou chamadas de rede. Toda ação relevante passa pelo Manager.",
    );

    let mcp_list = {
        let arr = arr_or_empty(config, "mcps");
        if arr.is_empty() {
            "- _(nenhum MCP configurado — este motor precisa de um MCP server em settings.json para agir)_".to_string()
        } else {
            arr.iter()
                .map(|m| format!("- `{}`", to_js_string(m)))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    let env_list = {
        let arr = arr_or_empty(config, "env_vars");
        if arr.is_empty() {
            "- _(nenhuma)_".to_string()
        } else {
            arr.iter()
                .map(|v| format!("- `{}`", to_js_string(v)))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    let tools_block = {
        let arr = arr_or_empty(config, "tools");
        if arr.is_empty() {
            "- _A definir_".to_string()
        } else {
            arr.iter()
                .map(|t| format!("- `{}`", to_js_string(t)))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    let allowed = {
        let arr = arr_or_empty(config, "allowed_actions");
        if arr.is_empty() {
            "- _A definir_".to_string()
        } else {
            arr.iter()
                .map(|a| format!("- {}", to_js_string(a)))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    let forbidden = {
        let arr = arr_or_empty(config, "forbidden_actions");
        if arr.is_empty() {
            "- Enviar conteúdo não verificado\n- Operar fora do horário comercial (quando aplicável)\n- Ações em massa sem aprovação do Manager".to_string()
        } else {
            arr.iter()
                .map(|a| format!("- {}", to_js_string(a)))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    // JS: `requires_manager_approval === false ? '_(exceto operações marcadas como seguras)_' : 'relevante'`
    // Strict equality a `false` — só `Value::Bool(false)` casa.
    let approval_qualifier = match config.get("requires_manager_approval") {
        Some(Value::Bool(false)) => "_(exceto operações marcadas como seguras)_",
        _ => "relevante",
    };

    format!(
        "# {name} — Motor ({action})

## Papel
{description}

## Guardrails obrigatórios
1. **Confirmar com o Manager** antes de executar qualquer ação {approval_qualifier}
2. **Nunca** armazenar ou logar conteúdo sensível (senhas, tokens, dados pessoais em texto plano)
3. **Idempotência** — toda ação precisa ter identificador único pra evitar duplicidade
4. **Dry-run primeiro** quando possível — simule antes de executar de verdade

## MCPs necessários
{mcp_list}

## Ferramentas permitidas
{tools_block}

## Env vars
{env_list}

## Ações permitidas
{allowed}

## Ações proibidas
{forbidden}

## Quando o Manager pode invocar
- Apenas após confirmar com o Engenheiro humano em ações de escopo amplo
- Individualmente pra ações rotineiras já aprovadas
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn integrator_minimal() {
        let s = integrator_template(&json!({"name": "gh"}));
        assert!(s.contains("# gh — Integrator (gh)"));
        assert!(s.contains("_A definir_"));
        assert!(s.contains("- _A preencher a partir da documentação oficial._"));
        assert!(s.contains("- _(sem webhooks documentados ainda)_"));
    }

    #[test]
    fn integrator_full() {
        let s = integrator_template(&json!({
            "name": "gh",
            "service": "GitHub",
            "base_url": "https://api.github.com",
            "docs_url": "https://docs.github.com",
            "auth": "Bearer token",
            "env_vars": ["GH_TOKEN", "GH_USER"],
            "endpoints": [
                {"method": "POST", "path": "/repos", "purpose": "Create repo"},
                {"path": "/user"}
            ],
            "webhooks": [{"event": "push", "purpose": "on commit"}],
            "gotchas": ["Rate limit 5k/h"]
        }));
        assert!(s.contains("Integrator (GitHub)"));
        assert!(s.contains("`https://api.github.com`"));
        assert!(s.contains("- `GH_TOKEN`"));
        assert!(s.contains("`POST /repos` — Create repo"));
        assert!(s.contains("`GET /user`")); // method default
        assert!(s.contains("`push` — on commit"));
        assert!(s.contains("- Rate limit 5k/h"));
    }

    #[test]
    fn firefighter_minimal() {
        let s = firefighter_template(&json!({"name": "alerta"}));
        assert!(s.contains("Firefighter (monitoramento)"));
        assert!(s.contains("- `DD_API_KEY`"));
        assert!(s.contains("- `DD_APP_KEY`"));
        assert!(s.contains("- `DD_SITE`"));
        assert!(s.contains("**Threshold:** 5"));
        assert!(s.contains("últimos 5 minutos"));
    }

    #[test]
    fn firefighter_full() {
        let s = firefighter_template(&json!({
            "name": "alerta",
            "service": "api",
            "datadog_query": "service:api status:error",
            "threshold": 10,
            "window_minutes": 15,
            "cooldown_seconds": 1800,
            "max_triages_per_hour": 2,
            "env_vars": ["DD_API_KEY"],
            "runbook": ["Olhar dashboard X", "Checar queue"],
            "escalation": ["Chamar plantão"]
        }));
        assert!(s.contains("Firefighter (api)"));
        assert!(s.contains("service:api status:error"));
        assert!(s.contains("**Threshold:** 10"));
        assert!(s.contains("**Cooldown:** 1800s"));
        assert!(s.contains("- Olhar dashboard X"));
        assert!(s.contains("- Chamar plantão"));
    }

    #[test]
    fn motor_minimal() {
        let s = motor_template(&json!({"name": "ship"}));
        assert!(s.contains("# ship — Motor (ship)"));
        assert!(s.contains("ação relevante"));
        assert!(s.contains("(nenhum MCP configurado"));
        assert!(s.contains("_A definir_"));
    }

    #[test]
    fn motor_full_with_safe_approval() {
        let s = motor_template(&json!({
            "name": "auto",
            "action": "deploy",
            "mcps": ["gh", "aws"],
            "tools": ["Read"],
            "env_vars": ["AWS_KEY"],
            "allowed_actions": ["deploy staging"],
            "forbidden_actions": ["deploy prod sem revisão"],
            "requires_manager_approval": false
        }));
        assert!(s.contains("Motor (deploy)"));
        assert!(s.contains("_(exceto operações marcadas como seguras)_"));
        assert!(s.contains("- `gh`"));
        assert!(s.contains("- `aws`"));
        assert!(s.contains("- deploy staging"));
        assert!(s.contains("- deploy prod sem revisão"));
    }
}
