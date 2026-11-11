'use client';

import { useCallback, useMemo, useRef } from 'react';
import type { GraphNode, GraphLink, ProposalGraphMetadata } from '@/lib/api';
import type { ForceGraphMethods, NodeObject, LinkObject } from 'react-force-graph-2d';
import { useRouter } from '@/src/navigation';
import dynamic from '@/lib/dynamic-client';
import { useThemePalette } from '@/hooks/useThemePalette';
import { hexAlpha, type Palette } from '@/lib/palette';

type ForceNode = NodeObject;
type ForceLink = LinkObject;

const ForceGraph2D = dynamic(() => import('react-force-graph-2d'), {
  ssr: false,
  loading: () => (
    <div className="bg-base-surface/50 flex h-full w-full items-center justify-center">
      <div className="text-text-dim">Loading graph...</div>
    </div>
  ),
});

export interface ProposalGraphProps {
  nodes: GraphNode[];
  links: GraphLink[];
  metadata: ProposalGraphMetadata;
  onNodeClick?: (node: GraphNode) => void;
  width?: number;
  height?: number;
}

/**
 * The graph's inks in the current theme, as the canvas needs them (concrete hex from the
 * palette, useThemePalette): the source block violet, a proposal the dim ink, a commit block
 * jade / gold / rouge by how fast it came; the proposing edges violet, the committing edges aqua.
 */
function graphInks(palette: Palette) {
  return {
    source_block: palette.object,
    proposal: palette['text-dim'],
    commit_block: {
      fast: palette.jade,
      medium: palette.gold,
      slow: palette.rouge,
    } as Record<string, string>,
    links: { proposes: palette.object, commits: palette.aqua } as Record<string, string>,
    fallback: palette['text-dim'],
    nodeEdge: hexAlpha(palette['text-bright'], 0.3),
    label: palette.text,
    background: palette['base-surface'],
  };
}

export function ProposalGraphRenderer({
  nodes,
  links,
  metadata,
  onNodeClick,
  width = 800,
  height = 500,
}: ProposalGraphProps) {
  const graphRef = useRef<ForceGraphMethods | undefined>(undefined);
  // Network-aware SPA navigation: a raw `window.location.href = '/blocks/N'`
  // both reloads the app and drops the `/<network>` prefix, so the guard would
  // resolve the block against the DEFAULT network.
  const router = useRouter();

  const graphData = useMemo(
    () => ({
      nodes: nodes.map((node) => ({ ...node })),
      links: links.map((link) => ({ ...link })),
    }),
    [nodes, links]
  );

  const { palette } = useThemePalette();
  const inks = useMemo(() => graphInks(palette), [palette]);

  const getNodeColor = useCallback(
    (node: ForceNode) => {
      if (node.nodeType === 'source_block') {
        return inks.source_block;
      }
      if (node.nodeType === 'proposal') {
        return inks.proposal;
      }
      if (node.nodeType === 'commit_block') {
        const data = node.data as Record<string, unknown> | undefined;
        const speed = (data?.speedCategory as string) || 'slow';
        return inks.commit_block[speed];
      }
      return inks.fallback;
    },
    [inks]
  );

  const getLinkColor = useCallback(
    (link: ForceLink) => {
      const linkType = link.linkType as string;
      return inks.links[linkType] || inks.fallback;
    },
    [inks]
  );

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

      ctx.strokeStyle = inks.nodeEdge;
      ctx.lineWidth = 1 / globalScale;
      ctx.stroke();

      if (globalScale > 0.5 && node.nodeType !== 'proposal') {
        ctx.font = `${fontSize}px sans-serif`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'top';
        ctx.fillStyle = inks.label;
        const labelY = node.nodeType === 'source_block' ? y + 14 : y + 8;
        ctx.fillText(label, x, labelY);
      }
    },
    [getNodeColor, inks]
  );

  const handleNodeClick = useCallback(
    (node: ForceNode) => {
      if (node.nodeType === 'source_block' || node.nodeType === 'commit_block') {
        const data = node.data as Record<string, unknown> | undefined;
        const blockNumber = data?.blockNumber;
        if (blockNumber !== undefined) {
          router.push(`/blocks/${blockNumber}`);
          return;
        }
      }
      if (onNodeClick) {
        onNodeClick(node as unknown as GraphNode);
      }
    },
    [onNodeClick, router]
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
        className="border-base-border bg-base-surface/50 flex items-center justify-center rounded-lg border"
        style={{ width, height }}
      >
        <p className="text-text-dim">No proposal data available</p>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="text-text-dim text-body flex flex-wrap items-center gap-4">
        <div className="flex items-center gap-2">
          <div className="h-3 w-3 rotate-45" style={{ backgroundColor: inks.source_block }} />
          <span>Source Block</span>
        </div>
        <div className="flex items-center gap-2">
          <div className="bg-base-border h-2.5 w-2.5 rounded-full" />
          <span>Proposal</span>
        </div>
        <div className="flex items-center gap-2">
          <div className="h-2.5 w-2.5" style={{ backgroundColor: inks.commit_block.fast }} />
          <span>Fast (+2-4)</span>
        </div>
        <div className="flex items-center gap-2">
          <div className="h-2.5 w-2.5" style={{ backgroundColor: inks.commit_block.medium }} />
          <span>Medium (+5-7)</span>
        </div>
        <div className="flex items-center gap-2">
          <div className="h-2.5 w-2.5" style={{ backgroundColor: inks.commit_block.slow }} />
          <span>Slow (+8-10)</span>
        </div>
      </div>

      <div className="text-body flex items-center gap-6">
        <div className="text-text-dim">
          Total Proposals: <span className="text-text-bright">{metadata.totalProposals}</span>
        </div>
        <div className="text-text-dim">
          Committed: <span className="text-positive">{metadata.committedCount}</span>
        </div>
        {metadata.totalProposals > metadata.committedCount && (
          <div className="text-text-dim">
            Pending:{' '}
            <span className="text-warning">
              {metadata.totalProposals - metadata.committedCount}
            </span>
          </div>
        )}
      </div>

      <div
        className="border-base-border overflow-hidden rounded-lg border"
        style={{ width, height }}
      >
        <ForceGraph2D
          ref={graphRef}
          graphData={graphData}
          width={width}
          height={height}
          backgroundColor={inks.background}
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

export default ProposalGraphRenderer;
