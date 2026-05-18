import type { ReactNode } from 'react';
import { Tooltip as MTooltip } from '@mantine/core';

export interface TooltipProps {
  children: ReactNode;
  label: string;
}

export function Tooltip({ label, children }: TooltipProps) {
  return (
    <MTooltip label={label} withArrow>
      {children}
    </MTooltip>
  );
}
