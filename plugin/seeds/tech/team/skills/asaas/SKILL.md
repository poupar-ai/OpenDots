---
name: asaas
description: Integração com gateway de pagamento Asaas (Pix, cartão, boleto)
scope: payment
version: "1.0.0"
requires_mcp: true
env:
  - ASAAS_API_KEY
---

# Asaas — Skill

Use para criar cobranças, consultar status, configurar webhooks e fazer
repasses via Asaas.

## Quando usar
- Gerar link de pagamento (Pix/cartão/boleto) para um pedido.
- Consultar status de cobrança existente.
- Configurar webhook de notificação de pagamento.
- Fazer repasse (split) entre vendedor e plataforma.

## Quando NÃO usar
- Gateways concorrentes (Mercado Pago, Stripe) — cada um tem skill próprio.
- KYC — use a skill `big-data-corp`.

## Endpoints principais
- POST /v3/payments — cria cobrança
- GET /v3/payments/:id — status
- POST /v3/transfers — repasse
- POST /v3/webhooks — registra webhook

## Idempotency
Sempre envie `externalReference` único por pedido. O Asaas deduplica
automaticamente se vier 2x com mesmo externalReference no mesmo dia.
