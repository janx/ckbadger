'use client';

import dynamic from '@/lib/dynamic-client';
import type { ProposalGraphProps } from '@/components/proposal-graph-renderer';

const ProposalGraphRenderer = dynamic(() => import('@/components/proposal-graph-renderer'), {
  loading: (props: ProposalGraphProps) => (
    <div className="space-y-4">
      <div className="text-text-muted flex flex-wrap items-center gap-4 text-sm">
        <span>Loading proposal graph...</span>
      </div>
      <div
        className="border-base-border bg-base-surface/50 flex items-center justify-center rounded-lg border"
        style={{ width: props.width ?? 800, height: props.height ?? 500 }}
      >
        <p className="text-text-muted">Loading graph...</p>
      </div>
    </div>
  ),
});

export function ProposalGraph(props: ProposalGraphProps) {
  return <ProposalGraphRenderer {...props} />;
}

export default ProposalGraph;
