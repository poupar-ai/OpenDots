import type { ReactNode } from 'react';
import { Modal as MModal } from '@mantine/core';

export interface ModalProps {
  opened: boolean;
  onClose: () => void;
  title?: ReactNode;
  size?: 'sm' | 'md' | 'lg' | 'xl';
  /** Ocupa a tela inteira — útil pra responsividade no mobile. */
  fullScreen?: boolean;
  /** Mostra o botão de fechar (X). Default true. */
  withCloseButton?: boolean;
  children: ReactNode;
}

export function Modal({ opened, onClose, title, size = 'md', fullScreen, withCloseButton, children }: ModalProps) {
  return (
    <MModal
      opened={opened}
      onClose={onClose}
      title={title}
      size={size}
      fullScreen={fullScreen}
      withCloseButton={withCloseButton}
    >
      {children}
    </MModal>
  );
}
