# Acessibilidade — Princípios (WCAG 2.1 AA)

> Aplica-se a agentes que escrevem UI (React, Next.js, HTML).

## Regras inegociáveis
1. **Todo controle interativo é alcançável pelo teclado** — `Tab`, `Enter`, `Space`, `Esc`.
2. **Foco visível sempre** — nunca remover outline sem colocar substituto.
3. **Contraste mínimo 4.5:1** para texto normal, 3:1 para texto grande e UI.
4. **`alt` em toda imagem** — decorativa usa `alt=""`, nunca omitir.
5. **Label associado a cada input** — `&lt;label htmlFor&gt;` ou `aria-label`.
6. **Hierarquia de headings** sem pular níveis (h1 → h2 → h3).

## ARIA — regras básicas
- **Não use ARIA se um elemento nativo resolve** (`&lt;button&gt;` > `role="button"` em div).
- `aria-live="polite"` para feedback assíncrono.
- `aria-describedby` para mensagens de erro em form.
- `role="dialog"` + `aria-modal="true"` para modais, com foco trapped.

## Checklist rápido em componente novo
- [ ] Funciona só com teclado?
- [ ] Foco fica visível?
- [ ] Leitor de tela anuncia estado (loading, erro, sucesso)?
- [ ] Contraste OK no dark mode também?
- [ ] Textos escaláveis (`rem`/`em`, não `px` fixo em fonte)?
- [ ] Nenhuma cor é o único indicador (erro tem ícone + texto)?

## Anti-padrões
- `div onClick` — não é focável, não lê teclado.
- `placeholder` como substituto de label.
- `disabled` em botão que deveria mostrar motivo.
- Animação sem `prefers-reduced-motion`.
- Tamanho de toque &lt; 44x44px em mobile.
