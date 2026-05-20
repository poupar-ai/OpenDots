import type { ReactNode } from 'react';
import { Avatar as MAvatar } from '@mantine/core';
import { colorToMantine } from './tokens';
import type { Color } from '../types';

export interface AvatarProps {
  /** Nome — o vendor deriva as iniciais automaticamente se não houver children. */
  name?: string;
  src?: string | null;
  color?: Color;
  /** rem (number) ou tamanho semântico do vendor. */
  size?: number | 'xs' | 'sm' | 'md' | 'lg' | 'xl';
  radius?: 'sm' | 'md' | 'lg' | 'xl' | number;
  title?: string;
  /** Conteúdo custom (ex.: iniciais já calculadas, ícone). */
  children?: ReactNode;
}

export function Avatar({ name, src, color = 'primary', size = 'md', radius = 'xl', title, children }: AvatarProps) {
  return (
    <MAvatar
      name={name}
      src={src ?? undefined}
      color={colorToMantine[color]}
      size={size}
      radius={radius}
      title={title}
    >
      {children}
    </MAvatar>
  );
}
