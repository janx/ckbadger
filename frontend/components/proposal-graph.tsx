'use client';

import dynamic from '@/lib/dynamic-client';
import { useCallback, useMemo, useRef } from 'react';
import type { GraphNode, GraphLink, ProposalGraphMetadata } from '@/lib/api';
import type { ForceGraphMethods, NodeObject, LinkObject } from 'react-force-graph-2d';

type ForceNode = NodeObject;
type ForceLink = LinkObject;

const ForceGraph2D = dynamic(() => import('react-force-graph-2d'), {
  ssr: false,
  loading: () => (
    <div className="flex h-full w-full items-center justify-center bg-slate-900/50">
      <div className="text-slate-400">Loading graph...</div>
    </div>
  ),
});

interface ProposalGraphProps {
  nodes: GraphNode[];
  links: GraphLink[];
  metadata: ProposalGraphMetadata;
  onNodeClick?: (node: GraphNode) => void;
  width?: number;
  height?: number;
}

const NODE_COLORS = {
  source_block: '#8b5cf6',
  proposal: '#6b7280',
  commit_block: {
    fast: '#22c55e',
    medium: '#eab308',
    slow: '#ef4444',
  } as Record<string, string>,
};

const LINK_COLORS: Record<string, string> = {
  proposes: '#8b5cf6',
  commits: '#3b82f6',
};

export function ProposalGraph({
  nodes,
  links,
  metadata,
  onNodeClick,
  width = 800,
  height = 500,
}: ProposalGraphProps) {
  const graphRef = useRef<ForceGraphMethods | undefined>(undefined);

  const graphData = useMemo(
    () => ({
      nodes: nodes.map((node) => ({ ...node })),
      links: links.map((link) => ({ ...link })),
    }),
    [nodes, links]
  );

  const getNodeColor = useCallback((node: ForceNode) => {
    if (node.nodeType === 'source_block') {
      return NODE_COLORS.source_block;
    }
    if (node.nodeType === 'proposal') {
      return NODE_COLORS.proposal;
    }
    if (node.nodeType === 'commit_block') {
      const data = node.data as Record<string, unknown> | undefined;
      const speed = (data?.speedCategory as string) || 'slow';
      return NODE_COLORS.commit_block[speed];
    }
    return '#6b7280';
  }, []);

  const getLinkColor = useCallback((link: ForceLink) => {
    const linkType = link.linkType as string;
    return LINK_COLORS[linkType] || '#6b7280';
  }, []);

  const drawNode = useCallback(
    (node: ForceNode, ctx: CanvasRenderingContext2D, globalScale: number) => {
      const label = (node.label as string) || '';
      const fontSize = 12 / globalScale;
      const x = node.x ?? 0;
      const y = node.y ?? 0;

      ctx.beginPath();

      if (node.nodeType === 'source_block') {
        const size = 12;
        ctx.moveTo(x, y - size);
        ctx.lineTo(x + size, y);
        ctx.lineTo(x, y + size);
        ctx.lineTo(x - size, y);
        ctx.closePath();
      } else if (node.nodeType === 'commit_block') {
        const size = 10;
        ctx.rect(x - size / 2, y - size / 2, size, size);
      } else {
        ctx.arc(x, y, 4, 0, 2 * Math.PI, false);
      }

      ctx.fillStyle = getNodeColor(node);
      ctx.fill();

      ctx.strokeStyle = 'rgba(255, 255, 255, 0.3)';
      ctx.lineWidth = 1 / globalScale;
      ctx.stroke();

      if (globalScale > 0.5 && node.nodeType !== 'proposal') {
        ctx.font = `${fontSize}px sans-serif`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'top';
        ctx.fillStyle = '#d1d5db';
        const labelY = node.nodeType === 'source_block' ? y + 14 : y + 8;
        ctx.fillText(label, x, labelY);
      }
    },
    [getNodeColor]
  );

  const handleNodeClick = useCallback(
    (node: ForceNode) => {
      if (node.nodeType === 'source_block' || node.nodeType === 'commit_block') {
        const data = node.data as Record<string, unknown> | undefined;
        const blockNumber = data?.blockNumber;
        if (blockNumber !== undefined) {
          window.location.href = `/blocks/${blockNumber}`;
          return;
        }
      }
      if (onNodeClick) {
        onNodeClick(node as unknown as GraphNode);
      }
    },
    [onNodeClick]
  );

  const nodePointerAreaPaint = useCallback(
    (node: ForceNode, color: string, ctx: CanvasRenderingContext2D) => {
      const nodeSize = node.nodeType === 'proposal' ? 6 : 12;
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.arc(node.x ?? 0, node.y ?? 0, nodeSize, 0, 2 * Math.PI, false);
      ctx.fill();
    },
    []
  );

  if (nodes.length === 0) {
    return (
      <div
        className="flex items-center justify-center rounded-lg border border-slate-800 bg-slate-900/50"
        style={{ width, height }}
      >
        <p className="text-slate-500">No proposal data available</p>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-4 text-sm text-slate-400">
        <div className="flex items-center gap-2">
          <div
            className="h-3 w-3 rotate-45"
            style={{ backgroundColor: NODE_COLORS.source_block }}
          />
          <span>Source Block</span>
        </div>
        <div className="flex items-center gap-2">
          <div className="h-2.5 w-2.5 rounded-full bg-slate-500" />
          <span>Proposal</span>
        </div>
        <div className="flex items-center gap-2">
          <div className="h-2.5 w-2.5" style={{ backgroundColor: NODE_COLORS.commit_block.fast }} />
          <span>Fast (+2-4)</span>
        </div>
        <div className="flex items-center gap-2">
          <div
            className="h-2.5 w-2.5"
            style={{ backgroundColor: NODE_COLORS.commit_block.medium }}
          />
          <span>Medium (+5-7)</span>
        </div>
        <div className="flex items-center gap-2">
          <div className="h-2.5 w-2.5" style={{ backgroundColor: NODE_COLORS.commit_block.slow }} />
          <span>Slow (+8-10)</span>
        </div>
      </div>

      <div className="flex items-center gap-6 text-sm">
        <div className="text-slate-400">
          Total Proposals: <span className="text-white">{metadata.totalProposals}</span>
        </div>
        <div className="text-slate-400">
          Committed: <span className="text-green-400">{metadata.committedCount}</span>
        </div>
        {metadata.totalProposals > metadata.committedCount && (
          <div className="text-slate-400">
            Pending:{' '}
            <span className="text-yellow-400">
              {metadata.totalProposals - metadata.committedCount}
            </span>
          </div>
        )}
      </div>

      <div className="overflow-hidden rounded-lg border border-slate-800" style={{ width, height }}>
        <ForceGraph2D
          ref={graphRef}
          graphData={graphData}
          width={width}
          height={height}
          backgroundColor="#0f0f1a"
          nodeCanvasObject={drawNode}
          nodePointerAreaPaint={nodePointerAreaPaint}
          linkColor={getLinkColor}
          linkDirectionalArrowLength={4}
          linkDirectionalArrowRelPos={1}
          linkCurvature={0.15}
          linkWidth={1}
          onNodeClick={handleNodeClick}
          cooldownTicks={100}
          d3AlphaDecay={0.02}
          d3VelocityDecay={0.3}
          enableZoomInteraction={true}
          enablePanInteraction={true}
        />
      </div>
    </div>
  );
}

export default ProposalGraph;
