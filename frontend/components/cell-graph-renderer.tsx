'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { GraphNode, GraphLink } from '@/lib/api';
import type { ForceGraphMethods, NodeObject, LinkObject } from 'react-force-graph-2d';
import { isFocusedCellNode, type FocusCellTarget } from '@/components/cell-graph-utils';
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

export interface CellGraphProps {
  nodes: GraphNode[];
  links: GraphLink[];
  onNodeClick?: (node: GraphNode) => void;
  focusCell?: FocusCellTarget;
  width?: number;
  height?: number;
}

/**
 * The graph's inks in the current theme, as the canvas needs them (concrete hex from the
 * palette, useThemePalette): a live cell jade, a consumed one rouge, a transaction aqua; the
 * edges that create in the dim jade, the edges that consume in the dim rouge; a focused cell
 * ringed in gold.
 */
function graphInks(palette: Palette) {
  return {
    cell: { live: palette.jade, dead: palette.rouge } as Record<string, string>,
    transaction: palette.aqua,
    links: {
      creates: palette['jade-dim'],
      output: palette['jade-dim'],
      consumed_by: palette['rouge-dim'],
      input: palette['rouge-dim'],
    } as Record<string, string>,
    linkFallback: palette['text-dim'],
    nodeEdge: hexAlpha(palette['text-bright'], 0.2),
    focusRing: palette.gold,
    focusLabel: palette['gold-bright'],
    label: palette.text,
    background: palette['base-surface'],
  };
}

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
  const { palette } = useThemePalette();
  const inks = useMemo(() => graphInks(palette), [palette]);

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

  const getNodeColor = useCallback(
    (node: ForceNode) => {
      if (node.nodeType === 'transaction') {
        return inks.transaction;
      }
      const status = (node.data as GraphNode['data'])?.status || 'dead';
      return inks.cell[status];
    },
    [inks]
  );

  const getLinkColor = useCallback(
    (link: ForceLink) => {
      const linkType = link.linkType as string;
      return inks.links[linkType] || inks.linkFallback;
    },
    [inks]
  );

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

      ctx.strokeStyle = inks.nodeEdge;
      ctx.lineWidth = 1 / globalScale;
      ctx.stroke();

      if (isFocused) {
        ctx.beginPath();
        ctx.arc(x, y, nodeSize + 2.5 / globalScale, 0, 2 * Math.PI, false);
        ctx.strokeStyle = inks.focusRing;
        ctx.lineWidth = 1.4 / globalScale;
        ctx.stroke();
      }

      if (globalScale > 0.5) {
        ctx.font = `${fontSize}px sans-serif`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'top';
        ctx.fillStyle = isFocused ? inks.focusLabel : inks.label;
        ctx.fillText(label, x, y + nodeSize + 2);
      }
    },
    [focusCell, getNodeColor, inks]
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
        backgroundColor={inks.background}
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
