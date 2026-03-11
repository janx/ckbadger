'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { GraphNode, GraphLink } from '@/lib/api';
import type { ForceGraphMethods, NodeObject, LinkObject } from 'react-force-graph-2d';
import { isFocusedCellNode, type FocusCellTarget } from '@/components/cell-graph-utils';
import dynamic from '@/lib/dynamic-client';

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

export interface CellGraphProps {
  nodes: GraphNode[];
  links: GraphLink[];
  onNodeClick?: (node: GraphNode) => void;
  focusCell?: FocusCellTarget;
  width?: number;
  height?: number;
}

const NODE_COLORS = {
  cell: {
    live: '#34d399',
    dead: '#fb7185',
  } as Record<string, string>,
  transaction: '#60a5fa',
};

const LINK_COLORS: Record<string, string> = {
  creates: '#6ee7b7',
  output: '#6ee7b7',
  consumed_by: '#fda4af',
  input: '#fda4af',
};

export function CellGraphRenderer({
  nodes,
  links,
  onNodeClick,
  focusCell,
  width,
  height = 500,
}: CellGraphProps) {
  const graphRef = useRef<ForceGraphMethods | undefined>(undefined);
  const containerRef = useRef<HTMLDivElement>(null);
  const [containerWidth, setContainerWidth] = useState<number | null>(width ?? null);

  const graphData = useMemo(
    () => ({
      nodes: nodes.map((node) => ({ ...node })),
      links: links.map((link) => ({ ...link })),
    }),
    [nodes, links]
  );

  useEffect(() => {
    if (width !== undefined) {
      setContainerWidth(width);
      return;
    }

    const element = containerRef.current;
    if (!element) {
      return;
    }

    const updateWidth = () => {
      const measuredWidth = Math.floor(element.clientWidth);
      if (measuredWidth > 0) {
        setContainerWidth(Math.max(320, measuredWidth));
      }
    };

    updateWidth();

    if (typeof ResizeObserver === 'undefined') {
      return;
    }

    const observer = new ResizeObserver(() => {
      updateWidth();
    });
    observer.observe(element);

    return () => observer.disconnect();
  }, [width]);

  const resolvedWidth = width ?? containerWidth ?? 640;

  useEffect(() => {
    if (!graphRef.current || nodes.length === 0) {
      return;
    }

    const timer = window.setTimeout(() => {
      graphRef.current?.zoomToFit?.(400, 50);
    }, 0);

    return () => {
      window.clearTimeout(timer);
    };
  }, [graphData, height, nodes.length, resolvedWidth]);

  const getNodeColor = useCallback((node: ForceNode) => {
    if (node.nodeType === 'transaction') {
      return NODE_COLORS.transaction;
    }
    const status = (node.data as GraphNode['data'])?.status || 'dead';
    return NODE_COLORS.cell[status];
  }, []);

  const getLinkColor = useCallback((link: ForceLink) => {
    const linkType = link.linkType as string;
    return LINK_COLORS[linkType] || '#6b7280';
  }, []);

  const drawNode = useCallback(
    (node: ForceNode, ctx: CanvasRenderingContext2D, globalScale: number) => {
      const label = (node.label as string) || '';
      const fontSize = 12 / globalScale;
      const isFocused = isFocusedCellNode(node as unknown as GraphNode, focusCell);
      const nodeSize = node.nodeType === 'transaction' ? 8 : isFocused ? 8 : 6;
      const x = node.x ?? 0;
      const y = node.y ?? 0;

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

      ctx.strokeStyle = 'rgba(255, 255, 255, 0.2)';
      ctx.lineWidth = 1 / globalScale;
      ctx.stroke();

      if (isFocused) {
        ctx.beginPath();
        ctx.arc(x, y, nodeSize + 2.5 / globalScale, 0, 2 * Math.PI, false);
        ctx.strokeStyle = '#fcd34d';
        ctx.lineWidth = 1.4 / globalScale;
        ctx.stroke();
      }

      if (globalScale > 0.5) {
        ctx.font = `${fontSize}px sans-serif`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'top';
        ctx.fillStyle = isFocused ? '#fde68a' : '#d1d5db';
        ctx.fillText(label, x, y + nodeSize + 2);
      }
    },
    [focusCell, getNodeColor]
  );

  const handleNodeClick = useCallback(
    (node: ForceNode) => {
      if (onNodeClick) {
        onNodeClick(node as unknown as GraphNode);
      }
    },
    [onNodeClick]
  );

  const nodePointerAreaPaint = useCallback(
    (node: ForceNode, color: string, ctx: CanvasRenderingContext2D) => {
      const isFocused = isFocusedCellNode(node as unknown as GraphNode, focusCell);
      const nodeSize = node.nodeType === 'transaction' ? 10 : isFocused ? 11 : 8;
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.arc(node.x ?? 0, node.y ?? 0, nodeSize, 0, 2 * Math.PI, false);
      ctx.fill();
    },
    [focusCell]
  );

  if (nodes.length === 0) {
    return (
      <div
        ref={containerRef}
        className="border-base-border bg-base-surface/50 flex w-full items-center justify-center rounded-lg border"
        style={{ width: width ?? '100%', height }}
      >
        <p className="text-text-dim">No graph data available</p>
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      className="border-base-border w-full overflow-hidden rounded-lg border"
      style={{ width: width ?? '100%', height }}
    >
      <ForceGraph2D
        ref={graphRef}
        graphData={graphData}
        width={resolvedWidth}
        height={height}
        backgroundColor="#0b1020"
        nodeCanvasObject={drawNode}
        nodePointerAreaPaint={nodePointerAreaPaint}
        linkColor={getLinkColor}
        linkDirectionalArrowLength={5}
        linkDirectionalArrowRelPos={1}
        linkCurvature={0.1}
        linkWidth={1.2}
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

export default CellGraphRenderer;
