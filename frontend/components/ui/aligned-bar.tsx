import type { ReactNode } from 'react';
import { DATA_COLOR, CLASS_COLORS } from '@/lib/design-tokens';
import { cn } from '@/lib/utils';
import { fontPx, textPx } from '@/lib/type-metrics';
import { formatTick } from '@/lib/chart-ticks';
import { WIDEST_COMPACT_TICK } from '@/lib/list-bounds';

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

/** px between an AlignedBar's track and its figure, its `gap-2.5`; a head over the figure spans it. */
export const BAR_FIGURE_GAP = 10;

/**
 * The shortest track a list bar is drawn on (readability plan 2026-11-18, lane F3-e): a share or a
 * count still reads as a length on it, and the figure beside it keeps its full width.
 */
export const MIN_TRACK_PX = 48;

/**
 * The rule of a bar column's head (readability F3-e, the coordinator's ruling): a head's tick row
 * is in the tick form and stands whole over its track. Its ticks are axis labels, and an axis
 * label is written in the tick form everywhere on the site (`formatTick`: "600k", "1.5M", "4B");
 * the head's unit stays in its label ("Size, bytes", "Cycles"), and the exact figure beside each
 * bar stays exact. The track holds the row whole, one glyph between neighbouring ticks so "0" and
 * "600k" never read as one number, and is never shorter than `MIN_TRACK_PX`: `trackPx` measures it
 * from the widest ticks the column's scale writes.
 */
export function trackPx(ticks: readonly string[]): number {
  if (!ticks.length || ticks.some((t) => !t)) throw new Error('trackPx: an empty tick row');
  const glyphs = ticks.reduce((n, t) => n + t.length, 0) + ticks.length - 1;
  return Math.max(MIN_TRACK_PX, textPx(glyphs, fontPx('meta')));
}

/**
 * A head's tick in the tick form: the default of every `ScaleHeader`. An axis maximum states the
 * scale its bars are drawn on (review fb-logic 7, the lead's ruling), so its text is exactly its
 * value: `formatTick` writes every digit a tick has ("1.234567M" for 1,234,567; the lead's ruling
 * of 2026-11-19), never a rounded text, and its own tests hold that.
 */
export function tickForm(value: number): string {
  return formatTick(value, value);
}

/**
 * The track of a bar on a count scale (bytes, cycles, holders, items): "0" and the widest tick the
 * tick form writes for any such scale up to the cycle limit ("750M", four characters), 43.2 px,
 * so the 48 px a bar keeps at least.
 */
export const COUNT_TRACK_PX = trackPx(['0', WIDEST_COMPACT_TICK]);

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
        className="bg-bar-track flex shrink-0 overflow-hidden"
        style={{ width: `${trackWidth}px`, height: `${height}px` }}
      >
        {segments.map((s) => (
          <span
            key={s.key}
            data-segment={s.key}
            className="scan-fill h-full"
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
  /** A tick's text; default the tick form (`tickForm`, "4k"), the rule at `trackPx`. */
  format?: (value: number) => string;
  /** Also mark the middle of the scale: "0 · 25B · 50B" instead of "0 … 50B". */
  midpoint?: boolean;
  /**
   * The width in px of the figure the column's bars print right of their track (their
   * `displayWidth`). Given, the name stands over that figure, ending where it ends, and the ticks
   * stay over the track (readability B5: a head stands over what it names, and in a column of a
   * bar and its figure that is the figure). Without it the name stands over the track.
   */
  figureWidth?: number;
}

/**
 * A bar column's header (F2 blocks, BA-02; lifted here in table-rows R2): "Size, bytes" over
 * "0 … 4k", the column's shared scale in the tick form, as wide as its bar track (the rule at
 * `trackPx`).
 */
export function ScaleHeader({
  label,
  scale,
  trackWidth = 120,
  tickGap = 3,
  format = tickForm,
  midpoint = false,
  figureWidth,
}: ScaleHeaderProps) {
  if (!Number.isSafeInteger(scale) || scale <= 0)
    throw new Error(`${label}: invalid column scale ${scale}`);
  if (!Number.isFinite(tickGap) || tickGap < 0)
    throw new Error(`${label}: invalid tick gap ${tickGap}`);
  if (figureWidth !== undefined && !(Number.isFinite(figureWidth) && figureWidth > 0))
    throw new Error(`${label}: invalid figure width ${figureWidth}`);
  const ticks = midpoint ? [0, scale / 2, scale] : [0, scale];
  return (
    <span className="flex flex-col" style={{ gap: `${tickGap}px` }}>
      {/* A column head at meta (table-rows T5), whatever cell holds it; over the figure, it runs
          from the track's start to the figure's end and stands at that end (a longer name reaches
          left over the track, never past the figure). */}
      {figureWidth === undefined ? (
        <span className="text-meta">{label}</span>
      ) : (
        <span
          className="text-meta flex justify-end whitespace-nowrap"
          style={{ width: `${trackWidth + BAR_FIGURE_GAP + figureWidth}px` }}
        >
          {label}
        </span>
      )}
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
