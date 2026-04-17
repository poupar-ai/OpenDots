---
name: resend
description: Envio transacional de e-mails via Resend
scope: email
version: "1.0.0"
requires_mcp: true
env:
  - RESEND_API_KEY
---

# Resend — Skill

Use para enviar e-mails transacionais (confirmação de pedido, recuperação
de senha, notificação de pagamento).

## Quando usar
- E-mails 1:1 acionados por evento.
- Templates de domínio (pedido confirmado, pagamento aprovado).

## Quando NÃO usar
- Newsletters em massa — escolha ferramenta própria de marketing.
- Notificações em tempo real dentro do app — use WebSocket/SSE.

## Templates obrigatórios
Todo template deve ter:
- `subject` em 50 caracteres ou menos
- `from` com domínio verificado
- texto-puro além de HTML (Gmail penaliza HTML-only)

## Idempotency
Usar `idempotency_key` = `user_id:event_id`. Resend não deduplica nativo,
então a idempotência é responsabilidade do chamador.
