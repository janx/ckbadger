'use client';
// F2 group `objects` (OB-7): the Object board's Sanger-style sequencing gel. 16 lanes, one per hex
// digit; the DNA's hex digits run top to bottom, each a band in the lane of its value, so reading
// the bands spells the DNA. Bands take the colour of the gene that owns their byte; each gene has
// a bracket and its reading (bytes · rule, raw → trait) to the right, level with its bands.
//
// Responsive (V3): drawn 1:1 for its measured box. Where every gene's reading fits beside the lanes
// it is the board's side-by-side map; narrower, the lanes keep up to 20 px and the gene columns
// become rows under the map (name · bytes · raw → value), the value the last thing to lose room.
// A tap on a gene pins it (`usePinnedReadout`): its span lights on the map, its row in the list.
import { useEffect, useRef, type ReactNode } from 'react';
import type { SequencingGene, SequencingLayout } from '@/lib/dob-genes';
import { GEL_COLORS } from '@/lib/design-tokens';
import { clipLabel, shortenHash } from '@/lib/shorten';
import { ShortId } from '@/components/ui/short-id';
import { TraitText } from '@/components/asset/trait-text';
import { useElementWidth } from '@/hooks/useElementWidth';
import { usePinnedReadout, type PinnedTargetProps } from '@/hooks/usePinnedReadout';
import { ReadoutRow } from '@/components/ui/readout-row';
import { cn } from '@/lib/utils';
import { tapOrHover } from '@/components/ui/tap-or-hover-hint';
import { formatCount } from '@/lib/format-count';
import { unitText } from '@/components/ui/units';
import { SVG_TEXT } from '@/lib/chart-type';
import { advancePx, withAllowance } from '@/lib/type-metrics';

/** The side-by-side map: lanes from LX, brackets at GX, the gene columns from TX. */
const WIDE = { LX: 38, LANE: 15, TOP: 44, STEP: 3.3, LABEL_GAP: 15 } as const;
const GX = WIDE.LX + 16 * WIDE.LANE + 10;
const TX = GX + 16;
/**
 * The gene columns' allowances: their glyphs were measured at 0.636 em in Chromium, 0.44 px a glyph
 * over the face's 0.6 em at the head size (12 px) and 0.42 px at the value size (11.5 px).
 */
const HEAD_MEASURED_ALLOWANCE = 0.44;
const VALUE_MEASURED_ALLOWANCE = 0.42;
/** One character of a gene's name, arrow and value (the head size) and of its bytes and raw (the value size). */
export const CHAR_12 = withAllowance(advancePx(SVG_TEXT.head), HEAD_MEASURED_ALLOWANCE);
export const CHAR_115 = withAllowance(advancePx(SVG_TEXT.value), VALUE_MEASURED_ALLOWANCE);
// A value that is longer is a label cut to this budget. 27 is the most the wide gel holds at the
// DNA plate at 1280 (896 px) for the flagship DNA (Nervape): its columns before the value end at
// 684.28 px (the gene columns from 304; the longest name, "prev.type", 9 × 7.64 + 10; the longest
// bytes · rule, "bytes 51–52 · nervapeNote", 25 × 7.32 + 12; the raw at the tight size, 11 × 7.32
// + 8; the arrow's 16; the 2 px end), and 896 − 684.28 = 211.72 px holds 27 characters (206.28)
// but not 28 (213.92). An inscription, "inscription a94617…79e0i0" (25, its txid at the tight size,
// U-09), is never cut.
const VALUE_MAX = 27;
const NAME_MAX = 11;
/** The stacked map: a ruler column, then lanes of up to 20 px and never under 14. */
const NARROW = { RULER: 34, MAX_LANE: 20, MIN_LANE: 14, TOP: 26, STEP: 7 } as const;

const r = (v: number) => Math.round(v * 10) / 10;

const bytesOf = (g: SequencingGene) =>
  g.length === 1 ? `byte ${g.offset}` : `bytes ${g.offset}–${g.offset + g.length - 1}`;
const valueOf = (g: SequencingGene) => g.value ?? 'not decoded yet';
const quiet = (g: SequencingGene) => g.value === 'empty' || g.value === null;

/**
 * The gene columns of the side-by-side map, each as wide as its longest text: name, bytes · rule,
 * raw, the arrow, then the value; `needed` is the canvas width that holds every reading whole.
 */
function wideColumns(genes: readonly SequencingGene[]) {
  const longest = (texts: string[]) => Math.max(0, ...texts.map((t) => t.length));
  const bytes = TX + longest(genes.map((g) => clipLabel(g.name, NAME_MAX))) * CHAR_12 + 10;
  const raw = bytes + longest(genes.map((g) => `${bytesOf(g)} · ${g.rule}`)) * CHAR_115 + 12;
  const arrow = raw + longest(genes.map((g) => shortenHash(g.raw, 'tight'))) * CHAR_115 + 8;
  const value = arrow + 16;
  const needed = value + longest(genes.map((g) => clipLabel(valueOf(g), VALUE_MAX))) * CHAR_12 + 2;
  const at = (x: number) => Math.ceil(x);
  return { name: TX, bytes: at(bytes), raw: at(raw), arrow: at(arrow), value: at(value), needed };
}

/** Which arrangement a box of `width` px takes: side by side only when every reading fits. */
export function sequencingGelForm(genes: readonly SequencingGene[], width: number) {
  return width >= wideColumns(genes).needed ? 'wide' : 'narrow';
}

export interface SequencingGelProps {
  layout: SequencingLayout;
  subject: string;
  /** The gene (by name) lit by any of the three linked views; null when none is. */
  active: string | null;
  onActive: (gene: string | null) => void;
  /**
   * Consistency round C10: the readout's words while nothing is hovered or pinned; default
   * "Tap or hover a gene.". A page whose readout row already carries the section's one hint
   * passes words without it.
   */
  idle?: ReactNode;
}

interface FormProps {
  layout: SequencingLayout;
  subject: string;
  width: number;
  lit: string | null;
  bind: (name: string) => PinnedTargetProps;
}

export function SequencingGel({ layout, subject, active, onActive, idle }: SequencingGelProps) {
  const { bytes, genes } = layout;
  const [box, measured] = useElementWidth<HTMLDivElement>();
  const width = measured === null ? null : Math.floor(measured);
  const readout = usePinnedReadout<string>();
  // The gel's own hover or pin is reported up, so the trait list and the allele gel follow it.
  const report = useRef(onActive);
  report.current = onActive;
  useEffect(() => report.current(readout.active), [readout.active]);
  const lit = active ?? readout.active;
  const gene = lit === null ? undefined : genes.find((g) => g.name === lit);
  const form = width === null ? null : sequencingGelForm(genes, width);
  const n = bytes.length * 2;
  return (
    <div ref={box} className="min-w-0 space-y-2">
      {width !== null && form === 'wide' && (
        <WideGel layout={layout} subject={subject} width={width} lit={lit} bind={readout.bind} />
      )}
      {width !== null && form === 'narrow' && (
        <NarrowGel layout={layout} subject={subject} width={width} lit={lit} bind={readout.bind} />
      )}
      <p className="text-text-dim text-meta">
        read top to bottom: each band is one hex digit in the lane of its value · {n} digits ={' '}
        {unitText(formatCount(bytes.length, 'byte'))}
        {form === 'narrow' ? ' · dashed rules mark the gene boundaries' : ''}
      </p>
      <ReadoutRow
        label="Gene readout"
        pinned={lit !== null && lit === readout.pinned}
        idle={idle ?? tapOrHover('a gene.', { capital: true })}
      >
        {gene ? (
          <TraitText
            text={`${gene.name} · ${bytesOf(gene)} · ${gene.rule} · ${shortenHash(gene.raw, 'tight')} → ${valueOf(gene)}`}
          />
        ) : null}
      </ReadoutRow>
    </div>
  );
}

function describe(subject: string, layout: SequencingLayout): string {
  const genes = layout.genes
    .map(
      (g) =>
        `${g.name} ${g.length === 1 ? `byte ${g.offset}` : `bytes ${g.offset} to ${g.offset + g.length - 1}`}${g.value ? `, ${g.value}` : ''}`
    )
    .join('; ');
  return `${subject}: the ${layout.bytes.length}-byte DNA as a sequencing gel: 16 lanes for hex digits 0 to f, its ${layout.bytes.length * 2} hex digits as bands from top to bottom. Genes: ${genes}.`;
}

/** The board's map: lanes left, each gene's bracket and reading level with its bands. */
function WideGel({ layout, subject, width, lit, bind }: FormProps) {
  const { bytes, genes, owners } = layout;
  const { LX, LANE, TOP, STEP, LABEL_GAP } = WIDE;
  const col = wideColumns(genes);
  const yOf = (pos: number) => TOP + pos * STEP;
  const digits = bytes.join('').split('');
  const n = digits.length;
  const height = TOP + n * STEP + 40;
  const byKey = new Map(genes.map((g) => [g.key, g]));
  const inLit = (byte: number) =>
    genes.some((g) => g.name === lit && byte >= g.offset && byte < g.offset + g.length);
  // Labels sit level with their genes, pushed down just enough not to overlap (DNA order).
  let floor = -Infinity;
  const labelY = new Map<string, number>();
  for (const g of genes) {
    const mid = (yOf(g.offset * 2) + yOf((g.offset + g.length) * 2 - 1)) / 2 + 4;
    const y = Math.max(mid, floor + LABEL_GAP);
    labelY.set(g.key, y);
    floor = y;
  }
  const labelsEnd = Math.max(height, floor + 12);
  return (
    <svg
      role="img"
      aria-label={describe(subject, layout)}
      viewBox={`0 0 ${width} ${r(labelsEnd)}`}
      width={width}
      height={r(labelsEnd)}
      className="block overflow-visible"
    >
      {Array.from({ length: 16 }, (_, d) => {
        const x = LX + d * LANE;
        return (
          <g key={d} aria-hidden>
            <text
              x={r(x + LANE / 2)}
              y="16"
              textAnchor="middle"
              fontSize={SVG_TEXT.tick}
              fill={GEL_COLORS.label}
            >
              {d.toString(16)}
            </text>
            <rect
              x={r(x + 2)}
              y="24"
              width={LANE - 4}
              height="4"
              fill="none"
              stroke={GEL_COLORS.well}
              strokeOpacity=".3"
              strokeWidth=".6"
            />
            <rect
              x={r(x + 2.5)}
              y={TOP - 6}
              width={LANE - 5}
              height={r(n * STEP + 8)}
              fill={GEL_COLORS.lane}
              fillOpacity=".025"
            />
          </g>
        );
      })}
      {Array.from({ length: Math.floor(bytes.length / 8) + 1 }, (_, i) => i * 8).map((b) => (
        <g key={b} data-ruler={b} aria-hidden>
          <line
            x1={LX - 9}
            x2={LX - 4}
            y1={r(yOf(b * 2))}
            y2={r(yOf(b * 2))}
            stroke={GEL_COLORS.ruler}
          />
          <text
            x={LX - 12}
            y={r(yOf(b * 2) + 3.5)}
            textAnchor="end"
            fontSize={SVG_TEXT.tick}
            fill={GEL_COLORS.rulerText}
          >
            {b}
          </text>
        </g>
      ))}
      <text
        data-ruler={bytes.length}
        x={LX - 12}
        y={r(yOf(n) + 4)}
        textAnchor="end"
        fontSize={SVG_TEXT.tick}
        fill={GEL_COLORS.rulerText}
      >
        {bytes.length}
      </text>
      <text
        x={LX - 12}
        y={r(yOf(n) + 18)}
        textAnchor="end"
        fontSize={SVG_TEXT.tick}
        fill={GEL_COLORS.rulerText}
      >
        byte
      </text>
      {digits.map((digit, pos) => {
        const byte = Math.floor(pos / 2);
        const owner = owners[byte] === null ? undefined : byKey.get(owners[byte]!);
        const value = parseInt(digit, 16);
        return (
          <rect
            key={pos}
            data-nibble={pos}
            x={r(LX + value * LANE + 2.5)}
            y={r(yOf(pos) - 0.8)}
            width={LANE - 5}
            height="1.6"
            fill={owner?.color ?? GEL_COLORS.label}
            fillOpacity={bytes[byte] === '00' ? 0.45 : 0.95}
            opacity={lit && !inLit(byte) ? 0.15 : 1}
          />
        );
      })}
      {genes.map((g) => {
        const y1 = yOf(g.offset * 2) - 1.6;
        const y2 = yOf((g.offset + g.length) * 2 - 1) + 1.6;
        const ly = labelY.get(g.key)!;
        const mid = (y1 + y2) / 2;
        return (
          <g
            key={g.key}
            data-gene={g.name}
            opacity={lit && lit !== g.name ? 0.12 : 1}
            aria-label={`${g.name}, ${bytesOf(g)}`}
            {...bind(g.name)}
            className="cursor-pointer transition-opacity"
          >
            <title>{`${g.name} · ${bytesOf(g)} · ${g.rule} · 0x${g.raw} → ${valueOf(g)}`}</title>
            <rect x={LX} y={r(y1)} width={16 * LANE} height={r(y2 - y1)} fill="transparent" />
            {g.owns ? (
              <>
                <path
                  d={`M${GX},${r(y1)} H${GX + 5} V${r(y2)} H${GX}`}
                  fill="none"
                  stroke={g.color}
                  strokeWidth="1"
                />
                <path
                  d={`M${GX + 5},${r(mid)} H${GX + 12} L${TX - 4},${r(ly - 4)}`}
                  fill="none"
                  stroke={g.color}
                  strokeOpacity=".5"
                  strokeWidth=".6"
                />
              </>
            ) : (
              <>
                <rect
                  x={LX + 1}
                  y={r(y1 - 1)}
                  width={16 * LANE - 2}
                  height={r(y2 - y1 + 2)}
                  fill="none"
                  stroke={g.color}
                  strokeOpacity=".6"
                  strokeDasharray="2 2"
                />
                <path
                  d={`M${LX + 16 * LANE},${r(mid)} H${GX + 12} L${TX - 4},${r(ly - 4)}`}
                  fill="none"
                  stroke={g.color}
                  strokeOpacity=".4"
                  strokeWidth=".6"
                />
              </>
            )}
            <rect x={TX - 2} y={r(ly - 13)} width={width - TX + 2} height="17" fill="transparent" />
            <text x={col.name} y={r(ly)} fontSize={SVG_TEXT.head} fill={g.color}>
              {clipLabel(g.name, NAME_MAX)}
            </text>
            <text x={col.bytes} y={r(ly)} fontSize={SVG_TEXT.value} fill={GEL_COLORS.label}>
              {bytesOf(g)} · {g.rule}
            </text>
            <text x={col.raw} y={r(ly)} fontSize={SVG_TEXT.value} fill={g.color}>
              {shortenHash(g.raw, 'tight')}
            </text>
            <text x={col.arrow} y={r(ly)} fontSize={SVG_TEXT.head} fill={GEL_COLORS.arrow}>
              →
            </text>
            <text
              x={col.value}
              y={r(ly)}
              fontSize={SVG_TEXT.head}
              fill={quiet(g) ? GEL_COLORS.rulerText : GEL_COLORS.well}
            >
              {clipLabel(valueOf(g), VALUE_MAX)}
            </text>
          </g>
        );
      })}
    </svg>
  );
}

/**
 * The stacked map (V3 board): lanes of up to 20 px over the whole width, dashed rules where a gene
 * begins, the lit gene's span outlined; the gene readings as rows under the map.
 */
function NarrowGel({ layout, subject, width, lit, bind }: FormProps) {
  const { bytes, genes, owners } = layout;
  const { RULER, MAX_LANE, MIN_LANE, TOP, STEP } = NARROW;
  const lane = Math.min(MAX_LANE, Math.floor((width - RULER) / 16));
  if (lane < MIN_LANE)
    throw new Error(
      `${subject}: a ${width} px DNA map cannot hold 16 lanes of ${MIN_LANE} px (${RULER} px ruler)`
    );
  const yOf = (pos: number) => TOP + pos * STEP + STEP / 2;
  const digits = bytes.join('').split('');
  const n = digits.length;
  const bottom = TOP + n * STEP;
  const height = bottom + 26;
  const byKey = new Map(genes.map((g) => [g.key, g]));
  const litGenes = genes.filter((g) => g.name === lit);
  const inLit = (byte: number) =>
    litGenes.some((g) => byte >= g.offset && byte < g.offset + g.length);
  const mapW = 16 * lane;
  // One line a gene (name · bytes · raw → value) while the longest reading fits its column;
  // otherwise the reading takes a line of its own, so the value is the last to lose room.
  const readingWidth = Math.max(
    0,
    ...genes.map((g) => (shortenHash(g.raw, 'tight').length + 3 + valueOf(g).length) * CHAR_12)
  );
  const oneLine = width - 88 - 84 - 2 * 10 >= readingWidth;
  return (
    <>
      <svg
        role="img"
        aria-label={describe(subject, layout)}
        viewBox={`0 0 ${width} ${height}`}
        width={width}
        height={height}
        className="block overflow-visible"
      >
        <rect
          x={RULER}
          y={TOP - 4}
          width={mapW}
          height={n * STEP + 8}
          fill={GEL_COLORS.lane}
          fillOpacity=".025"
        />
        {Array.from({ length: 16 }, (_, d) => (
          <g key={d} aria-hidden>
            <text
              x={RULER + d * lane + lane / 2}
              y="12"
              textAnchor="middle"
              fontSize={SVG_TEXT.tick}
              fill={GEL_COLORS.label}
            >
              {d.toString(16)}
            </text>
            {d > 0 && (
              <line
                x1={RULER + d * lane}
                x2={RULER + d * lane}
                y1={TOP - 4}
                y2={bottom + 4}
                stroke={GEL_COLORS.ruler}
                strokeOpacity=".35"
              />
            )}
          </g>
        ))}
        {Array.from({ length: Math.floor(bytes.length / 8) + 1 }, (_, i) => i * 8).map((b) => (
          <text
            key={b}
            data-ruler={b}
            x={RULER - 8}
            y={r(yOf(b * 2) + 3.5)}
            textAnchor="end"
            fontSize={SVG_TEXT.tick}
            fill={GEL_COLORS.rulerText}
          >
            {b}
          </text>
        ))}
        <text
          data-ruler={bytes.length}
          x={RULER - 8}
          y={r(bottom + 4)}
          textAnchor="end"
          fontSize={SVG_TEXT.tick}
          fill={GEL_COLORS.rulerText}
        >
          {bytes.length}
        </text>
        <text
          x={RULER - 8}
          y={r(bottom + 18)}
          textAnchor="end"
          fontSize={SVG_TEXT.tick}
          fill={GEL_COLORS.rulerText}
        >
          byte
        </text>
        {genes
          .filter((g) => g.owns && g.offset > 0)
          .map((g) => (
            <line
              key={g.key}
              data-boundary={g.name}
              x1={RULER}
              x2={RULER + mapW}
              y1={TOP + g.offset * 2 * STEP}
              y2={TOP + g.offset * 2 * STEP}
              stroke={g.color}
              strokeOpacity=".5"
              strokeDasharray="2 3"
            />
          ))}
        {litGenes.map((g) => (
          <rect
            key={g.key}
            data-span={g.name}
            x={RULER + 0.5}
            y={TOP + g.offset * 2 * STEP + 0.5}
            width={mapW - 1}
            height={g.length * 2 * STEP - 1}
            fill={g.color}
            fillOpacity=".08"
            stroke={g.color}
            strokeOpacity=".7"
          />
        ))}
        {digits.map((digit, pos) => {
          const byte = Math.floor(pos / 2);
          const owner = owners[byte] === null ? undefined : byKey.get(owners[byte]!);
          return (
            <rect
              key={pos}
              data-nibble={pos}
              x={RULER + parseInt(digit, 16) * lane + 3}
              y={r(yOf(pos) - 1.5)}
              width={lane - 6}
              height="3"
              fill={owner?.color ?? GEL_COLORS.label}
              fillOpacity={bytes[byte] === '00' ? 0.45 : 0.95}
              opacity={lit && !inLit(byte) ? 0.15 : 1}
            />
          );
        })}
      </svg>
      <ul
        aria-label="Genes"
        data-rows={oneLine ? 'one-line' : 'two-line'}
        className="border-row-rule text-small border-t"
      >
        {genes.map((g) => {
          const on = g.name === lit;
          const reading = (
            <span className={cn('min-w-0', !oneLine && 'col-span-2')}>
              <span className="text-text-dim [overflow-wrap:anywhere]">
                <ShortId value={g.raw} kind="hash" size="tight" /> →
              </span>{' '}
              <span
                className={cn(
                  '[overflow-wrap:anywhere]',
                  quiet(g) ? 'text-text-dim' : 'text-text-bright'
                )}
              >
                <TraitText text={valueOf(g)} />
              </span>
            </span>
          );
          return (
            <li
              key={g.key}
              data-gene-row={g.name}
              className="border-row-rule border-b last:border-b-0"
            >
              <div
                data-active={on ? 'true' : undefined}
                aria-label={`${g.name}, ${bytesOf(g)}, ${g.rule}, ${shortenHash(g.raw, 'tight')} → ${valueOf(g)}`}
                {...bind(g.name)}
                className={cn(
                  '-mx-2 grid cursor-pointer items-baseline gap-x-2.5 px-2 py-2',
                  oneLine
                    ? 'grid-cols-[88px_84px_minmax(0,1fr)]'
                    : 'grid-cols-[minmax(0,1fr)_auto] gap-y-0.5',
                  on && 'bg-base-surface'
                )}
              >
                <span className="min-w-0 [overflow-wrap:anywhere]" style={{ color: g.color }}>
                  {g.name}
                </span>
                {oneLine ? (
                  <span className="text-text-dim text-meta">
                    {bytesOf(g)}
                    <span className="block">{g.rule}</span>
                  </span>
                ) : (
                  <span className="text-text-dim text-meta text-right">
                    {bytesOf(g)} · {g.rule}
                  </span>
                )}
                {reading}
              </div>
            </li>
          );
        })}
      </ul>
    </>
  );
}
