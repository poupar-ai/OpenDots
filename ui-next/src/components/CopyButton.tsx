import { useState } from 'react';
import { Check, Copy } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

interface Props {
  value: string;
  label?: string;
  className?: string;
}

export function CopyButton({ value, label = 'copiar', className }: Props) {
  const [copied, setCopied] = useState(false);

  async function onClick(e: React.MouseEvent) {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(value);
    } catch {
      try {
        const ta = document.createElement('textarea');
        ta.value = value;
        ta.style.position = 'fixed';
        ta.style.opacity = '0';
        document.body.appendChild(ta);
        ta.select();
        document.execCommand('copy');
        document.body.removeChild(ta);
      } catch {}
    }
    setCopied(true);
    setTimeout(() => setCopied(false), 1200);
  }

  return (
    <Button
      size="icon"
      variant="ghost"
      type="button"
      onClick={onClick}
      title={copied ? 'copiado!' : label}
      aria-label={label}
      className={cn('h-6 w-6 text-slate-400 hover:text-slate-700', className)}
    >
      {copied ? <Check className="h-3.5 w-3.5 text-emerald-600" /> : <Copy className="h-3.5 w-3.5" />}
    </Button>
  );
}
