/* eslint-disable @typescript-eslint/no-explicit-any */
'use client';

import dynamic from 'next/dynamic';
import { useCallback, useMemo, useRef } from 'react';
import type { GraphNode, GraphLink } from '@/lib/api';

const ForceGraph2D = dynamic(() => import('react-force-graph-2d'), {
  ssr: false,
  loading: () => (
    <div className="flex h-full w-full items-center justify-center bg-slate-900/50">
      <div className="text-slate-400">Loading graph...</div>
    </div>
  ),
});

interface CellGraphProps {
  nodes: GraphNode[];
  links: GraphLink[];
  onNodeClick?: (node: GraphNode) => void;
  width?: number;
  height?: number;
}

const NODE_COLORS = {
  cell: {
    live: '#22c55e',
    dead: '#ef4444',
  },
  transaction: '#3b82f6',
};

const LINK_COLORS: Record<string, string> = {
  creates: '#22c55e',
  output: '#22c55e',
  consumed_by: '#ef4444',
  input: '#ef4444',
};

export function CellGraph({
  nodes,
  links,
  onNodeClick,
  width = 800,
  height = 500,
}: CellGraphProps) {
  const graphRef = useRef<any>(null);

  const graphData = useMemo(
    () => ({
      nodes: nodes.map((node) => ({ ...node })),
      links: links.map((link) => ({ ...link })),
    }),
    [nodes, links]
  );

  const getNodeColor = useCallback((node: any) => {
    if (node.nodeType === 'transaction') {
      return NODE_COLORS.transaction;
    }
    const status = (node.data?.status as 'live' | 'dead') || 'dead';
    return NODE_COLORS.cell[status];
  }, []);

  const getLinkColor = useCallback((link: any) => {
    return LINK_COLORS[link.linkType] || '#6b7280';
  }, []);

  const drawNode = useCallback(
    (node: any, ctx: CanvasRenderingContext2D, globalScale: number) => {
      const label = node.label || '';
      const fontSize = 12 / globalScale;
      const nodeSize = node.nodeType === 'transaction' ? 8 : 6;
      const x = node.x || 0;
      const y = node.y || 0;

      ctx.beginPath();

      if (node.nodeType === 'transaction') {
        ctx.moveTo(x, y - nodeSize);
        ctx.lineTo(x + nodeSize, y);
        ctx.lineTo(x, y + nodeSize);
        ctx.lineTo(x - nodeSize, y);
        ctx.closePath();
      } else {
        ctx.arc(x, y, nodeSize, 0, 2 * Math.PI, false);
      }

      ctx.fillStyle = getNodeColor(node);
      ctx.fill();

      ctx.strokeStyle = 'rgba(255, 255, 255, 0.3)';
      ctx.lineWidth = 1 / globalScale;
      ctx.stroke();

      if (globalScale > 0.5) {
        ctx.font = `${fontSize}px sans-serif`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'top';
        ctx.fillStyle = '#d1d5db';
        ctx.fillText(label, x, y + nodeSize + 2);
      }
    },
    [getNodeColor]
  );

  const handleNodeClick = useCallback(
    (node: any) => {
      if (onNodeClick) {
        onNodeClick(node as GraphNode);
      }
    },
    [onNodeClick]
  );

  const nodePointerAreaPaint = useCallback(
    (node: any, color: string, ctx: CanvasRenderingContext2D) => {
      const nodeSize = node.nodeType === 'transaction' ? 10 : 8;
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.arc(node.x || 0, node.y || 0, nodeSize, 0, 2 * Math.PI, false);
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
        <p className="text-slate-500">No graph data available</p>
      </div>
    );
  }

  return (
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
        linkDirectionalArrowLength={6}
        linkDirectionalArrowRelPos={1}
        linkCurvature={0.1}
        linkWidth={1.5}
        onNodeClick={handleNodeClick}
        cooldownTicks={100}
        d3AlphaDecay={0.02}
        d3VelocityDecay={0.3}
        enableZoomInteraction={true}
        enablePanInteraction={true}
      />
    </div>
  );
}

export default CellGraph;
