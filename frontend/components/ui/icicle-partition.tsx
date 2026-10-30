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
  /** Label ink: the tone's class colour (default) or the bright text ink. */
  ink?: 'tone' | 'bright';
  /** A quiet third line under the value (or after it, inside the box). */
  note?: string;
  /** Under the box from its left edge (default), under it ending at its right edge, or inside it. */
  align?: 'start' | 'end' | 'inside';
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

function place(node: IcicleNode, depth: number, x: number, width: number, out: Placed[]) {
  out.push({ node, depth, x, width });
  if (!node.children?.length) return;
  const spans = partitionSpans(
    node.children.map((c) => c.value),
    node.value,
    x,
    width,
    node.label
  );
  node.children.forEach((child, i) => place(child, depth + 1, spans[i].x, spans[i].width, out));
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
}: IciclePartitionProps) {
  const placed: Placed[] = [];
  place(root, 0, 0, total, placed);
  const depth = Math.max(...placed.map((p) => p.depth));
  const row = hideLabels ? BOX + 8 : ROW;
  const height = depth * row + BOX + (hideLabels ? 0 : LABELS);
  return (
    <svg
      role="img"
      aria-label={label}
      viewBox={`0 0 ${total} ${height}`}
      width="100%"
      className="block overflow-visible font-mono"
    >
      {placed.map(({ node, depth: d, x, width }) => {
        const ink = node.ink === 'bright' ? 'fill-text-bright' : undefined;
        const inkFill = node.ink === 'bright' ? undefined : CLASS_COLORS[node.tone];
        const end = node.align === 'end';
        const tx = end ? x + width - 10 : x + 2;
        const anchor = end ? 'end' : 'start';
        return (
          <g key={node.key}>
            <rect
              data-node={node.key}
              x={x}
              y={d * row}
              width={width}
              height={BOX}
              rx="3"
              fill={node.color ?? CLASS_COLORS[node.tone]}
              fillOpacity={node.opacity ?? 0.9}
            />
            {!hideLabels && node.align === 'inside' && (
              <text
                data-label={node.key}
                x={x + 8}
                y={d * row + 19}
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
            {!hideLabels && node.align !== 'inside' && (
              <>
                <text
                  data-label={node.key}
                  x={tx}
                  y={d * row + BOX + 15}
                  textAnchor={anchor}
                  fontSize="11"
                  fill={inkFill}
                  className={ink}
                >
                  {node.label}
                </text>
                <text
                  x={tx}
                  y={d * row + BOX + 30}
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
                    y={d * row + BOX + 44}
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
                  y={(d + 1) * row}
                  width={Math.max(width - 2, 0)}
                  height={BOX}
                  rx="3"
                  fill="none"
                  className="stroke-base-border"
                  strokeDasharray="2 3"
                />
                {!hideLabels && (
                  <text x={x + 8} y={(d + 1) * row + 19} fontSize="10.5" className="fill-text-dim">
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
