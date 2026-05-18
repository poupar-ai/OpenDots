import type { ReactNode } from 'react';
import { Stack as MStack } from '@mantine/core';
import { spacingToMantine } from './tokens';
import type { Spacing } from '../types';

export interface StackProps {
  children: ReactNode;
  gap?: Spacing | number;
  align?: 'flex-start' | 'center' | 'flex-end' | 'stretch';
}

export function Stack({ gap = 'md', align, children }: StackProps) {
  return (
    <MStack
      gap={typeof gap === 'number' ? gap : spacingToMantine[gap]}
      align={align}
    >
      {children}
    </MStack>
  );
}
