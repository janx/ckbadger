'use client';
// F2 group `objects` (OB-4, OB-C2): the allele gel — the population view of any DOB collection,
// from its own pattern (allelegel.py). One lane per gene in DNA order; one band per expressed value
// (allele), as thick as its share, common at the top; colour values in their own colour; a gene
// every object shares is dimmed. An object's genotype is its band in every lane, joined by a line.
// (The single-object gel that used to live here is now `ui/sequencing-gel.tsx`.)
import { useId } from 'react';
import type { AlleleLane } from '@/lib/dob-genes';
import { GEL_COLORS, GEL_INK } from '@/lib/design-tokens';
import { formatPercent } from '@/lib/ckb-decimal';

const LX = 62;
const LR = 22;
const HEAD = 92;
const GAP = 0.6;

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
  width?: number;
  height?: number;
  overlays?: readonly GenotypeOverlay[];
  activeOverlay?: string | null;
  activeGene?: string | null;
  onGeneHover?: (gene: string | null) => void;
  onOverlayHover?: (key: string | null) => void;
  onBandClick?: (gene: string, value: string) => void;
  selected?: Readonly<Record<string, string>>;
}

interface Placed {
  y: number;
  h: number;
}

export function AlleleGel({
  lanes,
  total,
  subject,
  ink = 'other',
  width = 900,
  height = 380,
  overlays = [],
  activeOverlay = null,
  activeGene = null,
  onGeneHover,
  onOverlayHover,
  onBandClick,
  selected = {},
}: AlleleGelProps) {
  if (!Number.isSafeInteger(total) || total <= 0)
    throw new Error(`${subject}: allele gel needs a positive object count, got ${total}`);
  const inkId = `gel-ink-${useId().replace(/:/g, '')}`;
  const lw = (width - LX - LR) / Math.max(lanes.length, 1);
  const top = HEAD + 14;
  const h = height - HEAD - 14 - 40;
  const placed: Placed[][] = lanes.map((lane) => {
    const spaced = lane.bands.length < h / 2;
    const usable = spaced ? h - GAP * (lane.bands.length - 1) : h;
    let y = top;
    return lane.bands.map((b) => {
      const bh = usable * b.share;
      const p = { y, h: bh };
      y += bh + (spaced ? GAP : 0);
      return p;
    });
  });
  const centre = (lane: number, band: number) => {
    const p = placed[lane][band];
    return { x: LX + lane * lw + lw / 2, y: p.y + p.h / 2 };
  };
  const rowActive = activeOverlay !== null && overlays.some((o) => o.key === activeOverlay);
  const laneOpacity = (gene: string) =>
    activeGene !== null ? (gene === activeGene ? 1 : 0.2) : rowActive ? 0.4 : 1;
  return (
    <div className="min-w-0 overflow-x-auto">
      <svg
        role="img"
        aria-label={`${subject}: allele gel of ${total.toLocaleString('en-US')} objects, one lane per gene (${lanes.map((l) => `${l.gene} ${l.variants} variant${l.variants === 1 ? '' : 's'}`).join(', ')}); each band is one variant, as thick as its share, common at the top.`}
        viewBox={`0 0 ${width} ${height}`}
        width="100%"
        className="block min-w-[560px] overflow-visible"
      >
        <defs>
          <linearGradient id={inkId} gradientUnits="userSpaceOnUse" x1="0" y1="0" x2={width} y2="0">
            <stop offset="0" stopColor={GEL_INK[ink][0]} />
            <stop offset=".45" stopColor={GEL_INK[ink][1]} />
            <stop offset="1" stopColor={GEL_INK[ink][2]} />
          </linearGradient>
        </defs>
        {lanes.map((lane, i) => {
          const x = LX + i * lw;
          const cx = x + lw / 2;
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
              <rect x={r(x + 1)} y="12" width={r(lw - 2)} height={height - 24} fill="transparent" />
              <text
                transform={`translate(${r(cx + 3)} ${HEAD - 6}) rotate(-90)`}
                fontSize="10"
                fill={lane.gene === activeGene ? GEL_COLORS.well : GEL_COLORS.heading}
              >
                {lane.gene.length > 14 ? `${lane.gene.slice(0, 13)}…` : lane.gene}
              </text>
              <rect
                x={r(x + 6)}
                y={HEAD + 2}
                width={r(lw - 12)}
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
                      x={r(x + 7)}
                      y={r(p.y)}
                      width={r(lw - 14)}
                      height={r(bh)}
                      rx={r(Math.min(1.2, bh / 2))}
                      fill={b.colour ?? `url(#${inkId})`}
                      fillOpacity={r(opacity)}
                      stroke={chosen ? GEL_COLORS.overlay : undefined}
                      strokeWidth={chosen ? 1 : undefined}
                      onClick={onBandClick ? () => onBandClick(lane.gene, b.value) : undefined}
                      className={onBandClick ? 'cursor-pointer' : undefined}
                    >
                      <title>{`${lane.gene} = ${b.label}: ${b.count.toLocaleString('en-US')} of ${total.toLocaleString('en-US')} (${formatPercent(BigInt(b.count), BigInt(total))}%)`}</title>
                    </rect>
                    {p.h >= 13 && lw >= 60 && !b.isColour && (
                      <text
                        x={r(cx)}
                        y={r(p.y + p.h / 2 + 3.5)}
                        textAnchor="middle"
                        fontSize="10"
                        pointerEvents="none"
                        fill={lane.conserved ? GEL_COLORS.well : GEL_COLORS.bandText}
                      >
                        {b.label.length > Math.floor((lw - 22) / 6.2)
                          ? `${b.label.slice(0, Math.floor((lw - 22) / 6.2) - 1)}…`
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
        <text x={LX} y={r(top + h + 32)} fontSize="10" fill={GEL_COLORS.label}>
          variants per gene ↑ · one lane per gene, in DNA order · band thickness = share of all{' '}
          {total.toLocaleString('en-US')} objects, common at the top, rare below
        </text>
        {overlays.map((o) => {
          if (o.points.length === 0) return null;
          const pts = o.points.map((p) => centre(p.lane, p.band));
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
                  x={r(p.x - lw / 2 + 7)}
                  y={r(p.y - 1.6)}
                  width={r(lw - 14)}
                  height="3.2"
                  rx="1.2"
                  fill="none"
                  stroke={GEL_COLORS.overlay}
                  strokeWidth="1"
                />
              ))}
              <text
                x={r(pts[0].x - lw / 2 + 2)}
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
      </svg>
    </div>
  );
}
