import type { ReactNode } from 'react';
import { Alert as MAlert } from '@mantine/core';
import { colorToMantine } from './tokens';
import type { Color } from '../types';

export interface AlertProps {
  children: ReactNode;
  color?: Color;
  title?: string;
  icon?: ReactNode;
  variant?: 'light' | 'filled' | 'outline';
}

export function Alert({
  color = 'info',
  variant = 'light',
  title,
  icon,
  children,
}: AlertProps) {
  return (
    <MAlert color={colorToMantine[color]} variant={variant} title={title} icon={icon}>
      {children}
    </MAlert>
  );
}
