import { Paper, Text } from '@/ui';

/** Separador de dia ("Hoje", "Ontem", data) — pílula centralizada. */
export function DayDivider({ label }: { label: string }) {
  return (
    <div className="flex justify-center py-2">
      <Paper px={12} py={3} shadow="xs" style={{ borderRadius: 9999 }}>
        <Text size="xs" muted fw={500}>
          {label}
        </Text>
      </Paper>
    </div>
  );
}
