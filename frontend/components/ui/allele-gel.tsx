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
import { GEL_COLORS, GEL_INK, GEL_INK_KEYS } from '@/lib/design-tokens';
import {
  color,
  hexToRgb,
  inkOnColours,
  overGround,
  type PaletteKey,
  type ThemeName,
} from '@/lib/palette';
import { useTheme } from '@/hooks/useTheme';
import { formatCount, countNoun } from '@/lib/format-count';
import { unitText } from '@/components/ui/units';
import {
  ALLELE_GEL,
  alleleGelLayout,
  geneCaption,
  type AlleleGelLayout,
} from '@/lib/allele-gel-layout';
import { useElementWidth } from '@/hooks/useElementWidth';
import { usePinnedReadout } from '@/hooks/usePinnedReadout';
import { ReadoutRow } from '@/components/ui/readout-row';
import { TraitText } from '@/components/asset/trait-text';
import { tapOrHover } from '@/components/ui/tap-or-hover-hint';
import { cn } from '@/lib/utils';
import { clipForeignLabel } from '@/lib/shorten';
import { percentOf } from '@/lib/format-unit';
import { SVG_TEXT, centreShift, lineAscentPx } from '@/lib/chart-type';
import { descentPx, foreignTextPx, textPx, tightLinePx } from '@/lib/type-metrics';

const GAP = 0.6;
/** How far outside its band the "this one" outline is drawn (PB-04). */
const OUTLINE_GAP = 1.5;
const ROW_GAP = 18;
const MIN_ROW_BAND = 120;
/** A band's name, a variant count and a tag are tick lines (readability plan 2026-11-18, lane F3). */
const TICK_LINE = tightLinePx(SVG_TEXT.tick);
/** The count of a lane's variants stands 4 px under its bands, and the row ends 5 px under it. */
const COUNT_GAP = 4;
const COUNT_FOOT = 5;
/** The ranked list shows this many genes before its "more genes" control. */
const LIST_FIRST = 10;

const r = (v: number) => Math.round(v * 100) / 100;
/** Where the plate-wide band gradient sets its stops, as fractions of the gel's width. */
const INK_STOPS = [0, 0.45, 1] as const;

/** `b` at `t` of the way from `a` (two hexes, sRGB, as an SVG gradient interpolates). */
function lerpHex(a: string, b: string, t: number): string {
  const [x, y] = [hexToRgb(a), hexToRgb(b)];
  const c = (p: number, q: number) =>
    Math.round(p + (q - p) * t)
      .toString(16)
      .padStart(2, '0');
  return `#${c(x.r, y.r)}${c(x.g, y.g)}${c(x.b, y.b)}`;
}

/**
 * The ink of a label on a band (words on a fill, `inkOnFill`'s rule): the band is the plate-wide
 * gradient of `stops` at `alpha` over `ground`, so the label, from `x0` to `x1` on a gel `width`
 * wide, is read on each colour under it (its two ends and any stop between) and takes the fill ink
 * that reads strongest on all of them.
 */
function bandLabelInk(
  theme: ThemeName,
  stops: readonly PaletteKey[],
  alpha: number,
  ground: PaletteKey,
  [x0, x1]: readonly [number, number],
  width: number
): string {
  const at = (x: number) => {
    const t = Math.min(1, Math.max(0, x / width));
    const i = t <= INK_STOPS[1] ? 0 : 1;
    const u = (t - INK_STOPS[i]) / (INK_STOPS[i + 1] - INK_STOPS[i]);
    return lerpHex(
      overGround(theme, stops[i], alpha, ground),
      overGround(theme, stops[i + 1], alpha, ground),
      u
    );
  };
  const xs = [x0, x1, ...INK_STOPS.map((s) => s * width).filter((x) => x > x0 && x < x1)];
  return color(inkOnColours(theme, xs.map(at)).ink);
}

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
  /**
   * The colour the gel is drawn on (its plate's fill, else the page ground): a band's label takes
   * the ink the band's fill calls for over it.
   */
  ground?: PaletteKey;
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
          // PB-04: only a genotype always shown ("this one") is tagged in the margin; a hovered one
          // (a cluster page's row) is named by its own lit row, so the lanes keep the width.
          overlayLabels: overlays.filter((o) => o.persistent).map((o) => o.label),
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
  ground = 'base-bg',
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
  const { resolved: theme } = useTheme();
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
  const rowH = head + 14 + h + COUNT_GAP + TICK_LINE + COUNT_FOOT;
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
  /** A band's box, as its rect is drawn. */
  const bandBox = (lane: number, band: number) => {
    const p = placed[lane][band];
    return { x: laneX(lane) + inset, y: p.y, w: lw - 2 * inset, h: Math.max(p.h, 0.35) };
  };
  const rowActive = activeOverlay !== null && overlays.some((o) => o.key === activeOverlay);
  const laneOpacity = (gene: string) =>
    activeGene !== null ? (gene === activeGene ? 1 : 0.2) : rowActive ? 0.4 : 1;
  // A band's label (a trait value, the collection creator's text) stands 4 px clear of its
  // band's sides, cut to that room at whole graphemes by the bound of lib/type-metrics.ts
  // (review F-B, logic finding 5).
  const labelRoom = lw - 2 * inset - 8;
  // One form of the lanes' counts a gel (lane P2's fifth round, NOTE 6): with the noun where every
  // lane holds its count's words between its insets, else every count bare, which the legend reads
  const countsWithNoun = lanes.every(
    (lane) => textPx(formatCount(lane.variants, 'variant').length, SVG_TEXT.tick) <= lw - 2 * inset
  );
  return (
    <>
      <svg
        role="img"
        aria-label={`${subject}: allele gel of ${formatCount(total, 'object')}, one lane per gene (${lanes.map((l) => `${l.gene} ${l.variants} ${countNoun(l.variants, 'variant')}`).join(', ')}); each band is one variant, as thick as its share, common at the top.`}
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
                      ? { x: r(cx), y: rowTop(ri) + ALLELE_GEL.captionY, textAnchor: 'middle' }
                      : {
                          // turned on end, its line centred across the lane
                          transform: `translate(${r(cx + (TICK_LINE - 2 * descentPx(SVG_TEXT.tick)) / 2)} ${rowTop(ri) + head - 6}) rotate(-90)`,
                        })}
                    fontSize={SVG_TEXT.tick}
                    fill={lane.gene === activeGene ? GEL_COLORS.well : GEL_COLORS.heading}
                  >
                    {geneCaption(labelOf(lane.gene))}
                  </text>
                  <rect
                    x={r(x + inset - 1)}
                    y={rowTop(ri) + head + 2}
                    width={span(lw - 2 * (inset - 1), 'well')}
                    height="5"
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
                          <title>{`${labelOf(lane.gene)} = ${b.label}: ${b.count.toLocaleString('en-US')} of ${total.toLocaleString('en-US')} (${percentOf(BigInt(b.count), BigInt(total))})`}</title>
                        </rect>
                        {p.h > TICK_LINE &&
                          lw >= ALLELE_GEL.labelledLane &&
                          !b.isColour &&
                          (() => {
                            const label = clipForeignLabel(b.label, SVG_TEXT.tick, labelRoom);
                            // the band's share of the objects under its value (lane P2's third
                            // round, item 5: it was in the band's hover title only), where the band
                            // holds two lines and the lane the figure
                            const share = percentOf(BigInt(b.count), BigInt(total));
                            const lines =
                              p.h > 2 * TICK_LINE &&
                              foreignTextPx(share, SVG_TEXT.tick) <= labelRoom
                                ? [label, share]
                                : [label];
                            const firstTop = p.y + (p.h - lines.length * TICK_LINE) / 2;
                            return lines.map((line, k) => {
                              // words on a fill: the ink the band calls for under the line's run
                              const half = foreignTextPx(line, SVG_TEXT.tick) / 2;
                              const fill = bandLabelInk(
                                theme,
                                GEL_INK_KEYS[ink],
                                r(opacity),
                                ground,
                                [r(cx) - half, r(cx) + half],
                                width
                              );
                              return (
                                <text
                                  key={k}
                                  x={r(cx)}
                                  y={r(firstTop + k * TICK_LINE + lineAscentPx(SVG_TEXT.tick))}
                                  textAnchor="middle"
                                  fontSize={SVG_TEXT.tick}
                                  pointerEvents="none"
                                  fill={fill}
                                >
                                  {line}
                                </text>
                              );
                            });
                          })()}
                      </g>
                    );
                  })}
                  <text
                    data-variants
                    x={r(cx)}
                    y={r(top + h + COUNT_GAP + lineAscentPx(SVG_TEXT.tick))}
                    textAnchor="middle"
                    fontSize={SVG_TEXT.tick}
                    fill={GEL_COLORS.label}
                  >
                    {/* the count with its noun where every lane holds it (lane P2's third round,
                        item 5: a bare "3", which only the legend explained), one form a gel
                        (the fifth round, NOTE 6: "3 variants · 12" in one row) */}
                    {countsWithNoun
                      ? formatCount(lane.variants, 'variant')
                      : lane.variants.toLocaleString('en-US')}
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
                  .sort((a, b) => slot.get(a.lane)!.k - slot.get(b.lane)!.k);
                if (pts.length === 0) return null;
                const boxes = pts.map((p) => bandBox(p.lane, p.band));
                return (
                  <g key={ri}>
                    {/* PB-04: the genotype joins its bands from side to side, between the lanes
                        (never across a band, nor across a lane where it has no counted band). */}
                    {pts.slice(1).map((p, k) => {
                      if (slot.get(p.lane)!.k !== slot.get(pts[k].lane)!.k + 1) return null;
                      const [a, b] = [boxes[k], boxes[k + 1]];
                      return (
                        <line
                          key={`link:${k}`}
                          data-link
                          x1={r(a.x + a.w + OUTLINE_GAP)}
                          y1={r(a.y + a.h / 2)}
                          x2={r(b.x - OUTLINE_GAP)}
                          y2={r(b.y + b.h / 2)}
                          stroke={GEL_COLORS.overlay}
                          strokeOpacity=".7"
                          strokeWidth=".8"
                        />
                      );
                    })}
                    {/* PB-04: its own band is outlined just outside its edges, clear of its name. */}
                    {boxes.map((box, k) => (
                      <rect
                        key={k}
                        data-tick
                        x={r(box.x - OUTLINE_GAP)}
                        y={r(box.y - OUTLINE_GAP)}
                        width={span(box.w + 2 * OUTLINE_GAP, 'genotype outline')}
                        height={r(box.h + 2 * OUTLINE_GAP)}
                        fill="none"
                        stroke={GEL_COLORS.overlay}
                        strokeWidth="1"
                      />
                    ))}
                    {o.persistent && (
                      <text
                        x={LX - 4}
                        y={r(boxes[0].y + boxes[0].h / 2 + centreShift(SVG_TEXT.tick))}
                        textAnchor="end"
                        fontSize={SVG_TEXT.tick}
                        fill={GEL_COLORS.overlay}
                      >
                        {o.label}
                      </text>
                    )}
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
          {readLane && readBand ? (
            <TraitText
              text={`${labelOf(readLane.gene)} · ${readBand.label} · ${readBand.count.toLocaleString('en-US')} of ${formatCount(total, 'object')}`}
            />
          ) : null}
        </ReadoutRow>
      </div>
      {/* Visual pass: as SVG text this line was clipped once a gel had 16 lanes; it wraps here. */}
      <p className="text-text-dim text-meta max-w-measure mt-1">
        variants per gene ↑ · one lane per gene, in DNA order · band thickness = share of all{' '}
        {unitText(formatCount(total, 'object'))}, common at the top, rare below
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
          className={cn(LIST_GRID, 'border-row-rule text-text-dim text-meta border-b pb-1.5')}
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
                'border-row-rule border-b py-1.5',
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
                            <TraitText text={b.label} /> · {of(b.count)}
                          </button>
                        ) : (
                          <TraitText text={`${b.label} · ${of(b.count)}`} />
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
                <span aria-hidden="true" className="bg-bar-track flex h-1 w-14">
                  <span
                    className="bg-jade scan-fill"
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
      <p className="text-text-dim text-meta max-w-measure mt-1">
        genes ranked by how many variants they have · most common = how many of all{' '}
        {unitText(formatCount(total, 'object'))} carry the commonest variant · open a gene for every
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
    // a sentence: the reading ink at 13 px (lane cd-fixes-controls, the ink and size round)
    <p className="text-text text-small max-w-measure [overflow-wrap:anywhere]">
      Not drawn: {names}, {traits.length > 1 ? 'outputs' : 'an output'} of the decoder that the
      pattern does not declare as {traits.length > 1 ? 'genes' : 'a gene'}.
    </p>
  );
}
