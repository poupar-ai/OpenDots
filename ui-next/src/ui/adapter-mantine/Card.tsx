import type { ReactNode, CSSProperties } from 'react';
import { Card as MCard } from '@mantine/core';
import { radiusToMantine, spacingToMantine } from './tokens';
import type { Radius, Spacing } from '../types';

export interface CardProps {
  children: ReactNode;
  padding?: Spacing | number;
  radius?: Radius;
  withBorder?: boolean;
  style?: CSSProperties;
}

export function Card({
  padding = 'md',
  radius = 'md',
  withBorder,
  style,
  children,
}: CardProps) {
  return (
    <MCard
      padding={typeof padding === 'number' ? padding : spacingToMantine[padding]}
      radius={radiusToMantine[radius]}
      withBorder={withBorder}
      style={style}
    >
      {children}
    </MCard>
  );
}
