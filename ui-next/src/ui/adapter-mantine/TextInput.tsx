import type { ChangeEvent } from 'react';
import { TextInput as MTextInput } from '@mantine/core';
import { sizeToMantine } from './tokens';
import type { Size } from '../types';

export interface TextInputProps {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  disabled?: boolean;
  size?: Size;
  spellCheck?: boolean;
  flex?: number;
  type?: 'text' | 'password' | 'email' | 'url' | 'number';
  /** Mensagem de erro (vermelho abaixo do input). */
  error?: string | null;
}

export function TextInput({
  value,
  onChange,
  placeholder,
  disabled,
  size = 'sm',
  spellCheck,
  flex,
  type,
  error,
}: TextInputProps) {
  return (
    <MTextInput
      value={value}
      onChange={(e: ChangeEvent<HTMLInputElement>) => onChange(e.currentTarget.value)}
      placeholder={placeholder}
      disabled={disabled}
      size={sizeToMantine[size]}
      spellCheck={spellCheck}
      flex={flex}
      type={type}
      error={error || undefined}
    />
  );
}
