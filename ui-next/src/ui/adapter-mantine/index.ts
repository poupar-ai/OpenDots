// Adapter Mantine — agrupa todos os wrappers. O `src/ui/index.ts` faz
// `export * from './adapter-mantine'` e é o ponto único de troca.

export { Box } from './Box';
export type { BoxProps } from './Box';

export { Stack } from './Stack';
export type { StackProps } from './Stack';

export { Group } from './Group';
export type { GroupProps } from './Group';

export { Paper } from './Paper';
export type { PaperProps } from './Paper';

export { Card } from './Card';
export type { CardProps } from './Card';

export { Text, Title, Code, Badge, Divider } from './typography';
export type { TextProps, TitleProps, CodeProps, BadgeProps } from './typography';

export { Button } from './Button';
export type { ButtonProps } from './Button';

export { TextInput } from './TextInput';
export type { TextInputProps } from './TextInput';

export { ActionIcon } from './ActionIcon';
export type { ActionIconProps } from './ActionIcon';

export { Alert } from './Alert';
export type { AlertProps } from './Alert';

export { Tooltip } from './Tooltip';
export type { TooltipProps } from './Tooltip';

export { CopyButton } from './CopyButton';
export type { CopyButtonProps } from './CopyButton';

export { Modal } from './Modal';
export type { ModalProps } from './Modal';

export { Radio, RadioGroup } from './Radio';
export type { RadioProps, RadioGroupProps } from './Radio';

export { Checkbox } from './Checkbox';
export type { CheckboxProps } from './Checkbox';

export { Chip } from './Chip';
export type { ChipProps } from './Chip';

export { UnstyledButton } from './UnstyledButton';
export type { UnstyledButtonProps } from './UnstyledButton';

export { colorVar } from './colors';
