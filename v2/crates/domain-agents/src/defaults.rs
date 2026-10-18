//! Defaults + `initDefaults` — porta de `agents.cjs:34-282, 446-459`.
//!
//! Este módulo contém:
//!
//! - [`default_manifest`] — constrói o `DEFAULT_MANIFEST` (estrutura
//!   com Manager, Oracle, Architect, Reviewer, Tester, Designer e
//!   Pipeline Writer).
//! - [`default_mini_contexts`] — iterador ordenado sobre os mini-
//!   contextos default (chave = nome do agente, valor = markdown).
//! - [`init_defaults`] — cria `<mem_dir>/agents/` e popula manifest +
//!   mini-contextos. Modo `overwrite=true` sobrescreve tudo.
//!
//! ## Paridade
//!
//! - **Chaves JSON**: `serde_json::json!` com feature `preserve_order`
//!   mantém ordem de escrita (IndexMap), casando byte-a-byte com
//!   `JSON.stringify(DEFAULT_MANIFEST, null, 2)` após [`save_manifest`].
//! - **Iteração dos mini-contextos**: `Object.entries` em JS preserva
//!   ordem de inserção para chaves string; usamos um `const` slice
//!   ordenado.
//! - **`initDefaults`**: `fs.writeFileSync` em JS grava bytes literais
//!   (sem trailing newline adicional). Usamos [`fs::write`] com o
//!   mesmo conteúdo bruto; NÃO passamos pelo normalizador de
//!   `save_mini_context` (que adicionaria `\n` final). Isso replica o
//!   comportamento do `.cjs`.

use crate::{agents_dir, load_manifest, manifest_path, save_manifest};
use serde_json::{json, Value};
use std::fs;
use std::io;
use std::path::Path;

/// Constrói o `DEFAULT_MANIFEST`. Mesmo shape, mesma ordem de chaves
/// que `agents.cjs:34-162`.
///
/// Retornado como `Value` para que [`save_manifest`] serialize com o
/// pretty-printer compartilhado (indent 2 + trailing `\n`).
pub fn default_manifest() -> Value {
    json!({
        "version": 1,
        "manager": "manager",
        "oracle": "oracle",
        "rule": "Toda decisão relevante deve ser comunicada ao Manager antes de finalizar. O Engenheiro pode falar direto com qualquer agente, mas o Manager é notificado. Motores (ação no mundo real) sempre pedem confirmação ao Manager antes de executar.",
        "agents": {
            "manager": {
                "type": "core",
                "emoji": "🎯",
                "model": "opus-5.5",
                "role": "Orquestrador central. Recebe ordens do Engenheiro, distribui tarefas, consolida resultados, comunica decisões.",
                "context_file": "agents/manager.md",
                "tools": ["Task", "Read", "Write", "Edit", "Bash", "Grep", "Glob"],
                "mcps": [],
                "can_talk_to": ["oracle", "architect", "reviewer", "tester", "designer", "pipeline_writer"],
                "autonomy": {
                    "level": "high",
                    "call_human_when": [
                        "decisão envolve gastar dinheiro da empresa",
                        "mudança vai pra produção",
                        "decisão muda roadmap ou estratégia",
                        "dois especialistas discordam e o manager não tem dados para decidir"
                    ]
                }
            },
            "oracle": {
                "type": "core",
                "emoji": "🔮",
                "model": "opus-5.5",
                "role": "Conhece todo o projeto — fonte de verdade sobre código, arquitetura e histórico. Responde, não dá ordens.",
                "context_file": "agents/oracle.md",
                "tools": ["Read", "Grep", "Glob"],
                "mcps": [],
                "can_talk_to": ["manager"],
                "autonomy": {
                    "level": "high",
                    "call_human_when": [
                        "pergunta exige acesso a sistema externo que o oracle não tem"
                    ]
                }
            },
            "architect": {
                "type": "specialist",
                "emoji": "🏛",
                "model": "opus-5.5",
                "role": "Decisões de arquitetura, trade-offs de design, ADRs.",
                "context_file": "agents/architect.md",
                "tools": ["Read", "Grep", "Glob", "Write"],
                "mcps": [],
                "can_talk_to": ["manager", "oracle"],
                "autonomy": {
                    "level": "medium",
                    "call_human_when": [
                        "decisão arquitetural é irreversível",
                        "trade-off envolve custo recorrente",
                        "mudança de stack ou de linguagem"
                    ]
                }
            },
            "reviewer": {
                "type": "specialist",
                "emoji": "🔍",
                "model": "opus-5.5",
                "role": "Revisa PRs, aponta bugs, sugere melhorias. Segurança e clareza.",
                "context_file": "agents/reviewer.md",
                "tools": ["Read", "Grep", "Glob"],
                "mcps": [],
                "can_talk_to": ["manager", "oracle"],
                "autonomy": {
                    "level": "high",
                    "call_human_when": [
                        "falha crítica de segurança que precisa decisão de bloqueio de merge",
                        "PR foge do escopo declarado de forma que afeta outras áreas"
                    ]
                }
            },
            "tester": {
                "type": "specialist",
                "emoji": "🧪",
                "model": "opus-5.5",
                "role": "Escreve testes, roda suites, reporta regressões. Unit + integration.",
                "context_file": "agents/tester.md",
                "tools": ["Read", "Write", "Edit", "Bash", "Grep"],
                "mcps": [],
                "can_talk_to": ["manager", "oracle"],
                "autonomy": {
                    "level": "high",
                    "call_human_when": [
                        "teste revela bug em código já em produção"
                    ]
                }
            },
            "designer": {
                "type": "specialist",
                "emoji": "🎨",
                "model": "opus-5.5",
                "role": "UI/UX, layout, acessibilidade, consistência visual.",
                "context_file": "agents/designer.md",
                "tools": ["Read", "Write", "Edit", "Grep", "Glob"],
                "mcps": [],
                "can_talk_to": ["manager"],
                "autonomy": {
                    "level": "medium",
                    "call_human_when": [
                        "mudança visual afeta identidade de marca",
                        "decisão de UX que muda fluxo principal do produto"
                    ]
                }
            },
            "pipeline_writer": {
                "type": "core",
                "emoji": "✨",
                "model": "opus-5.5",
                "role": "Transforma descrições humanas em pipelines YAML reproduzíveis. Conversa com os agentes envolvidos pra entender o fluxo, depois grava pipeline.yaml pronto pra rodar.",
                "context_file": "agents/pipeline_writer.md",
                "tools": ["Read", "Write", "Edit", "Grep", "Glob", "Bash"],
                "mcps": [],
                "can_talk_to": ["manager", "oracle", "architect", "reviewer", "tester", "designer"],
                "autonomy": {
                    "level": "medium",
                    "call_human_when": [
                        "não está claro qual sequência de agentes o humano quer",
                        "mais de um caminho razoável existe e escolher errado cria retrabalho",
                        "pipeline precisaria de agente que não existe no time"
                    ]
                }
            }
        }
    })
}

// ─── Mini-contextos default ─────────────────────────────────────────────────

const MANAGER_MD: &str = "# Manager — Orquestrador

## Responsabilidade
Você é o Manager deste projeto. Recebe ordens do Engenheiro humano, distribui tarefas para os agentes especializados, consolida resultados, e comunica decisões de volta ao Engenheiro.

## Princípios
- **Uma decisão, um dono.** Se duas áreas entram em conflito, o Manager resolve (ou escala ao Engenheiro).
- **Transparência.** Todas as trocas relevantes entre agentes passam por você.
- **Economia de contexto.** Só chame um agente quando a especialidade dele for necessária. Não crie ruído.

## Fluxo típico
1. Receber pedido do Engenheiro
2. Decompor em sub-tarefas
3. Delegar via Task tool (escolhendo o agente certo)
4. Consolidar respostas
5. Reportar ao Engenheiro com 1 decisão clara
";

const ORACLE_MD: &str = "# Oracle — Fonte de verdade

## Responsabilidade
Você conhece o projeto inteiro — código, arquitetura, decisões passadas, histórico. Responde perguntas, **nunca dá ordens**.

## Princípios
- **Cite fontes.** Sempre referencie arquivo:linha quando falar sobre código.
- **Não suponha.** Se não sabe, diga. Não invente arquitetura.
- **Direto.** Respostas curtas, sem floreio.

## Quando é chamado
- \"Onde está implementada a função X?\"
- \"Como o auth funciona aqui?\"
- \"Esse padrão já existe no projeto?\"
- Consultas antes de decisões do Architect/Reviewer
";

const ARCHITECT_MD: &str = "# Architect — Decisões de design

## Responsabilidade
Decisões de arquitetura, trade-offs, escolha de padrões. Produz ADRs quando a decisão é relevante.

## Princípios
- **Simplicidade primeiro.** Complexidade só se justificada.
- **Cita alternativas.** Sempre mostra 2-3 opções antes de recomendar.
- **Horizonte claro.** Diz qual decisão é reversível, qual não é.
";

const REVIEWER_MD: &str = "# Reviewer — Revisão de código

## Responsabilidade
Revisa PRs, aponta bugs, sugere melhorias. Foco em segurança, clareza e manutenibilidade.

## Princípios
- **Específico.** Aponta arquivo:linha e explica o porquê.
- **Proporcional.** Distingue nit de bloqueante.
- **Construtivo.** Sugere como arrumar, não só o que está errado.
";

const TESTER_MD: &str = "# Tester — Qualidade

## Responsabilidade
Escreve testes (unit + integration), roda suites, reporta regressões. Garante que mudanças não quebram o existente.

## Princípios
- **Cobre o caminho feliz e 1-2 edges.** Não testa tudo.
- **Testes rápidos.** Se um teste é lento demais, sinaliza.
- **Fail informativo.** Mensagens de erro que explicam o que quebrou.
";

const DESIGNER_MD: &str = "# Designer — UI/UX

## Responsabilidade
Decisões visuais, layout, acessibilidade, consistência. Conversa com o Manager antes de mudanças grandes.

## Princípios
- **Acessível por padrão.** Contraste, aria, foco.
- **Consistente com a alma.** Respeita a voz e estética do SOUL.md.
- **Mobile-first quando relevante.**
";

const PIPELINE_WRITER_MD: &str = "# Pipeline Writer — Gravador de Fluxos

## Responsabilidade
Transformar descrição humana (\"quero uma pipeline pra X\") em **pipeline YAML válida** que roda no runner.
Conversa com os agentes envolvidos pra entender o fluxo real, decidir a ordem dos passos e onde entram checkpoints humanos.

## Princípios
- **Fluxo real, não ideal.** Só steps que o humano pediu.
- **Checkpoints onde dói errar.** Antes de gastar tempo caro, antes de efeito externo (publicar, cobrar, enviar).
- **IDs são slugs.** `[a-z][a-z0-9-]+[a-z0-9]`. Nunca espaço, acento, maiúscula.
- **Cada step tem dono.** `agent: <nome>` pra steps executáveis; `type: checkpoint` + `owner` pra gates humanos.
- **Pergunte antes de inventar.** Se não ficou claro quem faz X, consulte o humano ou o agente em dúvida.

## Onde grava
Sempre no projeto corrente: `.poupar/pipelines/<id>/pipeline.yaml`.
Template global é trabalho do humano (não tem acesso a `plugin/pipelines/`).

## Schema mínimo
```yaml
id: <slug>
label: <string>
description: |
  multi-linha opcional
tags: [a, b]
steps:
  - id: <slug>
    agent: <nome-agente>
    label: <string>
    description: <string>
    best_practices: [x, y]   # opcional
  - id: <slug>
    type: checkpoint
    label: <string>
    owner: any_human
```

## Fluxo típico
1. Entenda o pedido. Pergunte se faltar dado.
2. Consulte agentes envolvidos via handoff se precisar (ex: \"designer, você faz só copy ou copy+arte?\"). Só se a resposta muda o YAML.
3. Esboce 3-10 steps.
4. Coloque checkpoints antes de ação cara/irreversível. Nunca como 1º step.
5. Valide: IDs slug válidos e únicos, agentes existem, checkpoints têm owner, fluxo ponta a ponta faz sentido.
6. Write no arquivo e confirma ao humano: id, path, 1 linha por step.
";

/// Pares `(nome, conteúdo)` dos mini-contextos default, em ordem
/// idêntica à inserção do objeto literal em JS (garante iteração
/// determinística).
pub fn default_mini_contexts() -> &'static [(&'static str, &'static str)] {
    &[
        ("manager", MANAGER_MD),
        ("oracle", ORACLE_MD),
        ("architect", ARCHITECT_MD),
        ("reviewer", REVIEWER_MD),
        ("tester", TESTER_MD),
        ("designer", DESIGNER_MD),
        ("pipeline_writer", PIPELINE_WRITER_MD),
    ]
}

// ─── init_defaults ─────────────────────────────────────────────────────────

/// Cria `<mem_dir>/agents/` e popula manifest + mini-contextos.
///
/// - Se `overwrite` ou `agents.json` não existe, sobrescreve com
///   [`default_manifest`].
/// - Para cada `(nome, md)` em [`default_mini_contexts`], grava
///   `<mem_dir>/agents/<nome>.md` apenas se `overwrite` ou o arquivo
///   ainda não existe.
///
/// Retorna `Ok(Some(manifest))` após a operação (equivalente ao
/// `return loadManifest(memDir)` do `.cjs`). Se a leitura de volta
/// falhar, retorna `Ok(None)` (mesmo comportamento do `.cjs` que
/// propaga o `null` de `loadManifest`).
///
/// ## Divergência de I/O error
///
/// O `.cjs` propaga exceções de `fs.mkdirSync`/`fs.writeFileSync` para
/// o caller. A versão Rust propaga [`io::Error`] — paridade direta.
pub fn init_defaults(mem_dir: &Path, overwrite: bool) -> io::Result<Option<Value>> {
    let dir = agents_dir(mem_dir);
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }

    let manifest = manifest_path(mem_dir);
    if overwrite || !manifest.exists() {
        // save_manifest propaga io::Error; paridade com fs.writeFileSync.
        save_manifest(mem_dir, &default_manifest())?;
    }

    for (name, content) in default_mini_contexts() {
        let p = dir.join(format!("{name}.md"));
        if overwrite || !p.exists() {
            // Paridade com .cjs: fs.writeFileSync(p, content) — grava
            // bytes literais, SEM adicionar `\n` final. Não usamos
            // save_mini_context porque ele normaliza trailing newline.
            fs::write(&p, content)?;
        }
    }

    Ok(load_manifest(mem_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_manifest_has_core_agents() {
        let m = default_manifest();
        let agents = m.get("agents").unwrap().as_object().unwrap();
        assert!(agents.contains_key("manager"));
        assert!(agents.contains_key("oracle"));
        assert!(agents.contains_key("pipeline_writer"));
    }

    #[test]
    fn default_manifest_manager_is_core() {
        let m = default_manifest();
        let t = m["agents"]["manager"]["type"].as_str().unwrap();
        assert_eq!(t, "core");
    }

    #[test]
    fn default_manifest_preserves_agent_order() {
        // IndexMap preserva ordem de inserção — testa que
        // `json!` respeita ordem de escrita.
        let m = default_manifest();
        let agents = m.get("agents").unwrap().as_object().unwrap();
        let names: Vec<&str> = agents.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            vec![
                "manager",
                "oracle",
                "architect",
                "reviewer",
                "tester",
                "designer",
                "pipeline_writer",
            ]
        );
    }

    #[test]
    fn default_mini_contexts_has_seven_entries() {
        assert_eq!(default_mini_contexts().len(), 7);
    }

    #[test]
    fn default_mini_contexts_order_matches_manifest() {
        let manifest = default_manifest();
        let agents = manifest.get("agents").unwrap().as_object().unwrap();
        let manifest_names: Vec<&str> = agents.keys().map(String::as_str).collect();
        let ctx_names: Vec<&str> = default_mini_contexts().iter().map(|(n, _)| *n).collect();
        assert_eq!(manifest_names, ctx_names);
    }

    #[test]
    fn default_mini_contexts_all_start_with_heading() {
        for (_name, content) in default_mini_contexts() {
            assert!(
                content.starts_with("# "),
                "mini-context must start with '# ' heading"
            );
        }
    }

    // ─── init_defaults ─────────────────────────────────────────────────────

    #[test]
    fn init_creates_agents_dir() {
        let tmp = TempDir::new().unwrap();
        init_defaults(tmp.path(), false).unwrap();
        assert!(tmp.path().join("agents").is_dir());
    }

    #[test]
    fn init_writes_manifest() {
        let tmp = TempDir::new().unwrap();
        init_defaults(tmp.path(), false).unwrap();
        let path = tmp.path().join("agents.json");
        assert!(path.is_file());
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("\"manager\""));
        assert!(body.ends_with('\n'));
    }

    #[test]
    fn init_writes_every_mini_context() {
        let tmp = TempDir::new().unwrap();
        init_defaults(tmp.path(), false).unwrap();
        for (name, _) in default_mini_contexts() {
            let p = tmp.path().join("agents").join(format!("{name}.md"));
            assert!(p.is_file(), "{name}.md missing");
        }
    }

    #[test]
    fn init_returns_loaded_manifest() {
        let tmp = TempDir::new().unwrap();
        let got = init_defaults(tmp.path(), false).unwrap().unwrap();
        assert_eq!(got["manager"].as_str(), Some("manager"));
        assert_eq!(got["oracle"].as_str(), Some("oracle"));
    }

    #[test]
    fn init_does_not_overwrite_existing_manifest() {
        let tmp = TempDir::new().unwrap();
        let mf = tmp.path().join("agents.json");
        fs::create_dir_all(tmp.path().join("agents")).unwrap();
        fs::write(&mf, r#"{"custom":true}"#).unwrap();

        init_defaults(tmp.path(), false).unwrap();

        let body = fs::read_to_string(&mf).unwrap();
        assert_eq!(body, r#"{"custom":true}"#);
    }

    #[test]
    fn init_does_not_overwrite_existing_mini_context() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("agents")).unwrap();
        let p = tmp.path().join("agents").join("manager.md");
        fs::write(&p, "# Customizado").unwrap();

        init_defaults(tmp.path(), false).unwrap();

        assert_eq!(fs::read_to_string(&p).unwrap(), "# Customizado");
    }

    #[test]
    fn init_overwrite_replaces_manifest() {
        let tmp = TempDir::new().unwrap();
        let mf = tmp.path().join("agents.json");
        fs::create_dir_all(tmp.path().join("agents")).unwrap();
        fs::write(&mf, r#"{"custom":true}"#).unwrap();

        init_defaults(tmp.path(), true).unwrap();

        let body = fs::read_to_string(&mf).unwrap();
        assert!(body.contains("\"manager\": \"manager\""));
    }

    #[test]
    fn init_overwrite_replaces_mini_contexts() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("agents")).unwrap();
        let p = tmp.path().join("agents").join("manager.md");
        fs::write(&p, "# Custom").unwrap();

        init_defaults(tmp.path(), true).unwrap();

        let body = fs::read_to_string(&p).unwrap();
        assert!(body.starts_with("# Manager"));
    }

    #[test]
    fn init_mini_context_bytes_match_source() {
        // Paridade: .cjs grava via fs.writeFileSync(p, content) SEM
        // trailing newline adicional. Rust deve fazer idem.
        let tmp = TempDir::new().unwrap();
        init_defaults(tmp.path(), false).unwrap();
        for (name, expected) in default_mini_contexts() {
            let p = tmp.path().join("agents").join(format!("{name}.md"));
            let got = fs::read_to_string(&p).unwrap();
            assert_eq!(&got, expected, "mismatch em {name}.md");
        }
    }
}
