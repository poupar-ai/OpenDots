import type { ReactNode } from 'react';
import { Group as MGroup } from '@mantine/core';
import { spacingToMantine } from './tokens';
import type { Spacing } from '../types';

export interface GroupProps {
  children: ReactNode;
  gap?: Spacing | number;
  justify?: 'flex-start' | 'center' | 'flex-end' | 'space-between' | 'space-around';
  align?: 'flex-start' | 'center' | 'flex-end' | 'baseline' | 'stretch';
  wrap?: 'wrap' | 'nowrap';
  /** Filhos dividem a largura igualmente. */
  grow?: boolean;
}

export function Group({ gap = 'sm', justify, align, wrap, grow, children }: GroupProps) {
  return (
    <MGroup
      gap={typeof gap === 'number' ? gap : spacingToMantine[gap]}
      justify={justify}
      align={align}
      wrap={wrap}
      grow={grow}
    >
      {children}
    </MGroup>
  );
}
