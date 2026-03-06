'use client';

import dynamic from '@/lib/dynamic-client';
import type { ProposalGraphProps } from '@/components/proposal-graph-renderer';

const ProposalGraphRenderer = dynamic(() => import('@/components/proposal-graph-renderer'), {
  loading: (props: ProposalGraphProps) => (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-4 text-sm text-slate-400">
        <span>Loading proposal graph...</span>
      </div>
      <div
        className="flex items-center justify-center rounded-lg border border-slate-800 bg-slate-900/50"
        style={{ width: props.width ?? 800, height: props.height ?? 500 }}
      >
        <p className="text-slate-500">Loading graph...</p>
      </div>
    </div>
  ),
});

export function ProposalGraph(props: ProposalGraphProps) {
  return <ProposalGraphRenderer {...props} />;
}

export default ProposalGraph;
