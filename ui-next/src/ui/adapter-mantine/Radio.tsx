import type { ReactNode } from 'react';
import { Radio as MRadio } from '@mantine/core';

export interface RadioGroupProps {
  value: string;
  onChange: (v: string) => void;
  name: string;
  children: ReactNode;
}

export function RadioGroup({ value, onChange, name, children }: RadioGroupProps) {
  return (
    <MRadio.Group value={value} onChange={onChange} name={name}>
      {children}
    </MRadio.Group>
  );
}

export interface RadioProps {
  value: string;
  label?: string;
  disabled?: boolean;
}

export function Radio({ value, label, disabled }: RadioProps) {
  return <MRadio value={value} label={label} disabled={disabled} />;
}
