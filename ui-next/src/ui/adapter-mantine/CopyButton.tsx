import type { ReactNode } from 'react';
import { CopyButton as MCopyButton } from '@mantine/core';

export interface CopyButtonProps {
  value: string;
  timeoutMs?: number;
  /** Render fn — recebe `copied` (bool) e `copy` (fn). */
  children: (state: { copied: boolean; copy: () => void }) => ReactNode;
}

export function CopyButton({ value, timeoutMs = 1500, children }: CopyButtonProps) {
  return (
    <MCopyButton value={value} timeout={timeoutMs}>
      {({ copied, copy }) => <>{children({ copied, copy })}</>}
    </MCopyButton>
  );
}
