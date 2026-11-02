import type { ReactNode } from 'react';
import { CLASS_COLORS, type AssetClass } from '@/lib/design-tokens';
import { partitionSpans } from '@/lib/partition-geometry';

export interface IcicleNode {
  key: string;
  label: string;
  value: bigint;
  tone: AssetClass;
  opacity?: number;
  /** Shown as a dashed box in the next row when this node does not split further. */
  placeholder?: string;
  children?: readonly IcicleNode[];
  // ---- F2 group dao (additive; omitted = the original drawing) ----
  /** Box fill instead of the tone's class colour (a design token, e.g. the DAO issuance palette). */
  color?: string;
  /** Label ink: the tone's class colour (default), the bright text ink, or (hierarchy v3) the page ink on a light fill. */
  ink?: 'tone' | 'bright' | 'dark';
  /** A quiet third line under the value (or after it, inside the box). */
  note?: string;
  /** Under the box from its left edge (default), under it ending at its right edge, or inside it. */
  align?: 'start' | 'end' | 'inside';
  // ---- hierarchy v3, DAO page (additive; omitted = the labels above) ----
  /**
   * The words inside the box, vertically centred (R12: a graphic names its parts and their shares,
   * never their exact amounts): the name, the share in medium weight and its unit, then a quiet
   * note. Each is written only where it fits the box (R9): all of it, else without the note, else
   * the share alone, else nothing. It replaces the node's other labels.
   */
  boxLabel?: { name: string; share: string; unit?: string; note?: string };
}
export interface IciclePartitionProps {
  root: IcicleNode;
  label: string;
  formatValue: (value: bigint) => ReactNode;
  // ---- F2 group dao (additive) ----
  /** Measured pixel width, so text stays at its set size; default the fixed 900-unit viewBox. */
  width?: number;
  /** Draw only the boxes, rows one box apart; the caller states every word in HTML. */
  hideLabels?: boolean;
  // ---- hierarchy v3, DAO page (additive) ----
  /** Box heights by depth (px) and the gap between rows and between sibling boxes. */
  rows?: { heights: readonly number[]; gap: number };
}

const W = 900;
const BOX = 28;
const ROW = 84;
const LABELS = 44;

interface Placed {
  node: IcicleNode;
  depth: number;
  x: number;
  width: number;
}

function place(node: IcicleNode, depth: number, x: number, width: number, out: Placed[], gap = 0) {
  out.push({ node, depth, x, width });
  if (!node.children?.length) return;
  // Siblings share the parent's width less the gaps between them, each shifted by its gaps.
  const n = node.children.length;
  const spans = partitionSpans(
    node.children.map((c) => c.value),
    node.value,
    x,
    width - gap * (n - 1),
    node.label
  );
  node.children.forEach((child, i) =>
    place(child, depth + 1, round2(spans[i].x + gap * i), spans[i].width, out, gap)
  );
}

const round2 = (n: number) => Math.round(n * 100) / 100;

/**
 * A monospace advance in px: browsers that snap glyph advances to whole pixels (Chromium: 11 and
 * 12 px → 7, 13 px → 8) and those that do not (0.6 em) both fit inside this.
 */
const advance = (px: number) => Math.max(Math.round(0.6 * px), 0.6 * px);

/** The words of a `boxLabel` that fit a box `width` wide at `px`, padded `pad` on each side. */
function fittedLabel(
  label: NonNullable<IcicleNode['boxLabel']>,
  width: number,
  px: number,
  pad: number
): { name: boolean; note: boolean } | 'share' | null {
  const fits = (text: string) => text.length * advance(px) + 2 * pad <= width;
  const core = `${label.name} ${label.share}${label.unit ? ` ${label.unit}` : ''}`;
  if (label.note && fits(`${core} · ${label.note}`)) return { name: true, note: true };
  if (fits(core)) return { name: true, note: false };
  if (fits(label.share)) return 'share';
  return null;
}

/**
 * An aligned partition (icicle, DAO-7): each row splits the row above exactly, and every part sits
 * under the part it came from. Nodes that stop splitting may leave a dashed note in the next row.
 */
export function IciclePartition({
  root,
  label,
  formatValue,
  width: total = W,
  hideLabels = false,
  rows,
}: IciclePartitionProps) {
  const placed: Placed[] = [];
  place(root, 0, 0, total, placed, rows?.gap ?? 0);
  const depth = Math.max(...placed.map((p) => p.depth));
  if (rows && rows.heights.length !== depth + 1)
    throw new Error(`${label}: ${rows.heights.length} row heights for ${depth + 1} rows`);
  const boxH = (d: number) => (rows ? rows.heights[d] : BOX);
  // Row tops: a row whose words all sit inside its boxes needs no label band below it (F2 dao);
  // otherwise rows stay ROW apart, the original drawing. Set rows sit their gap apart (v3).
  const rowY = [0];
  for (let d = 0; d < depth; d++) {
    const labelsBelow = placed.some((p) => p.depth === d && p.node.align !== 'inside');
    rowY.push(
      rowY[d] +
        (rows ? rows.heights[d] + rows.gap : hideLabels ? BOX + 8 : labelsBelow ? ROW : BOX + 16)
    );
  }
  const lastBelow = placed.some((p) => p.depth === depth && p.node.align !== 'inside');
  const height = rowY[depth] + boxH(depth) + (rows || hideLabels || !lastBelow ? 0 : LABELS);
  return (
    <svg
      role="img"
      aria-label={label}
      viewBox={`0 0 ${total} ${height}`}
      width="100%"
      className="block overflow-visible font-mono"
    >
      {placed.map(({ node, depth: d, x, width }) => {
        const ink =
          node.ink === 'bright'
            ? 'fill-text-bright'
            : node.ink === 'dark'
              ? 'fill-base-bg'
              : undefined;
        const inkFill =
          node.ink === 'bright' || node.ink === 'dark' ? undefined : CLASS_COLORS[node.tone];
        const h = boxH(d);
        // hierarchy v3: words inside the box at body (a ≥ 40 px row) or meta, where they fit.
        const px = h >= 40 ? 13 : 11;
        const pad = h >= 40 ? 12 : 6;
        const fitted =
          !hideLabels && node.boxLabel ? fittedLabel(node.boxLabel, width, px, pad) : null;
        const end = node.align === 'end';
        const tx = end ? x + width - 10 : x + 2;
        const anchor = end ? 'end' : 'start';
        return (
          <g key={node.key}>
            <rect
              data-node={node.key}
              x={x}
              y={rowY[d]}
              width={width}
              height={h}
              rx="3"
              fill={node.color ?? CLASS_COLORS[node.tone]}
              fillOpacity={node.opacity ?? 0.9}
            />
            {node.boxLabel && fitted !== null && (
              <text
                data-label={node.key}
                x={x + pad}
                y={rowY[d] + h / 2 + Math.round(px * 0.35)}
                fontSize={px}
                className={ink}
              >
                {fitted === 'share' ? (
                  <tspan fontWeight={500}>{node.boxLabel.share}</tspan>
                ) : (
                  <>
                    {node.boxLabel.name} <tspan fontWeight={500}>{node.boxLabel.share}</tspan>
                    {node.boxLabel.unit && (
                      <tspan className={node.ink === 'dark' ? undefined : 'fill-text'}>
                        {' '}
                        {node.boxLabel.unit}
                      </tspan>
                    )}
                    {fitted.note && (
                      <tspan
                        data-part="note"
                        className={node.ink === 'dark' ? undefined : 'fill-text-dim'}
                        fillOpacity={node.ink === 'dark' ? 0.7 : undefined}
                      >
                        {' · '}
                        {node.boxLabel.note}
                      </tspan>
                    )}
                  </>
                )}
              </text>
            )}
            {!hideLabels && !node.boxLabel && node.align === 'inside' && (
              <text
                data-label={node.key}
                x={x + 8}
                y={rowY[d] + 19}
                fontSize="12"
                fill={inkFill}
                className={ink}
              >
                {node.label} <tspan className="fill-text-bright">{formatValue(node.value)}</tspan>
                {node.note && (
                  <tspan className="fill-text" fontSize="11">
                    {' · '}
                    {node.note}
                  </tspan>
                )}
              </text>
            )}
            {!hideLabels && !node.boxLabel && node.align !== 'inside' && (
              <>
                <text
                  data-label={node.key}
                  x={tx}
                  y={rowY[d] + BOX + 15}
                  textAnchor={anchor}
                  fontSize="11"
                  fill={inkFill}
                  className={ink}
                >
                  {node.label}
                </text>
                <text
                  x={tx}
                  y={rowY[d] + BOX + 30}
                  textAnchor={anchor}
                  fontSize="11.5"
                  className="fill-text-bright"
                >
                  {formatValue(node.value)}
                </text>
                {node.note && (
                  <text
                    data-note={node.key}
                    x={tx}
                    y={rowY[d] + BOX + 44}
                    textAnchor={anchor}
                    fontSize="10.5"
                    className="fill-text-dim"
                  >
                    {node.note}
                  </text>
                )}
              </>
            )}
            {node.placeholder && !node.children?.length && d < depth && (
              <g data-placeholder={node.key}>
                <rect
                  x={x + 1}
                  y={rowY[d + 1]}
                  width={Math.max(width - 2, 0)}
                  height={boxH(d + 1)}
                  rx="3"
                  fill="none"
                  className="stroke-base-border"
                  strokeDasharray="2 3"
                />
                {!hideLabels && (
                  <text x={x + 8} y={rowY[d + 1] + 19} fontSize="10.5" className="fill-text-dim">
                    {node.placeholder}
                  </text>
                )}
              </g>
            )}
          </g>
        );
      })}
    </svg>
  );
}
