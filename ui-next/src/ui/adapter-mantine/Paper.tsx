import type { ReactNode, CSSProperties } from 'react';
import { Paper as MPaper } from '@mantine/core';
import { radiusToMantine, spacingToMantine } from './tokens';
import type { Radius, Spacing } from '../types';

export interface PaperProps {
  children: ReactNode;
  padding?: Spacing | number;
  /** Padding horizontal/vertical (sobrepõe `padding` no eixo). number = px. */
  px?: Spacing | number;
  py?: Spacing | number;
  radius?: Radius;
  withBorder?: boolean;
  /** Elevação — mapeada direto pra escala de sombras do vendor. */
  shadow?: 'xs' | 'sm' | 'md' | 'lg' | 'xl';
  className?: string;
  style?: CSSProperties;
}

const sp = (v: Spacing | number) => (typeof v === 'number' ? v : spacingToMantine[v]);

export function Paper({
  padding = 'md',
  px,
  py,
  radius = 'md',
  withBorder,
  shadow,
  className,
  style,
  children,
}: PaperProps) {
  const usesAxis = px !== undefined || py !== undefined;
  return (
    <MPaper
      p={usesAxis ? undefined : sp(padding)}
      px={px !== undefined ? sp(px) : undefined}
      py={py !== undefined ? sp(py) : undefined}
      radius={radiusToMantine[radius]}
      withBorder={withBorder}
      shadow={shadow}
      className={className}
      style={style}
    >
      {children}
    </MPaper>
  );
}
