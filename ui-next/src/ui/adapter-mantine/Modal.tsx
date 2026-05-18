import type { ReactNode } from 'react';
import { Modal as MModal } from '@mantine/core';

export interface ModalProps {
  opened: boolean;
  onClose: () => void;
  title?: ReactNode;
  size?: 'sm' | 'md' | 'lg' | 'xl';
  children: ReactNode;
}

export function Modal({ opened, onClose, title, size = 'md', children }: ModalProps) {
  return (
    <MModal opened={opened} onClose={onClose} title={title} size={size}>
      {children}
    </MModal>
  );
}
