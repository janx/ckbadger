'use client';

import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
  type SVGProps,
} from 'react';
import {
  TerminalPanel,
  TerminalPanelFooter,
  TerminalPanelHeader,
} from '@/components/ui/terminal-panel';
import { usePipelineSnapshot } from '@/hooks/usePipelineSnapshot';
import { usePrefersReducedMotion } from '@/hooks/usePrefersReducedMotion';
import type { PipelineSnapshot } from '@/lib/api';
import type { MotionCtx, MotionView } from '@/lib/pipeline/motion';
import { diffSnapshots, type Transition } from '@/lib/pipeline/transitions';
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
import { MotionSvg } from '@/components/pipeline/motion-svg';
import { NextCard, NextColumn } from '@/components/pipeline/next-card';
import { PipelineLegend } from '@/components/pipeline/pipeline-legend';
import { PoolZone } from '@/components/pipeline/pool-zone';
import { T10, TZ, fmtInt } from '@/components/pipeline/svg-text';
import { WindowLabel, WindowTicks } from '@/components/pipeline/window-ruler';

const SVG_LABEL =
  'Mempool queue, the next block being formed, and the chain of recent blocks drawn as flat cards linked to their parents through their headers, each with a row of proposal chips and a field of committed transactions starting with the cellbase';

const MIN_VIEW_W = 760;

/**
 * Width available to the SVG, measured before the canvas mounts (null until then), so the first
 * frame is already laid out for the real width. The SVG is drawn 1:1 and only scrolls below 760 px.
 */
function useMeasuredWidth(ref: RefObject<HTMLDivElement | null>): number | null {
  const [width, setWidth] = useState<number | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const read = (w: number) =>
      setWidth((prev) => (w <= 0 || (prev !== null && Math.abs(prev - w) <= 1) ? prev : w));
    read(el.getBoundingClientRect().width);
    const ro = new ResizeObserver((entries) => read(entries[entries.length - 1].contentRect.width));
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref]);
  return width;
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

/** The snapshot on screen, how it was reached from the previous one, and the choreography step. */
interface ViewState {
  snapshot: PipelineSnapshot;
  transition: Transition;
  error: Error | null;
  /** 'landing' while a freshly mined block still sits at NEXT, before it joins the chain. */
  phase: ScenePhase;
  seq: number;
}

const toError = (e: unknown) => (e instanceof Error ? e : new Error(String(e)));

function advance(view: ViewState | null, snapshot: PipelineSnapshot, reduced: boolean): ViewState {
  try {
    const transition = diffSnapshots(view && !view.error ? view.snapshot : null, snapshot);
    return {
      snapshot,
      transition,
      error: null,
      phase: transition.mode === 'mined' && !reduced ? 'landing' : 'settled',
      seq: (view?.seq ?? -1) + 1,
    };
  } catch (e) {
    return {
      snapshot,
      transition: { mode: 'boot', events: [], contact: 0, settleAt: 0 },
      error: toError(e),
      phase: 'settled',
      seq: (view?.seq ?? -1) + 1,
    };
  }
}

/** The pipeline canvas for one snapshot stream (exported for the motion tests). */
export function PipelineCanvas({ snapshot }: CanvasProps) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const measured = useMeasuredWidth(wrapRef);
  const viewW = measured === null ? null : Math.max(MIN_VIEW_W, measured);
  const reduced = usePrefersReducedMotion();
  const [view, setView] = useState<ViewState>(() => advance(null, snapshot, reduced));
  if (view.snapshot !== snapshot) {
    // A new observation: diff it against what is on screen (render-time derived state).
    setView(advance(view, snapshot, reduced));
  }

  // The landing block joins the chain once its commits have landed.
  useEffect(() => {
    if (view.phase !== 'landing') return;
    const seq = view.seq;
    const settle = () => setView((v) => (v.seq === seq ? { ...v, phase: 'settled' } : v));
    if (reduced) {
      settle();
      return;
    }
    const id = window.setTimeout(settle, view.transition.settleAt);
    return () => window.clearTimeout(id);
  }, [view.phase, view.seq, view.transition, reduced]);

  const built = useMemo((): { scene: Scene } | { error: Error } | null => {
    if (view.error) return { error: view.error };
    if (viewW === null) return null;
    try {
      return { scene: buildScene(view.snapshot, viewW, view.phase) };
    } catch (e) {
      return { error: toError(e) };
    }
  }, [view.snapshot, view.error, viewW, view.phase]);

  const ctx = useMemo<MotionCtx>(
    () => ({ seq: view.seq, transition: view.transition }),
    [view.seq, view.transition]
  );

  return (
    <div ref={wrapRef} className="overflow-x-auto overscroll-x-contain">
      {built === null ? (
        <div data-testid="pipeline-measuring" className="h-[240px]" />
      ) : 'error' in built ? (
        <StatusRow testId="pipeline-error" tone="text-rouge">
          {`Pipeline snapshot rejected: ${built.error.message}`}
        </StatusRow>
      ) : (
        <SceneSvg scene={built.scene} ctx={ctx} reduced={reduced} />
      )}
    </div>
  );
}

interface SceneSvgProps {
  scene: Scene;
  ctx: MotionCtx;
  reduced: boolean;
}

function SceneSvg({ scene, ctx, reduced }: SceneSvgProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  // The animator reads the hover-dependent bits of the latest render through this.
  const live = useRef({ anchor: 0, highlighted: null as string | null });
  const motionView = useMemo<MotionView>(
    () => ({
      highlighted: (id) => live.current.highlighted === id,
      anchor: () => live.current.anchor,
    }),
    []
  );
  const { consensus } = scene.snapshot;
  const anchor = scene.windowTip + 1;
  live.current.anchor = anchor;
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
  const ruler = windowRuler(rulerSlots, anchor, consensus, scene.viewW);
  const paths = chainPaths(
    chainSlots,
    scene.next ? { x: scene.next.x, w: scene.next.w } : null,
    anchor,
    consensus,
    scene.viewW
  );

  return (
    <MotionSvg
      svgRef={svgRef}
      scene={scene}
      ctx={ctx}
      reduced={reduced}
      view={motionView}
      attrs={
        {
          'data-testid': 'pipeline-svg',
          className:
            'pipeline-svg block h-[240px] w-full min-w-[760px] select-none font-mono tabular-nums',
          viewBox: `0 0 ${scene.viewW} ${G.H}`,
          role: 'img',
          'aria-label': SVG_LABEL,
        } as SVGProps<SVGSVGElement>
      }
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
    </MotionSvg>
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
