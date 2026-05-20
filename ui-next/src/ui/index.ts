// `@/ui` — porta única de componentes visuais (arquitetura hexagonal).
//
// PANELS NUNCA IMPORTAM DE `@mantine/core` DIRETO. Sempre via `@/ui`.
// Pra trocar a lib UI:
//   1. Criar `src/ui/adapter-<nome>/` espelhando a mesma API pública.
//   2. Trocar a linha de re-export abaixo (`./adapter-mantine` →
//      `./adapter-<nome>`).
//   3. Atualizar/remover `MantineProvider` em `main.tsx`.
//
// Os tipos semânticos (Color, Size, Variant, ...) ficam em `./types`
// — não dependem do vendor. O mapeamento `semântico → vendor` mora em
// `adapter-mantine/tokens.ts` (único lugar com nomes do Mantine).

export * from './adapter-mantine';
export type { Color, Size, Variant, Radius, Spacing, Breakpoint } from './types';
