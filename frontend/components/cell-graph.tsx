'use client';

import dynamic from '@/lib/dynamic-client';
import type { CellGraphProps } from '@/components/cell-graph-renderer';

const CellGraphRenderer = dynamic(() => import('@/components/cell-graph-renderer'), {
  loading: (props: CellGraphProps) => (
    <div
      className="flex w-full items-center justify-center rounded-lg border border-slate-800 bg-slate-900/50"
      style={{ width: props.width ?? '100%', height: props.height ?? 500 }}
    >
      <p className="text-slate-500">Loading graph...</p>
    </div>
  ),
});

export function CellGraph(props: CellGraphProps) {
  return <CellGraphRenderer {...props} />;
}

export default CellGraph;
