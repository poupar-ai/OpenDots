import type { ReactNode, CSSProperties } from 'react';
import { Box as MBox } from '@mantine/core';
import type { Breakpoint } from '../types';

export interface BoxProps {
  children?: ReactNode;
  style?: CSSProperties;
  className?: string;
  // Margins/paddings simples (pass-through pro Mantine, mas usando
  // nossa Spacing scale via classes ou style).
  mt?: number | string;
  mb?: number | string;
  ml?: number | string;
  mr?: number | string;
  pt?: number | string;
  pb?: number | string;
  pl?: number | string;
  pr?: number | string;
  flex?: number | string;
  /** Esconde abaixo deste breakpoint (mostra a partir dele). Responsivo. */
  visibleFrom?: Breakpoint;
  /** Esconde a partir deste breakpoint (mostra abaixo dele). Responsivo. */
  hiddenFrom?: Breakpoint;
}

export function Box(props: BoxProps) {
  return <MBox {...props} />;
}
