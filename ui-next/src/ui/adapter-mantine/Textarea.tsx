import type { ChangeEvent } from 'react';
import { Textarea as MTextarea } from '@mantine/core';
import { sizeToMantine } from './tokens';
import type { Size } from '../types';

export interface TextareaProps {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  disabled?: boolean;
  size?: Size;
  label?: string;
  /** Mínimo de linhas visíveis (autosize cresce a partir daí). */
  minRows?: number;
  maxRows?: number;
  error?: string | null;
}

export function Textarea({
  value,
  onChange,
  placeholder,
  disabled,
  size = 'sm',
  label,
  minRows = 2,
  maxRows = 8,
  error,
}: TextareaProps) {
  return (
    <MTextarea
      value={value}
      onChange={(e: ChangeEvent<HTMLTextAreaElement>) => onChange(e.currentTarget.value)}
      placeholder={placeholder}
      disabled={disabled}
      size={sizeToMantine[size]}
      label={label}
      autosize
      minRows={minRows}
      maxRows={maxRows}
      error={error || undefined}
    />
  );
}
