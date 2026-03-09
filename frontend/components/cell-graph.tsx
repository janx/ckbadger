'use client';

import dynamic from '@/lib/dynamic-client';
import type { CellGraphProps } from '@/components/cell-graph-renderer';

const CellGraphRenderer = dynamic(() => import('@/components/cell-graph-renderer'), {
  loading: (props: CellGraphProps) => (
    <div
      className="border-base-border bg-base-surface/50 flex w-full items-center justify-center rounded-lg border"
      style={{ width: props.width ?? '100%', height: props.height ?? 500 }}
    >
      <p className="text-text-muted">Loading graph...</p>
    </div>
  ),
});

export function CellGraph(props: CellGraphProps) {
  return <CellGraphRenderer {...props} />;
}

export default CellGraph;
