# Seller Onboarding — Princípios

> Aplica-se a agentes que guiam o vendedor no primeiro uso da plataforma.

## Fluxo canônico
1. Cadastro básico (e-mail, nome, celular com OTP).
2. KYC — CPF/CNPJ + selfie. Assíncrono, não bloqueia criar loja.
3. Nome da loja + slug único.
4. Primeiro produto (até 3 campos obrigatórios — resto é opcional).
5. Gateway de pagamento — Asaas por padrão.
6. Link compartilhável (bio + WhatsApp).

## Regras de UX
- **Uma decisão por tela** — nunca pedir duas escolhas juntas.
- **Pular é sempre uma opção** — exceto KYC e primeiro produto.
- **Feedback imediato** — cada save confirma visualmente em &lt;500ms.
- **Texto da próxima ação** no botão principal ("Criar minha loja" > "Próximo").

## Sinais de que o onboarding falhou
- Usuário abandona antes de criar loja — revisite passos 3 e 4.
- KYC reprovado sem explicação — mostre motivo e como corrigir.
- Primeiro produto com 0 vendas em 7 dias — dispare flow de ativação.

## O que NÃO fazer
- **Nunca** pedir dados de cartão antes do vendedor ter o primeiro produto.
- **Nunca** forçar download de app pra completar onboarding.
- **Nunca** pedir documento fiscal de PF (só CPF).

## Glossário (projeto Bankin)
- **Seller** — vendedor, o dono da loja.
- **Store** — loja virtual do seller.
- **KYC** — verificação de identidade obrigatória.
- **Gateway** — provedor de pagamento (Asaas no MVP).
