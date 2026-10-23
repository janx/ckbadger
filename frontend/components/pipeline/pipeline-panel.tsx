'use client';

import { useLayoutEffect, useMemo, useRef, useState, type RefObject } from 'react';
import {
  TerminalPanel,
  TerminalPanelFooter,
  TerminalPanelHeader,
} from '@/components/ui/terminal-panel';
import { usePipelineSnapshot } from '@/hooks/usePipelineSnapshot';
import type { PipelineSnapshot } from '@/lib/api';
import { tok } from '@/lib/pipeline/encoding';
import {
  G,
  buildScene,
  chainPaths,
  inWindow,
  windowRuler,
  type ChainSlotState,
  type RulerSlot,
  type Scene,
  type ScenePhase,
} from '@/lib/pipeline/layout';
import { BlockCard } from '@/components/pipeline/block-card';
import { ChainLine } from '@/components/pipeline/chain-line';
import { NextCard, NextColumn } from '@/components/pipeline/next-card';
import { PipelineLegend } from '@/components/pipeline/pipeline-legend';
import { PoolZone } from '@/components/pipeline/pool-zone';
import { T10, TZ, fmtInt } from '@/components/pipeline/svg-text';
import { WindowLabel, WindowTicks } from '@/components/pipeline/window-ruler';

const SVG_LABEL =
  'Mempool queue, the next block being formed, and the chain of recent blocks drawn as flat cards linked to their parents through their headers, each with a row of proposal chips and a field of committed transactions starting with the cellbase';

const MIN_VIEW_W = 760;
/** Until the first measurement arrives. */
const UNMEASURED_VIEW_W = 1200;

/** Width of the SVG in its own user units: it is drawn 1:1 and only scrolls below 760 px. */
function useViewWidth(ref: RefObject<SVGSVGElement | null>): number {
  const [width, setWidth] = useState<number | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const w = entries[entries.length - 1].contentRect.width;
      setWidth((prev) => (prev !== null && Math.abs(prev - w) <= 1 ? prev : w));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref]);
  return width ? Math.max(MIN_VIEW_W, width) : UNMEASURED_VIEW_W;
}

function Stat({ label, value, tone }: { label: string; value: string; tone: string }) {
  return (
    <span className="flex items-baseline gap-1.5 font-mono">
      <span className="text-text-dim text-xs uppercase tracking-wider">{label}</span>
      <span className={`text-[13px] font-semibold tabular-nums ${tone}`}>{value}</span>
    </span>
  );
}

function PipelineStats({ snapshot }: { snapshot: PipelineSnapshot | undefined }) {
  const pool = snapshot?.pool ?? null;
  const next = snapshot?.next ?? null;
  return (
    <div data-testid="pipeline-stats" className="flex flex-wrap gap-x-[18px] gap-y-1">
      <Stat label="Mempool" value={pool ? fmtInt(pool.pendingTotal) : '—'} tone="text-aqua" />
      <Stat label="Proposed" value={pool ? fmtInt(pool.proposedTotal) : '—'} tone="text-gold" />
      <Stat label="Next" value={next ? `~${next.txHashes.length + 1} tx` : '—'} tone="text-jade" />
      <Stat
        label="Tip"
        value={snapshot ? `#${fmtInt(snapshot.tip.number)}` : '—'}
        tone="text-text-bright"
      />
    </div>
  );
}

function StatusRow({ testId, tone, children }: { testId: string; tone: string; children: string }) {
  return (
    <div
      data-testid={testId}
      role={testId === 'pipeline-error' ? 'alert' : undefined}
      className={`flex h-[240px] items-center px-3 font-mono text-sm ${tone}`}
    >
      {children}
    </div>
  );
}

interface CanvasProps {
  snapshot: PipelineSnapshot;
}

function PipelineCanvas({ snapshot }: CanvasProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  const viewW = useViewWidth(svgRef);
  const phase: ScenePhase = 'settled';

  const built = useMemo((): { scene: Scene } | { error: Error } => {
    try {
      return { scene: buildScene(snapshot, viewW, phase) };
    } catch (e) {
      return { error: e instanceof Error ? e : new Error(String(e)) };
    }
  }, [snapshot, viewW, phase]);

  if ('error' in built) {
    return (
      <StatusRow testId="pipeline-error" tone="text-rouge">
        {`Pipeline snapshot rejected: ${built.error.message}`}
      </StatusRow>
    );
  }
  const { scene } = built;
  const { consensus } = scene.snapshot;
  const anchor = scene.windowTip + 1;
  const chainSlots: ChainSlotState[] = scene.cards.map((c) => ({
    number: c.block.number,
    x: c.x,
    w: c.w,
    mined: true,
    hash: c.block.hash,
    parentHash: c.block.parentHash,
    solid: 1,
  }));
  const rulerSlots: RulerSlot[] = scene.next
    ? [{ number: scene.next.number, x: scene.next.x, w: scene.next.w, mined: false }, ...chainSlots]
    : chainSlots;
  const ruler = windowRuler(rulerSlots, anchor, consensus, viewW);
  const paths = chainPaths(
    chainSlots,
    scene.next ? { x: scene.next.x, w: scene.next.w } : null,
    anchor,
    consensus,
    viewW
  );

  return (
    <div className="overflow-x-auto overscroll-x-contain">
      <svg
        ref={svgRef}
        data-testid="pipeline-svg"
        className="pipeline-svg block h-[240px] w-full min-w-[760px] select-none font-mono tabular-nums"
        viewBox={`0 0 ${viewW} ${G.H}`}
        role="img"
        aria-label={SVG_LABEL}
      >
        <g pointerEvents="none">
          <line
            x1={G.divX}
            x2={G.divX}
            y1={8}
            y2={G.H - 8}
            strokeWidth={1}
            strokeDasharray="2 3"
            vectorEffect="non-scaling-stroke"
            style={{ stroke: tok('border') }}
          />
          <ChainLine paths={paths} tailOpen={ruler.tailOpen} windowEmphasis={false} />
          <WindowTicks ruler={ruler} faded={false} />
        </g>
        <PoolZone scene={scene} />
        <g data-layer="cards">
          {scene.next && <NextCard next={scene.next} highlighted={false} dim="false" />}
          {scene.cards.map((c) => (
            <BlockCard
              key={c.id}
              card={c}
              numberInWindow={inWindow(c.block.number, anchor, consensus)}
              highlighted={false}
              dim="false"
            />
          ))}
        </g>
        <g data-layer="labels" pointerEvents="none">
          <NextColumn phase={scene.phase} available={scene.snapshot.next !== null} />
          <text x={G.chainX0} y={16} className={TZ}>
            Chain
          </text>
          <text data-chain-sub x={G.chainX0} y={30} className={T10}>
            newest → older
          </text>
          <WindowLabel ruler={ruler} emphasis={false} faded={false} />
        </g>
      </svg>
    </div>
  );
}

const SELECT_HINT =
  'Hover a transaction or a proposal to see how they connect. Click to pin a transaction and follow it from the pool to the block that commits it.';

/** Homepage transaction pipeline: mempool → next block → chain, from one backend snapshot. */
export function PipelinePanel() {
  const { data, error } = usePipelineSnapshot();
  return (
    <TerminalPanel aria-label="Transaction pipeline" data-testid="pipeline-panel">
      <TerminalPanelHeader indicator="active" actions={<PipelineStats snapshot={data} />}>
        <span className="flex items-center gap-2.5">
          Transaction Pipeline
          <span
            title="Two-step confirmation: a tx is proposed in one block and committed in a later block inside its proposal window."
            className="border-text-dim/70 text-text-dim inline-block h-3.5 w-3.5 rounded-full border text-center font-mono text-[9px] normal-case leading-3 tracking-normal"
          >
            ?
          </span>
        </span>
      </TerminalPanelHeader>
      {error ? (
        <StatusRow testId="pipeline-error" tone="text-rouge">
          {`Failed to load the pipeline: ${error.message}`}
        </StatusRow>
      ) : data ? (
        <PipelineCanvas snapshot={data} />
      ) : (
        <StatusRow testId="pipeline-loading" tone="text-text-dim">
          Loading pipeline…
        </StatusRow>
      )}
      <TerminalPanelFooter>
        <div
          data-testid="pipeline-selection"
          aria-live="polite"
          className="text-text mb-1.5 flex min-h-6 flex-wrap items-center gap-x-3 gap-y-1 font-mono text-[11px]"
        >
          <span className="text-text-dim text-[10px] uppercase tracking-wider">Select</span>
          <span>{SELECT_HINT}</span>
        </div>
        <PipelineLegend />
      </TerminalPanelFooter>
    </TerminalPanel>
  );
}
