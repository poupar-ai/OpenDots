import { cn } from '@/lib/utils';

type AgentType = 'core' | 'specialist' | 'integrator' | 'motor' | 'firefighter';

const PALETTE: Record<AgentType, { primary: string; accent: string }> = {
  core:        { primary: '#6366f1', accent: '#312e81' },
  specialist:  { primary: '#0ea5e9', accent: '#0c4a6e' },
  integrator:  { primary: '#a855f7', accent: '#581c87' },
  motor:       { primary: '#f97316', accent: '#7c2d12' },
  firefighter: { primary: '#ef4444', accent: '#7f1d1d' },
};

const PROFESSION: Record<string, { primary: string; accent: string }> = {
  devs: { primary: '#16a34a', accent: '#14532d' },
};

const SKIN = '#fcd9b6';
const OUTLINE = '#0f172a';

interface Props {
  type?: string | null;
  profession?: string | null;
  size?: number;
  mood?: 'neutral' | 'happy' | 'thinking';
  className?: string;
}

export function AgentFace({
  type = 'specialist',
  profession,
  size = 28,
  mood = 'neutral',
  className,
}: Props) {
  const p =
    (profession && PROFESSION[profession]) ||
    PALETTE[(type as AgentType) || 'specialist'] ||
    PALETTE.specialist;
  const smile = mood === 'happy';

  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 48 48"
      className={cn('shrink-0', className)}
      aria-hidden
    >
      <circle
        cx={24}
        cy={24}
        r={23}
        fill={p.primary}
        stroke={OUTLINE}
        strokeOpacity={0.35}
        strokeWidth={1}
      />
      <path
        d="M 8 34 Q 24 48 40 34 L 40 46 L 8 46 Z"
        fill={p.accent}
        opacity={0.7}
      />
      <circle
        cx={24}
        cy={22}
        r={13}
        fill={SKIN}
        stroke={OUTLINE}
        strokeOpacity={0.45}
        strokeWidth={1.2}
      />
      <circle cx={19.5} cy={21.5} r={1.6} fill={OUTLINE} />
      <circle cx={28.5} cy={21.5} r={1.6} fill={OUTLINE} />
      {smile ? (
        <path
          d="M 20.5 27 Q 24 30 27.5 27"
          stroke={OUTLINE}
          strokeWidth={1.3}
          fill="none"
          strokeLinecap="round"
        />
      ) : (
        <line
          x1={21.5}
          y1={27.5}
          x2={26.5}
          y2={27.5}
          stroke={OUTLINE}
          strokeWidth={1.3}
          strokeLinecap="round"
        />
      )}
      {type === 'firefighter' && (
        <path
          d="M 13 13 Q 13 6 24 6 Q 35 6 35 13 L 35 14 L 13 14 Z"
          fill={p.primary}
          stroke={OUTLINE}
          strokeOpacity={0.5}
          strokeWidth={1}
        />
      )}
      {type === 'core' && (
        <circle
          cx={24}
          cy={9}
          r={2.2}
          fill="#fbbf24"
          stroke={OUTLINE}
          strokeOpacity={0.5}
          strokeWidth={0.8}
        />
      )}
      {type === 'motor' && (
        <g>
          <circle cx={32} cy={10} r={2.4} fill="#94a3b8" stroke={OUTLINE} strokeWidth={0.8} />
          <circle cx={32} cy={10} r={0.9} fill={OUTLINE} />
        </g>
      )}
      {type === 'integrator' && (
        <g>
          <line x1={24} y1={6} x2={24} y2={10} stroke={OUTLINE} strokeWidth={1.2} />
          <circle cx={24} cy={5} r={2.2} fill="#22d3ee" stroke={OUTLINE} strokeOpacity={0.5} strokeWidth={0.8} />
        </g>
      )}
      {type === 'specialist' && (
        <path
          d="M 12 11 Q 24 6 36 11 L 34 15 Q 24 11 14 15 Z"
          fill={p.accent}
          opacity={0.85}
        />
      )}
    </svg>
  );
}
