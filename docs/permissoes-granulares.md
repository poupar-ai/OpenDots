# Como evoluir para permissões granulares (RBAC/ABAC)

Hoje a autorização é **binária**: qualquer principal autenticado pode tudo.
A estrutura já está pronta pra granular — este doc é a receita.

## Estado atual (onde mexer)

- **`Principal`** (`api-http/src/auth.rs`): `Human(id) | Agent(name) | Anonymous`,
  resolvido do Bearer JWT e anexado na request (extension).
- **`authorize(principal, action, resource) -> bool`** (`auth.rs`): **ponto
  único** de decisão. Hoje retorna `principal.is_authenticated()`.
- **Enforcement** (`auth.rs::auth_middleware`): nas rotas de escrita
  (POST/PUT/PATCH/DELETE), bootstrap público + resto exige principal autenticado.
  Ligado por padrão; `POUPAR_REQUIRE_HUMAN=0` desliga.
- **Dados**: humanos em `humans.json`, agentes em `agents.json` (já têm `pubkey`).

## Passos

### 1. Modelar papéis/permissões nos dados

Adicionar `role` (e/ou `permissions`) ao registro do humano em `humans.json`
(via `domain_humans::create_human`/`update_human` + `human_summary`):

```jsonc
{ "id": "dani", "role": "admin", "permissions": ["*"] }      // ou
{ "id": "joao", "role": "member" }                            // papel resolve perms
```

Sugestão de papéis: `admin` (tudo), `member` (escreve o que é seu), `viewer`
(só leitura). Agentes podem ganhar um conjunto de capabilities análogo em
`agents.json` se precisar limitar o que cada agente faz.

> Projeto limpo: definir o **1º humano como `admin`** no `create_human` (sem
> migração de legado).

### 2. Padronizar `action` + `resource`

Derivar um par semântico da rota (um mapa rota→permissão pequeno), em vez de
passar `method`/`path` crus. Ex.:

| rota                          | action   | resource         |
|-------------------------------|----------|------------------|
| `PUT /api/humans/:id`         | `write`  | `human:<id>`     |
| `DELETE /api/humans/:id`      | `delete` | `human:<id>`     |
| `POST /api/agents...`         | `manage` | `agent:<name>`   |
| `POST /api/vault/secret/...`  | `manage` | `vault`          |
| `POST /api/cron...`           | `manage` | `cron`           |

### 3. Implementar a política em `authorize()`

```rust
pub fn authorize(p: &Principal, action: &str, resource: &str) -> bool {
    let Some(id) = p.id() else { return false };       // anon nunca
    let role = load_role(p);                            // lê humans.json/agents.json
    match role {
        Role::Admin => true,
        Role::Viewer => action == "read",
        Role::Member => {
            // ownership: edita/deleta só o PRÓPRIO perfil
            if let Some(target) = resource.strip_prefix("human:") {
                return target == id;
            }
            // member não gerencia vault/agents/cron/scope
            !matches!(resource, "vault" | "cron" | "scope") && !resource.starts_with("agent:")
        }
    }
}
```

### 4. Onde plugar (grosso x fino)

- **Coarse (middleware)**: decisão por rota já cobre a maioria — o
  `auth_middleware` chama `authorize(principal, action, resource)` derivado da
  rota.
- **Fine / row-level (handler)**: pra ownership com o id concreto (ex.: "só
  edita o próprio perfil"), chamar `authorize()` **dentro do handler** com o
  `resource` real (o middleware só conhece o padrão da rota, não o `:id`
  resolvido — embora dê pra extrair do path).

### 5. Expor pro frontend (opcional)

Endpoint `GET /api/me` → `{ id, role, permissions }`, pra UI esconder/desabilitar
ações que o usuário não pode fazer. O wrapper de fetch já manda o Bearer.

### 6. Testes

Tabela de casos em `authorize()` (admin/member/viewer × write/delete/manage ×
próprio/alheio). É lógica pura — fácil de cobrir.

## Resumo

A única peça que muda de verdade é o corpo do **`authorize()`** + um campo
`role` nos dados. O `Principal`, o enforcement e o transporte do token (Bearer)
já estão prontos.
