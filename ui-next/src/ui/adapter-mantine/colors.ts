import type { Color } from '../types';
import { colorToMantine } from './tokens';

/**
 * Retorna a CSS variable correspondente à cor semântica, num shade
 * dado (0–9). Usado quando o panel precisa colorir um border/background
 * inline (ex.: card destacado) sem amarrar o componente ao nome do
 * vendor. Pra trocar de lib, o adapter novo reimplementa essa fn.
 */
export function colorVar(color: Color, shade: 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 = 6): string {
  return `var(--mantine-color-${colorToMantine[color]}-${shade})`;
}
