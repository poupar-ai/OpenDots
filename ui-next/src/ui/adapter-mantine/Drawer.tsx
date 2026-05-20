import type { ReactNode } from 'react';
import { Drawer as MDrawer } from '@mantine/core';

export interface DrawerProps {
  opened: boolean;
  onClose: () => void;
  title?: ReactNode;
  /** Lado da tela onde o drawer abre. Default 'right'. */
  position?: 'left' | 'right' | 'top' | 'bottom';
  size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl';
  children: ReactNode;
}

export function Drawer({
  opened,
  onClose,
  title,
  position = 'right',
  size = 'xs',
  children,
}: DrawerProps) {
  return (
    <MDrawer
      opened={opened}
      onClose={onClose}
      title={title}
      position={position}
      size={size}
    >
      {children}
    </MDrawer>
  );
}
