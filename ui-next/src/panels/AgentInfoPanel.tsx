import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import type { AgentCfg, Manifest } from '@/lib/types';
import { Eye, ArrowRight, ArrowLeft } from 'lucide-react';

interface Props {
  name: string;
  cfg: AgentCfg;
  context: string;
  count?: number;
  lastInvoked?: string;
  manifest?: Manifest | null;
  onSelect?: (name: string) => void;
}

function edges(c: AgentCfg): string[] {
  return c.can_talk_to || c.consults || [];
}

export function AgentInfoPanel({
  name,
  cfg,
  context,
  count,
  lastInvoked,
  manifest,
  onSelect,
}: Props) {
  const type = cfg.type || 'specialist';
  const outgoing = edges(cfg);
  const incoming = manifest
    ? Object.entries(manifest.agents)
        .filter(([n, a]) => n !== name && edges(a).includes(name))
        .map(([n]) => n)
    : [];

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <CardTitle className="flex items-center gap-2">
            <span>{cfg.emoji || '🤖'}</span>
            {name}
          </CardTitle>
          <Badge variant="outline">{type}</Badge>
        </div>
        {cfg.role && <p className="text-xs text-muted-foreground">{cfg.role}</p>}
      </CardHeader>
      <CardContent className="space-y-2 pt-0 text-xs">
        {cfg.model && (
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">modelo</span>
            <span className="font-mono">{cfg.model}</span>
          </div>
        )}
        {cfg.watch && (
          <div className="flex items-start gap-1 rounded-md bg-red-50 px-2 py-1.5 text-red-700">
            <Eye className="mt-0.5 h-3 w-3 shrink-0" />
            <div className="min-w-0">
              <div className="font-mono text-[10px] truncate">{cfg.watch.query}</div>
              <div className="text-[10px] text-red-500">
                threshold {cfg.watch.threshold}/{cfg.watch.window_minutes}min
              </div>
            </div>
          </div>
        )}
        {outgoing.length > 0 && (
          <div>
            <div className="mb-1 flex items-center gap-1 text-orange-600">
              <ArrowRight className="h-3 w-3" />
              <span className="text-[10px] font-semibold uppercase tracking-wide">fala com</span>
            </div>
            <div className="flex flex-wrap gap-1">
              {outgoing.map((c) => (
                <button
                  key={c}
                  onClick={() => onSelect?.(c)}
                  className="rounded-full border border-orange-200 bg-orange-50 px-2 py-0.5 text-[10px] font-medium text-orange-700 hover:bg-orange-100"
                >
                  {c}
                </button>
              ))}
            </div>
          </div>
        )}
        {incoming.length > 0 && (
          <div>
            <div className="mb-1 flex items-center gap-1 text-blue-600">
              <ArrowLeft className="h-3 w-3" />
              <span className="text-[10px] font-semibold uppercase tracking-wide">recebe de</span>
            </div>
            <div className="flex flex-wrap gap-1">
              {incoming.map((c) => (
                <button
                  key={c}
                  onClick={() => onSelect?.(c)}
                  className="rounded-full border border-blue-200 bg-blue-50 px-2 py-0.5 text-[10px] font-medium text-blue-700 hover:bg-blue-100"
                >
                  {c}
                </button>
              ))}
            </div>
          </div>
        )}
        {(count !== undefined || lastInvoked) && (
          <div className="flex items-center justify-between border-t pt-2 text-muted-foreground">
            <span>invocações: {count ?? 0}</span>
            {lastInvoked && <span className="text-[10px]">{lastInvoked.slice(0, 10)}</span>}
          </div>
        )}
        {context && (
          <details className="rounded-md border bg-muted/30 p-2">
            <summary className="cursor-pointer text-[11px] font-medium">mini-contexto</summary>
            <pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap text-[10px]">
              {context}
            </pre>
          </details>
        )}
      </CardContent>
    </Card>
  );
}
