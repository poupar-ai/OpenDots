// Tipos compartilhados da camada UI hexagonal.
//
// API neutra (sem amarrar nomes a Mantine). Adapters mapeiam pra cada
// vendor — ver `adapter-mantine/*` pra implementação atual. Pra trocar
// de lib, basta criar `adapter-<nome>/` espelhando esses tipos.

/** Tamanho semântico — adapter mapeia pra "xs|sm|md" do vendor. */
export type Size = 'xs' | 'sm' | 'md' | 'lg';

/** Cor semântica — adapter mapeia pra paleta do vendor. */
export type Color = 'primary' | 'success' | 'warning' | 'danger' | 'gray' | 'info';

/** Variantes visuais — superfície/contraste. */
export type Variant = 'filled' | 'outline' | 'subtle' | 'default' | 'light';

/** Raio de borda — derivado dos tokens, não direto do vendor. */
export type Radius = 'none' | 'sm' | 'md' | 'lg' | 'full';

/** Espaçamento — escala consistente. */
export type Spacing = 'xs' | 'sm' | 'md' | 'lg' | 'xl';
