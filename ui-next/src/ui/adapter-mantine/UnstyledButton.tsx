import type { ReactNode, CSSProperties } from 'react';
import { UnstyledButton as MUnstyledButton } from '@mantine/core';

export interface UnstyledButtonProps {
  children: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  fullWidth?: boolean;
  style?: CSSProperties;
  type?: 'button' | 'submit' | 'reset';
}

export function UnstyledButton({
  children,
  onClick,
  disabled,
  fullWidth,
  style,
  type = 'button',
}: UnstyledButtonProps) {
  const mergedStyle: CSSProperties = fullWidth
    ? { width: '100%', textAlign: 'left', ...style }
    : style ?? {};
  return (
    <MUnstyledButton
      onClick={onClick}
      disabled={disabled}
      type={type}
      style={mergedStyle}
    >
      {children}
    </MUnstyledButton>
  );
}
