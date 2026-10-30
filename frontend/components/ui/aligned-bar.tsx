import { DATA_COLOR, CLASS_COLORS } from '@/lib/design-tokens';
import { cn } from '@/lib/utils';

export interface AlignedBarSegment {
  key: string;
  label: string;
  value: number;
  /** `overhead`: neutral grey (block overhead); `data`: jade (what users put on chain). */
  tone: 'overhead' | 'data';
}
export interface AlignedBarProps {
  label: string;
  /** Round page-max scale shared by every row of the column (see `resourceScale`). */
  scale: number;
  segments: readonly AlignedBarSegment[];
  /** Track length in px; every row of a column uses the same one (Blocks board: 120). */
  trackWidth?: number;
  className?: string;
}

const TONE = { overhead: CLASS_COLORS.ckb, data: DATA_COLOR } as const;
const fmt = (n: number) => n.toLocaleString('en-US');

/**
 * A linear bar on a shared left baseline (BA-01, Blocks v3): the fixed track comes first so every
 * row starts at the same x, then the exact value in a column as wide as the scale's digits.
 */
export function AlignedBar({
  label,
  scale,
  segments,
  trackWidth = 120,
  className,
}: AlignedBarProps) {
  if (!Number.isSafeInteger(scale) || scale <= 0)
    throw new Error(`${label}: invalid scale ${scale}`);
  for (const s of segments)
    if (!Number.isSafeInteger(s.value) || s.value < 0)
      throw new Error(`${label}: invalid ${s.label} ${s.value}`);
  const total = segments.reduce((sum, s) => sum + s.value, 0);
  if (total > scale) throw new Error(`${label}: ${total} exceeds the scale ${scale}`);
  const described = [
    fmt(total),
    ...(segments.length > 1 ? segments.map((s) => `${s.label} ${fmt(s.value)}`) : []),
  ];
  return (
    <span data-aligned-bar className={cn('flex min-w-0 items-center gap-2.5', className)}>
      <span
        data-track
        role="img"
        aria-label={`${label}: ${described.join(' · ')}`}
        className="bg-base-elevated flex h-2 shrink-0 overflow-hidden rounded-sm"
        style={{ width: `${trackWidth}px` }}
      >
        {segments.map((s) => (
          <span
            key={s.key}
            data-segment={s.key}
            className="h-full"
            style={{
              width: `${Math.round((s.value / scale) * trackWidth * 100) / 100}px`,
              backgroundColor: TONE[s.tone],
            }}
          />
        ))}
      </span>
      <span
        className="text-text shrink-0 text-right tabular-nums"
        style={{ width: `${fmt(scale).length}ch` }}
      >
        {fmt(total)}
      </span>
    </span>
  );
}

/** The scale and the consensus cap live in one quiet footnote under the column (Blocks v3). */
export function AlignedBarFootnote({ scale, cap }: { scale: string; cap: string }) {
  return (
    <p className="text-text-dim text-[11px]">
      Scale {scale} · {cap}
    </p>
  );
}
