import { FileText, Database, AlertTriangle, ScrollText, Package } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import type { ServerData } from '@/lib/types';

interface Props {
  data: ServerData | null;
}

export function ModulesPanel({ data }: Props) {
  if (!data) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        carregando…
      </div>
    );
  }

  const stats = data.stats || {};

  return (
    <div className="space-y-3">
      <Card>
        <CardHeader className="pb-2">
          <div className="flex items-center justify-between">
            <CardTitle className="flex items-center gap-2">
              <Package className="h-4 w-4 text-primary" />
              {data.project.name}
            </CardTitle>
            <Badge variant="outline">project-mem</Badge>
          </div>
          <p className="font-mono text-[10px] text-muted-foreground">{data.project.root}</p>
        </CardHeader>
      </Card>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-xs uppercase tracking-wide text-muted-foreground">
            Memória
          </CardTitle>
        </CardHeader>
        <CardContent className="grid grid-cols-2 gap-2 pt-0">
          <Stat icon={<FileText className="h-3.5 w-3.5" />} label="sessões" value={stats.sessions} />
          <Stat
            icon={<ScrollText className="h-3.5 w-3.5" />}
            label="observações"
            value={stats.observations}
          />
          <Stat
            icon={<Database className="h-3.5 w-3.5" />}
            label="decisões"
            value={stats.decisions}
          />
          <Stat
            icon={<AlertTriangle className="h-3.5 w-3.5" />}
            label="gotchas"
            value={stats.gotchas}
          />
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-xs uppercase tracking-wide text-muted-foreground">
            Documentos
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-1.5 pt-0">
          <DocRow label="SOUL.md" present={!!data.soulMd} />
          <DocRow label="PROJECT.md" present={!!data.projectMd} />
          <DocRow label="DECISIONS.md" present={!!data.decisionsMd} />
          <DocRow label="GOTCHAS.md" present={!!data.gotchasMd} />
          <DocRow label="GLOSSARY.md" present={!!data.glossaryMd} />
          <DocRow label="CODE_INDEX.md" present={!!data.codeIndexMd} />
        </CardContent>
      </Card>

      {stats.codeIndex && (
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-xs uppercase tracking-wide text-muted-foreground">
              Code Index
            </CardTitle>
          </CardHeader>
          <CardContent className="pt-0">
            <div className="flex items-center justify-between text-xs">
              <span className="text-muted-foreground">arquivos</span>
              <span className="font-mono">{stats.codeIndex.files}</span>
            </div>
            <div className="flex items-center justify-between text-xs">
              <span className="text-muted-foreground">símbolos</span>
              <span className="font-mono">{stats.codeIndex.symbols}</span>
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );
}

function Stat({ icon, label, value }: { icon: React.ReactNode; label: string; value: number }) {
  return (
    <div className="flex items-center gap-2 rounded-md bg-muted/40 px-2 py-1.5">
      <span className="text-muted-foreground">{icon}</span>
      <div className="flex flex-col leading-tight">
        <span className="text-base font-semibold">{value ?? 0}</span>
        <span className="text-[10px] uppercase tracking-wide text-muted-foreground">{label}</span>
      </div>
    </div>
  );
}

function DocRow({ label, present }: { label: string; present: boolean }) {
  return (
    <div className="flex items-center justify-between text-xs">
      <span className="font-mono text-slate-600">{label}</span>
      <Badge variant={present ? 'success' : 'outline'}>{present ? 'ok' : 'faltando'}</Badge>
    </div>
  );
}
