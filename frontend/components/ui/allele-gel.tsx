'use client';
// F2 group `objects` (OB-4, OB-C2): the allele gel — the population view of any DOB collection,
// from its own pattern (allelegel.py). One lane per gene in DNA order; one band per expressed value
// (allele), as thick as its share, common at the top; colour values in their own colour; a gene
// every object shares is dimmed. An object's genotype is its band in every lane, joined by a line.
// (The single-object gel that used to live here is now `ui/sequencing-gel.tsx`.)
//
// Responsive (R3, V2): the gel is laid out for its measured box and drawn 1:1. Lanes keep a 12 px
// floor and wrap to further rows in DNA order (`lib/allele-gel-layout.ts`); more than 32 genes
// below md become a ranked gene list that keeps every variant one tap away. In the gel a hovered
// band reads in the line under it, and a tap pins it until a second tap or Escape (R6, board V2).
import { useId, useState, type ReactNode } from 'react';
import type { AlleleLane } from '@/lib/dob-genes';
import { GEL_COLORS, GEL_INK } from '@/lib/design-tokens';
import { formatPercent } from '@/lib/ckb-decimal';
import { formatCount } from '@/lib/format-count';
import {
  ALLELE_GEL,
  alleleGelLayout,
  geneCaption,
  type AlleleGelLayout,
} from '@/lib/allele-gel-layout';
import { useElementWidth } from '@/hooks/useElementWidth';
import { usePinnedReadout } from '@/hooks/usePinnedReadout';
import { ReadoutRow } from '@/components/ui/readout-row';
import { tapOrHover } from '@/components/ui/tap-or-hover-hint';
import { cn } from '@/lib/utils';

const GAP = 0.6;
const ROW_GAP = 18;
const MIN_ROW_BAND = 120;
/** The ranked list shows this many genes before its "more genes" control. */
const LIST_FIRST = 10;

const r = (v: number) => Math.round(v * 100) / 100;

export interface GenotypeOverlay {
  key: string;
  label: string;
  points: readonly { lane: number; band: number }[];
  /** Always shown ("this one" on the Object page); otherwise only while active. */
  persistent?: boolean;
}

export interface AlleleGelProps {
  lanes: readonly AlleleLane[];
  /** Objects counted (the collection's decoded DOB items). */
  total: number;
  subject: string;
  ink?: keyof typeof GEL_INK;
  /** Height of the band area in one row; wrapped rows share it, down to 120 px each. */
  bandHeight?: number;
  overlays?: readonly GenotypeOverlay[];
  activeOverlay?: string | null;
  activeGene?: string | null;
  onGeneHover?: (gene: string | null) => void;
  onOverlayHover?: (key: string | null) => void;
  onBandClick?: (gene: string, value: string) => void;
  selected?: Readonly<Record<string, string>>;
  /**
   * Consistency round C9: how a lane is named for people (a known DOB/1 key as a word); default
   * the gene's pattern name.
   */
  labelOf?: (gene: string) => string;
  /**
   * Consistency round C10: what the readout says while nothing is hovered or pinned; default
   * "Tap or hover a band.". A page whose readout row already carries the section's one hint
   * passes words without it.
   */
  idle?: ReactNode;
}

interface Placed {
  y: number;
  h: number;
}

/** One band: a gene's variant. */
interface Variant {
  gene: string;
  value: string;
}
const variantKey = (v: Variant) => `${v.gene}\u0000${v.value}`;

export function AlleleGel(props: AlleleGelProps) {
  const { lanes, total, subject, overlays = [] } = props;
  if (!Number.isSafeInteger(total) || total <= 0)
    throw new Error(`${subject}: allele gel needs a positive object count, got ${total}`);
  const [box, measured] = useElementWidth<HTMLDivElement>();
  const width = measured === null ? null : Math.floor(measured);
  const layout =
    width === null
      ? null
      : alleleGelLayout({
          genes: lanes.map((lane) => (props.labelOf ? props.labelOf(lane.gene) : lane.gene)),
          width,
          subject,
          overlayLabels: overlays.map((o) => o.label),
        });
  return (
    <div ref={box} className="min-w-0">
      {layout?.form === 'list' && <GeneList {...props} />}
      {layout?.form === 'gel' && width !== null && <Gel {...props} width={width} layout={layout} />}
    </div>
  );
}

function Gel({
  lanes,
  total,
  subject,
  ink = 'other',
  bandHeight = 250,
  overlays = [],
  activeOverlay = null,
  activeGene = null,
  onGeneHover,
  onOverlayHover,
  onBandClick,
  selected = {},
  labelOf = (gene: string) => gene,
  idle,
  width,
  layout,
}: AlleleGelProps & { width: number; layout: Extract<AlleleGelLayout, { form: 'gel' }> }) {
  const inkId = `gel-ink-${useId().replace(/:/g, '')}`;
  const readout = usePinnedReadout<Variant>(variantKey);
  const read = readout.active;
  const readLane = read === null ? undefined : lanes.find((l) => l.gene === read.gene);
  const readBand = readLane?.bands.find((b) => b.value === read?.value);
  // A reading names a band on the gel: one the lanes no longer hold lets go.
  if (read !== null && !readBand) readout.clear();
  const { rows, laneWidth: lw, gutter: LX, head, captions } = layout;
  // Wrapped rows share the band height, down to 120 px a row (a share still reads at that height).
  const h =
    rows.length === 1 ? bandHeight : Math.max(MIN_ROW_BAND, Math.round(bandHeight / rows.length));
  /** Inside a lane, bands keep this much air on each side; a narrow lane keeps 2 px. */
  const inset = lw >= 28 ? 7 : 2;
  const rowH = head + 14 + h + 24;
  const height = rows.length * rowH + (rows.length - 1) * ROW_GAP;
  const slot = new Map<number, { row: number; k: number }>();
  rows.forEach((row, ri) => row.forEach((lane, k) => slot.set(lane, { row: ri, k })));
  const rowTop = (ri: number) => ri * (rowH + ROW_GAP);
  const laneX = (lane: number) => LX + slot.get(lane)!.k * lw;
  const bandsTop = (lane: number) => rowTop(slot.get(lane)!.row) + head + 14;
  /** A drawn width: never zero or negative (a lane under the floor is a layout bug). */
  const span = (w: number, what: string) => {
    if (!(w > 0)) throw new Error(`${subject}: allele gel ${what} width ${w} (lane ${lw} px)`);
    return r(w);
  };
  const placed: Placed[][] = lanes.map((lane, i) => {
    const spaced = lane.bands.length < h / 2;
    const usable = spaced ? h - GAP * (lane.bands.length - 1) : h;
    let y = bandsTop(i);
    return lane.bands.map((b) => {
      const bh = usable * b.share;
      const p = { y, h: bh };
      y += bh + (spaced ? GAP : 0);
      return p;
    });
  });
  const centre = (lane: number, band: number) => {
    const p = placed[lane][band];
    return { x: laneX(lane) + lw / 2, y: p.y + p.h / 2 };
  };
  const rowActive = activeOverlay !== null && overlays.some((o) => o.key === activeOverlay);
  const laneOpacity = (gene: string) =>
    activeGene !== null ? (gene === activeGene ? 1 : 0.2) : rowActive ? 0.4 : 1;
  const labelChars = Math.floor((lw - 2 * inset - 8) / ALLELE_GEL.charWidth);
  return (
    <>
      <svg
        role="img"
        aria-label={`${subject}: allele gel of ${total.toLocaleString('en-US')} objects, one lane per gene (${lanes.map((l) => `${l.gene} ${l.variants} variant${l.variants === 1 ? '' : 's'}`).join(', ')}); each band is one variant, as thick as its share, common at the top.`}
        viewBox={`0 0 ${width} ${height}`}
        width={width}
        height={height}
        className="block overflow-visible"
      >
        <defs>
          <linearGradient id={inkId} gradientUnits="userSpaceOnUse" x1="0" y1="0" x2={width} y2="0">
            <stop offset="0" stopColor={GEL_INK[ink][0]} />
            <stop offset=".45" stopColor={GEL_INK[ink][1]} />
            <stop offset="1" stopColor={GEL_INK[ink][2]} />
          </linearGradient>
        </defs>
        {rows.map((row, ri) => (
          <g key={ri} data-gel-row={ri}>
            {row.map((i) => {
              const lane = lanes[i];
              const x = laneX(i);
              const cx = x + lw / 2;
              const top = bandsTop(i);
              const max = lane.bands[0]?.count ?? 1;
              return (
                <g
                  key={lane.gene}
                  data-lane={lane.gene}
                  opacity={laneOpacity(lane.gene)}
                  onMouseEnter={() => onGeneHover?.(lane.gene)}
                  onMouseLeave={() => onGeneHover?.(null)}
                  className="transition-opacity"
                >
                  <title>{labelOf(lane.gene)}</title>
                  <rect
                    x={r(x + 1)}
                    y={rowTop(ri)}
                    width={span(lw - 2, 'lane')}
                    height={rowH}
                    fill="transparent"
                  />
                  <text
                    data-gene-caption
                    {...(captions === 'across'
                      ? { x: r(cx), y: rowTop(ri) + 12, textAnchor: 'middle' }
                      : {
                          transform: `translate(${r(cx + 3)} ${rowTop(ri) + head - 6}) rotate(-90)`,
                        })}
                    fontSize="10"
                    fill={lane.gene === activeGene ? GEL_COLORS.well : GEL_COLORS.heading}
                  >
                    {geneCaption(labelOf(lane.gene))}
                  </text>
                  <rect
                    x={r(x + inset - 1)}
                    y={rowTop(ri) + head + 2}
                    width={span(lw - 2 * (inset - 1), 'well')}
                    height="5"
                    rx="1.2"
                    fill="none"
                    stroke={GEL_COLORS.well}
                    strokeOpacity=".35"
                    strokeWidth=".6"
                  />
                  {lane.bands.map((b, j) => {
                    const p = placed[i][j];
                    const opacity = lane.conserved
                      ? 0.22
                      : b.isColour
                        ? 0.9
                        : 0.35 + (0.65 * b.count) / max;
                    const chosen = selected[lane.gene] === b.value;
                    const bh = Math.max(p.h, 0.35);
                    return (
                      <g key={`${b.value}:${j}`}>
                        <rect
                          data-allele={b.value}
                          x={r(x + inset)}
                          y={r(p.y)}
                          width={span(lw - 2 * inset, 'band')}
                          height={r(bh)}
                          rx={r(Math.min(1.2, bh / 2))}
                          fill={b.colour ?? `url(#${inkId})`}
                          fillOpacity={r(opacity)}
                          stroke={chosen ? GEL_COLORS.overlay : undefined}
                          strokeWidth={chosen ? 1 : undefined}
                          onMouseEnter={() => readout.hover({ gene: lane.gene, value: b.value })}
                          onMouseLeave={() => readout.hover(null)}
                          onClick={() => {
                            readout.toggle({ gene: lane.gene, value: b.value });
                            onBandClick?.(lane.gene, b.value);
                          }}
                          className="cursor-pointer"
                        >
                          <title>{`${labelOf(lane.gene)} = ${b.label}: ${b.count.toLocaleString('en-US')} of ${total.toLocaleString('en-US')} (${formatPercent(BigInt(b.count), BigInt(total))}%)`}</title>
                        </rect>
                        {p.h >= 13 && lw >= ALLELE_GEL.labelledLane && !b.isColour && (
                          <text
                            x={r(cx)}
                            y={r(p.y + p.h / 2 + 3.5)}
                            textAnchor="middle"
                            fontSize="10"
                            pointerEvents="none"
                            fill={lane.conserved ? GEL_COLORS.well : GEL_COLORS.bandText}
                          >
                            {b.label.length > labelChars
                              ? `${b.label.slice(0, labelChars - 1)}…`
                              : b.label}
                          </text>
                        )}
                      </g>
                    );
                  })}
                  <text
                    data-variants
                    x={r(cx)}
                    y={r(top + h + 16)}
                    textAnchor="middle"
                    fontSize="10"
                    fill={GEL_COLORS.label}
                  >
                    {lane.variants.toLocaleString('en-US')}
                  </text>
                </g>
              );
            })}
          </g>
        ))}
        {overlays.map((o) => {
          if (o.points.length === 0) return null;
          const shown = o.persistent || o.key === activeOverlay;
          return (
            <g
              key={o.key}
              data-overlay={o.key}
              opacity={shown ? 1 : 0}
              pointerEvents={shown ? undefined : 'none'}
              onMouseEnter={() => onOverlayHover?.(o.key)}
              onMouseLeave={() => onOverlayHover?.(null)}
              className="transition-opacity"
            >
              {rows.map((row, ri) => {
                const pts = o.points
                  .filter((p) => slot.get(p.lane)!.row === ri)
                  .map((p) => centre(p.lane, p.band));
                if (pts.length === 0) return null;
                return (
                  <g key={ri}>
                    <polyline
                      points={pts.map((p) => `${r(p.x)},${r(p.y)}`).join(' ')}
                      fill="none"
                      stroke={GEL_COLORS.overlay}
                      strokeOpacity=".7"
                      strokeWidth=".8"
                    />
                    {pts.map((p, k) => (
                      <rect
                        key={k}
                        data-tick
                        x={r(p.x - lw / 2 + inset)}
                        y={r(p.y - 1.6)}
                        width={span(lw - 2 * inset, 'genotype tick')}
                        height="3.2"
                        rx="1.2"
                        fill="none"
                        stroke={GEL_COLORS.overlay}
                        strokeWidth="1"
                      />
                    ))}
                    <text
                      x={LX + 2}
                      y={r(pts[0].y + 3.5)}
                      textAnchor="end"
                      fontSize="10"
                      fill={GEL_COLORS.overlay}
                    >
                      {o.label}
                    </text>
                  </g>
                );
              })}
            </g>
          );
        })}
      </svg>
      <div className="mt-2">
        <ReadoutRow
          label="Variant readout"
          pinned={readout.showingPin}
          idle={idle ?? tapOrHover('a band.', { capital: true })}
        >
          {readLane && readBand
            ? `${labelOf(readLane.gene)} · ${readBand.label} · ${readBand.count.toLocaleString('en-US')} of ${total.toLocaleString('en-US')} objects`
            : null}
        </ReadoutRow>
      </div>
      {/* Visual pass: as SVG text this line was clipped once a gel had 16 lanes; it wraps here. */}
      <p className="text-text-dim text-meta mt-1">
        variants per gene ↑ · one lane per gene, in DNA order · band thickness = share of all{' '}
        {total.toLocaleString('en-US')} objects, common at the top, rare below
      </p>
    </>
  );
}

/** "62 more genes: 61 with 2 variants and 1 with 1 variant". */
function moreGenes(rest: readonly AlleleLane[]): string {
  const byVariants = new Map<number, number>();
  for (const lane of rest) byVariants.set(lane.variants, (byVariants.get(lane.variants) ?? 0) + 1);
  const parts = [...byVariants]
    .sort(([a], [b]) => b - a)
    .map(([v, n]) => `${n.toLocaleString('en-US')} with ${formatCount(v, 'variant')}`);
  const tail = parts.length > 1 ? `${parts.slice(0, -1).join(', ')} and ${parts.at(-1)}` : parts[0];
  return `${formatCount(rest.length, 'more gene', 'more genes')}: ${tail}`;
}

const LIST_GRID = 'grid grid-cols-[minmax(0,1fr)_64px_118px] items-start gap-x-2.5';

/**
 * The gel as a ranked gene list (R3: more than 32 genes below md): gene · variants · most common,
 * the gene with the most variants first. Opening a gene lists every variant with its count (a
 * button when the page filters by variant); a genotype shown on the gel is written per gene.
 */
function GeneList({
  lanes,
  total,
  overlays = [],
  activeOverlay = null,
  activeGene = null,
  onGeneHover,
  onBandClick,
  selected = {},
}: AlleleGelProps) {
  const [open, setOpen] = useState<string | null>(null);
  const [all, setAll] = useState(false);
  const ranked = lanes
    .map((lane, index) => ({ lane, index }))
    .sort((a, b) => b.lane.variants - a.lane.variants || a.index - b.index);
  const shown = all ? ranked : ranked.slice(0, LIST_FIRST);
  const rest = ranked.slice(LIST_FIRST).map(({ lane }) => lane);
  const marking = overlays.filter((o) => o.persistent || o.key === activeOverlay);
  const of = (count: number) =>
    `${count.toLocaleString('en-US')} of ${total.toLocaleString('en-US')}`;
  return (
    <div className="min-w-0">
      <div
        role="table"
        aria-label="Genes ranked by how many variants they have"
        className="text-small min-w-0"
      >
        <div
          role="row"
          className={cn(LIST_GRID, 'border-base-border text-text-dim text-meta border-b pb-1.5')}
        >
          <span role="columnheader">gene</span>
          <span role="columnheader" className="text-right">
            variants
          </span>
          <span role="columnheader" className="text-right">
            most common
          </span>
        </div>
        {shown.map(({ lane, index }) => {
          const top = lane.bands[0];
          const expanded = open === lane.gene;
          const marks = marking.flatMap((o) => {
            const point = o.points.find((p) => p.lane === index);
            return point ? [{ key: o.key, label: o.label, band: lane.bands[point.band] }] : [];
          });
          return (
            <div
              key={lane.gene}
              role="row"
              data-gene-row={lane.gene}
              data-active={lane.gene === activeGene ? 'true' : undefined}
              onMouseEnter={() => onGeneHover?.(lane.gene)}
              onMouseLeave={() => onGeneHover?.(null)}
              className={cn(
                LIST_GRID,
                'border-base-elevated border-b py-1.5',
                lane.gene === activeGene && 'bg-base-surface'
              )}
            >
              <span role="cell" className="min-w-0">
                <button
                  type="button"
                  aria-expanded={expanded}
                  aria-label={`${lane.gene}, ${formatCount(lane.variants, 'variant')}`}
                  onClick={() => setOpen(expanded ? null : lane.gene)}
                  className="text-text-bright min-h-6 text-left [overflow-wrap:anywhere]"
                >
                  {lane.gene}
                </button>
                {marks.map((m) => (
                  <span key={m.key} className="text-text-dim text-meta block">
                    ▸ {m.label}: {m.band.label} · {of(m.band.count)}
                  </span>
                ))}
                {expanded && (
                  <ul aria-label={`${lane.gene} variants`} className="text-meta mt-1 space-y-0.5">
                    {lane.bands.map((b) => (
                      <li key={b.value} className="[overflow-wrap:anywhere]">
                        {onBandClick ? (
                          <button
                            type="button"
                            aria-pressed={selected[lane.gene] === b.value}
                            onClick={() => onBandClick(lane.gene, b.value)}
                            className={cn(
                              'min-h-6 text-left',
                              selected[lane.gene] === b.value ? 'text-text-bright' : 'text-aqua'
                            )}
                          >
                            {b.label} · {of(b.count)}
                          </button>
                        ) : (
                          `${b.label} · ${of(b.count)}`
                        )}
                      </li>
                    ))}
                  </ul>
                )}
              </span>
              <span role="cell" className="text-right tabular-nums">
                {lane.variants.toLocaleString('en-US')}
              </span>
              <span role="cell" className="flex items-center justify-end gap-2">
                <span aria-hidden="true" className="bg-base-elevated flex h-1 w-14 rounded-sm">
                  <span
                    className="bg-jade rounded-sm"
                    style={{ width: `${(top.count / total) * 100}%` }}
                  />
                </span>
                <span className="text-text whitespace-nowrap tabular-nums">{of(top.count)}</span>
              </span>
            </div>
          );
        })}
      </div>
      {!all && rest.length > 0 && (
        <button
          type="button"
          onClick={() => setAll(true)}
          className="text-aqua text-small min-h-11 text-left"
        >
          {moreGenes(rest)} ▾
        </button>
      )}
      <p className="text-text-dim text-meta mt-1">
        genes ranked by how many variants they have · most common = how many of all{' '}
        {total.toLocaleString('en-US')} objects carry the commonest variant · open a gene for every
        variant
      </p>
    </div>
  );
}

/**
 * Says which decoder outputs are not drawn because the pattern does not declare them as genes
 * (a DOB/1 renderer's IMAGE), so the gene count matches the pattern and nothing goes unmentioned.
 */
export function NotGenesNote({ traits }: { traits: readonly string[] }) {
  if (traits.length === 0) return null;
  const names =
    traits.length > 1 ? `${traits.slice(0, -1).join(', ')} and ${traits.at(-1)}` : traits[0];
  return (
    <p className="text-text-dim text-meta [overflow-wrap:anywhere]">
      Not drawn: {names}, {traits.length > 1 ? 'outputs' : 'an output'} of the decoder that the
      pattern does not declare as {traits.length > 1 ? 'genes' : 'a gene'}.
    </p>
  );
}
