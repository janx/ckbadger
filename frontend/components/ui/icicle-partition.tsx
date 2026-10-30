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
}
export interface IciclePartitionProps {
  root: IcicleNode;
  label: string;
  formatValue: (value: bigint) => ReactNode;
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
export function IciclePartition({ root, label, formatValue }: IciclePartitionProps) {
  const placed: Placed[] = [];
  place(root, 0, 0, W, placed);
  const depth = Math.max(...placed.map((p) => p.depth));
  const height = depth * ROW + BOX + LABELS;
  return (
    <svg
      role="img"
      aria-label={label}
      viewBox={`0 0 ${W} ${height}`}
      width="100%"
      className="block overflow-visible font-mono"
    >
      {placed.map(({ node, depth: d, x, width }) => (
        <g key={node.key}>
          <rect
            data-node={node.key}
            x={x}
            y={d * ROW}
            width={width}
            height={BOX}
            rx="3"
            fill={CLASS_COLORS[node.tone]}
            fillOpacity={node.opacity ?? 0.9}
          />
          <text x={x + 2} y={d * ROW + BOX + 15} fontSize="11" fill={CLASS_COLORS[node.tone]}>
            {node.label}
          </text>
          <text x={x + 2} y={d * ROW + BOX + 30} fontSize="11.5" className="fill-text-bright">
            {formatValue(node.value)}
          </text>
          {node.placeholder && !node.children?.length && d < depth && (
            <g data-placeholder={node.key}>
              <rect
                x={x + 1}
                y={(d + 1) * ROW}
                width={Math.max(width - 2, 0)}
                height={BOX}
                rx="3"
                fill="none"
                className="stroke-base-border"
                strokeDasharray="2 3"
              />
              <text x={x + 8} y={(d + 1) * ROW + 19} fontSize="10.5" className="fill-text-dim">
                {node.placeholder}
              </text>
            </g>
          )}
        </g>
      ))}
    </svg>
  );
}
