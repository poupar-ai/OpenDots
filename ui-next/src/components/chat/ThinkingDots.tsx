/**
 * Indicador "pensando…" — três bolinhas animadas. Reaproveitável em qualquer
 * lista de chat (bubble do agente, header, preview). Animação em index.css
 * (`.typing-dot` / `@keyframes typing-bounce`).
 */
export function ThinkingDots({ label = 'pensando' }: { label?: string }) {
  return (
    <div className="flex items-center gap-1.5 py-0.5">
      <span className="typing-dot" />
      <span className="typing-dot" />
      <span className="typing-dot" />
      {label && <span className="ml-1 text-xs text-slate-500">{label}</span>}
    </div>
  );
}
