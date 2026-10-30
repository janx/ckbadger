'use client';
// F2 group `objects` (OB-7): the Object board's Sanger-style sequencing gel. 16 lanes, one per hex
// digit; the DNA's hex digits run top to bottom, each a band in the lane of its value, so reading
// the bands spells the DNA. Bands take the colour of the gene that owns their byte; each gene has
// a bracket and its reading (bytes · rule, raw → trait) to the right, level with its bands.
import type { SequencingLayout } from '@/lib/dob-genes';
import { GEL_COLORS } from '@/lib/design-tokens';

const W = 900;
const LX = 64;
const LANE = 15;
const TOP = 44;
const STEP = 3.3;
const GX = LX + 16 * LANE + 18;
const TX = GX + 22;
const LABEL_GAP = 15;

const r = (v: number) => Math.round(v * 10) / 10;
const yOf = (pos: number) => TOP + pos * STEP;

function shortRaw(raw: string): string {
  return raw.length <= 12 ? raw : `${raw.slice(0, 8)}…${raw.slice(-2)}`;
}
function clip(text: string, max: number): string {
  return text.length <= max ? text : `${text.slice(0, max - 1)}…`;
}

export interface SequencingGelProps {
  layout: SequencingLayout;
  subject: string;
  /** The gene (by name) lit by any of the three linked views; null when none is. */
  active: string | null;
  onActive: (gene: string | null) => void;
}

export function SequencingGel({ layout, subject, active, onActive }: SequencingGelProps) {
  const { bytes, genes, owners } = layout;
  const digits = bytes.join('').split('');
  const n = digits.length;
  const height = TOP + n * STEP + 40;
  const byKey = new Map(genes.map((g) => [g.key, g]));
  const inActive = (byte: number) =>
    genes.some((g) => g.name === active && byte >= g.offset && byte < g.offset + g.length);
  // Labels sit level with their genes, pushed down just enough not to overlap (DNA order).
  let floor = -Infinity;
  const labelY = new Map<string, number>();
  for (const g of genes) {
    const mid = (yOf(g.offset * 2) + yOf((g.offset + g.length) * 2 - 1)) / 2 + 4;
    const y = Math.max(mid, floor + LABEL_GAP);
    labelY.set(g.key, y);
    floor = y;
  }
  const labelsEnd = Math.max(height, floor + 24);
  const describe = genes
    .map(
      (g) =>
        `${g.name} ${g.length === 1 ? `byte ${g.offset}` : `bytes ${g.offset} to ${g.offset + g.length - 1}`}${g.value ? `, ${g.value}` : ''}`
    )
    .join('; ');
  const set = (name: string | null) => onActive(name);
  return (
    <div className="min-w-0 overflow-x-auto">
      <svg
        role="img"
        aria-label={`${subject}: the ${bytes.length}-byte DNA as a sequencing gel: 16 lanes for hex digits 0 to f, its ${n} hex digits as bands from top to bottom. Genes: ${describe}.`}
        viewBox={`0 0 ${W} ${r(labelsEnd)}`}
        width="100%"
        className="block min-w-[640px] overflow-visible"
      >
        {Array.from({ length: 16 }, (_, d) => {
          const x = LX + d * LANE;
          return (
            <g key={d} aria-hidden>
              <text
                x={r(x + LANE / 2)}
                y="16"
                textAnchor="middle"
                fontSize="10"
                fill={GEL_COLORS.label}
              >
                {d.toString(16)}
              </text>
              <rect
                x={r(x + 2)}
                y="24"
                width={LANE - 4}
                height="4"
                rx="1"
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
              x1={LX - 10}
              x2={LX - 4}
              y1={r(yOf(b * 2))}
              y2={r(yOf(b * 2))}
              stroke={GEL_COLORS.ruler}
            />
            <text
              x={LX - 14}
              y={r(yOf(b * 2) + 3.5)}
              textAnchor="end"
              fontSize="10"
              fill={GEL_COLORS.rulerText}
            >
              {b}
            </text>
          </g>
        ))}
        <text
          data-ruler={bytes.length}
          x={LX - 14}
          y={r(yOf(n) + 4)}
          textAnchor="end"
          fontSize="10"
          fill={GEL_COLORS.rulerText}
        >
          {bytes.length}
        </text>
        <text
          x={LX - 14}
          y={r(yOf(n) + 18)}
          textAnchor="end"
          fontSize="10"
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
              rx=".6"
              fill={owner?.color ?? GEL_COLORS.label}
              fillOpacity={bytes[byte] === '00' ? 0.45 : 0.95}
              opacity={active && !inActive(byte) ? 0.15 : 1}
            />
          );
        })}
        {genes.map((g) => {
          const y1 = yOf(g.offset * 2) - 1.6;
          const y2 = yOf((g.offset + g.length) * 2 - 1) + 1.6;
          const ly = labelY.get(g.key)!;
          const mid = (y1 + y2) / 2;
          const bytesText =
            g.length === 1 ? `byte ${g.offset}` : `bytes ${g.offset}–${g.offset + g.length - 1}`;
          return (
            <g
              key={g.key}
              data-gene={g.name}
              tabIndex={0}
              opacity={active && active !== g.name ? 0.12 : 1}
              onMouseEnter={() => set(g.name)}
              onMouseLeave={() => set(null)}
              onFocus={() => set(g.name)}
              onBlur={() => set(null)}
              className="cursor-default outline-none transition-opacity"
            >
              <title>{`${g.name} · ${bytesText} · ${g.rule} · 0x${g.raw} → ${g.value ?? 'not decoded yet'}`}</title>
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
                    rx="1.5"
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
              <rect x={TX - 2} y={r(ly - 13)} width={W - TX + 2} height="17" fill="transparent" />
              <text x={TX} y={r(ly)} fontSize="12" fill={g.color}>
                {clip(g.name, 11)}
              </text>
              <text x={TX + 84} y={r(ly)} fontSize="11.5" fill={GEL_COLORS.label}>
                {bytesText} · {g.rule}
              </text>
              <text x={TX + 264} y={r(ly)} fontSize="11.5" fill={g.color} fillOpacity=".85">
                {shortRaw(g.raw)}
              </text>
              <text x={TX + 344} y={r(ly)} fontSize="12" fill={GEL_COLORS.rulerText}>
                →
              </text>
              <text
                x={TX + 360}
                y={r(ly)}
                fontSize="12"
                fill={
                  g.value === 'empty' || g.value === null ? GEL_COLORS.rulerText : GEL_COLORS.well
                }
              >
                {clip(g.value ?? 'not decoded yet', 28)}
              </text>
            </g>
          );
        })}
        <text x={LX} y={r(labelsEnd - 6)} fontSize="10" fill={GEL_COLORS.label}>
          read top to bottom: each band is one hex digit in the lane of its value · {n} digits ={' '}
          {bytes.length} bytes
        </text>
      </svg>
    </div>
  );
}
