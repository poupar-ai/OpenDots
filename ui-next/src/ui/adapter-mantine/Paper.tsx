import type { ReactNode } from 'react';
import { Paper as MPaper } from '@mantine/core';
import { radiusToMantine, spacingToMantine } from './tokens';
import type { Radius, Spacing } from '../types';

export interface PaperProps {
  children: ReactNode;
  padding?: Spacing | number;
  radius?: Radius;
  withBorder?: boolean;
}

export function Paper({
  padding = 'md',
  radius = 'md',
  withBorder,
  children,
}: PaperProps) {
  return (
    <MPaper
      p={typeof padding === 'number' ? padding : spacingToMantine[padding]}
      radius={radiusToMantine[radius]}
      withBorder={withBorder}
    >
      {children}
    </MPaper>
  );
}
