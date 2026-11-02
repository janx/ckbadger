import type { ReactNode } from 'react';
import { DATA_COLOR, CLASS_COLORS } from '@/lib/design-tokens';
import { cn } from '@/lib/utils';

export interface AlignedBarSegment {
  key: string;
  label: string;
  value: number;
  /**
   * `overhead`: neutral grey (block overhead); `data`: jade (what users put on chain; a script's
   * CKB). `tokens` / `objects` / `identities` / `dao`: the asset class colour (table-rows R2).
   */
  tone: 'overhead' | 'data' | 'tokens' | 'objects' | 'identities' | 'dao';
}
export interface AlignedBarProps {
  label: string;
  /** Round page-max scale shared by every row of the column (see `resourceScale`). */
  scale: number;
  segments: readonly AlignedBarSegment[];
  /** Track length in px; every row of a column uses the same one (Blocks board: 120). */
  trackWidth?: number;
  /** Track height in px (table-rows T4: 8, `ALIGNED_BAR_HEIGHT`). */
  height?: number;
  /**
   * The narrowest a nonzero segment is drawn, in px, so a 0.1 % value is still a mark (table-rows
   * T4: 2). Zero is never drawn. Default 0: every segment at its exact width, as the bars have
   * always been drawn.
   */
  minFill?: number;
  /**
   * What is printed right of the track in place of the exact integer total, e.g. a CKB amount to
   * 2 dp. The printed text is then the value and the track only pictures it (`aria-hidden`).
   */
  display?: ReactNode;
  /** Width in px of the printed column; default the scale's digits, in `ch`. */
  displayWidth?: number;
  className?: string;
}

/** The bar's height on every row table (T4). */
export const ALIGNED_BAR_HEIGHT = 8;

const TONE = {
  overhead: CLASS_COLORS.ckb,
  data: DATA_COLOR,
  tokens: CLASS_COLORS.tokens,
  objects: CLASS_COLORS.objects,
  identities: CLASS_COLORS.identities,
  dao: CLASS_COLORS.dao,
} as const;
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
  height = ALIGNED_BAR_HEIGHT,
  minFill = 0,
  display,
  displayWidth,
  className,
}: AlignedBarProps) {
  if (!Number.isSafeInteger(scale) || scale <= 0)
    throw new Error(`${label}: invalid scale ${scale}`);
  if (!Number.isFinite(height) || height <= 0)
    throw new Error(`${label}: invalid bar height ${height}`);
  if (!Number.isFinite(minFill) || minFill < 0 || minFill > trackWidth)
    throw new Error(`${label}: invalid minimum fill ${minFill}`);
  for (const s of segments)
    if (!Number.isSafeInteger(s.value) || s.value < 0)
      throw new Error(`${label}: invalid ${s.label} ${s.value}`);
  const total = segments.reduce((sum, s) => sum + s.value, 0);
  if (total > scale) throw new Error(`${label}: ${total} exceeds the scale ${scale}`);
  // "Size, bytes: 3,205, 883 overhead, 2,322 user transactions": the total, then each part.
  const described = [
    fmt(total),
    ...(segments.length > 1 ? segments.map((s) => `${fmt(s.value)} ${s.label}`) : []),
  ];
  const width = (value: number) => {
    const exact = Math.round((value / scale) * trackWidth * 100) / 100;
    return value > 0 && exact < minFill ? minFill : exact;
  };
  return (
    <span data-aligned-bar className={cn('flex min-w-0 items-center gap-2.5', className)}>
      <span
        data-track
        {...(display === undefined
          ? { role: 'img', 'aria-label': `${label}: ${described.join(', ')}` }
          : { 'aria-hidden': true })}
        className="bg-base-elevated flex shrink-0 overflow-hidden rounded-sm"
        style={{ width: `${trackWidth}px`, height: `${height}px` }}
      >
        {segments.map((s) => (
          <span
            key={s.key}
            data-segment={s.key}
            className="h-full"
            style={{ width: `${width(s.value)}px`, backgroundColor: TONE[s.tone] }}
          />
        ))}
      </span>
      <span
        className="text-text shrink-0 text-right tabular-nums"
        style={{
          width: displayWidth === undefined ? `${fmt(scale).length}ch` : `${displayWidth}px`,
        }}
      >
        {display === undefined ? fmt(total) : display}
      </span>
    </span>
  );
}

export interface ScaleHeaderProps {
  label: string;
  /** The column's round scale, the same one its `AlignedBar`s take. */
  scale: number;
  /** The width of the column's bar track, so the ticks span it exactly. */
  trackWidth?: number;
  /** px between the name and the ticks (Blocks: 3; the transactions table has always drawn 2). */
  tickGap?: number;
  /** A tick's text; default the exact integer, en-US grouped ("4,000"). */
  format?: (value: number) => string;
  /** Also mark the middle of the scale: "0 · 25B · 50B" instead of "0 … 50B". */
  midpoint?: boolean;
}

/**
 * A bar column's header (F2 blocks, BA-02; lifted here in table-rows R2): "Size, bytes" over
 * "0 … 4,000", the column's shared scale, as wide as its bar track.
 */
export function ScaleHeader({
  label,
  scale,
  trackWidth = 120,
  tickGap = 3,
  format = fmt,
  midpoint = false,
}: ScaleHeaderProps) {
  if (!Number.isSafeInteger(scale) || scale <= 0)
    throw new Error(`${label}: invalid column scale ${scale}`);
  if (!Number.isFinite(tickGap) || tickGap < 0)
    throw new Error(`${label}: invalid tick gap ${tickGap}`);
  const ticks = midpoint ? [0, scale / 2, scale] : [0, scale];
  return (
    <span className="flex flex-col" style={{ gap: `${tickGap}px` }}>
      <span>{label}</span>
      <span
        data-scale-ticks
        aria-hidden="true"
        className="text-meta flex justify-between"
        style={{ width: `${trackWidth}px` }}
      >
        {ticks.map((t) => (
          <span key={t}>{t === 0 ? '0' : format(t)}</span>
        ))}
      </span>
    </span>
  );
}

/** The scale and the consensus cap live in one quiet footnote under the column (Blocks v3). */
export function AlignedBarFootnote({ scale, cap }: { scale: string; cap: string }) {
  return (
    <p className="text-text-dim text-meta">
      Scale {scale} · {cap}
    </p>
  );
}
