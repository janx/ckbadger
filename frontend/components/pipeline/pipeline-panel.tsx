'use client';

import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  type RefObject,
  type SVGProps,
} from 'react';
import {
  TerminalPanel,
  TerminalPanelFooter,
  TerminalPanelHeader,
} from '@/components/ui/terminal-panel';
import { usePipelineSnapshot } from '@/hooks/usePipelineSnapshot';
import { usePipelineHoverStore } from '@/hooks/usePipelineHoverStore';
import { usePrefersReducedMotion } from '@/hooks/usePrefersReducedMotion';
import type { PipelineSnapshot } from '@/lib/api';
import { arcPath, type MotionCtx, type MotionView } from '@/lib/pipeline/motion';
import { diffSnapshots, type Transition } from '@/lib/pipeline/transitions';
import { STAGE_BASE, mixColor, tok } from '@/lib/pipeline/encoding';
import {
  G,
  buildScene,
  chainPaths,
  inWindow,
  pick,
  pickTargets,
  tail4,
  windowRuler,
  type ChainSlotState,
  type RulerSlot,
  type Scene,
  type ScenePhase,
  type SceneSquare,
} from '@/lib/pipeline/layout';
import { BlockCard, type DimLevel } from '@/components/pipeline/block-card';
import { ChainLine } from '@/components/pipeline/chain-line';
import { MotionSvg } from '@/components/pipeline/motion-svg';
import { NextCard, NextColumn } from '@/components/pipeline/next-card';
import { PipelineLegend } from '@/components/pipeline/pipeline-legend';
import { PoolZone } from '@/components/pipeline/pool-zone';
import Link from '@/components/ui/link';
import {
  PipelineTooltip,
  blockTip,
  commitNote,
  fmtBytes,
  shortHash,
  chipTip,
  nextTip,
  predictedTip,
  txTip,
  uncleTip,
  windowTip,
  type TipContent,
  type TxInfo,
} from '@/components/pipeline/pipeline-tooltip';
import { T10C, TZ, fmtInt } from '@/components/pipeline/svg-text';
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
      <Stat
        label="Next"
        value={next ? `~${fmtInt(next.txs.length + next.omittedTxs + 1)} tx` : '—'}
        tone="text-jade"
      />
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

/** Every drawn tx by hash, with what a hover needs to explain it. */
function txIndex(scene: Scene): Map<string, TxInfo> {
  const out = new Map<string, TxInfo>();
  const pool = scene.snapshot.pool;
  if (pool && scene.pool) {
    const hollow = new Set(
      scene.pool.proposed.squares.filter((q) => q.style === 'hollow').map((q) => q.id)
    );
    for (const tx of pool.pending) out.set(tx.hash, { kind: 'pending', tx });
    for (const tx of pool.proposed) {
      out.set(tx.hash, { kind: 'proposed', tx, tooEarly: hollow.has(tx.hash) });
    }
  }
  for (const c of scene.cards) {
    out.set(c.block.cellbase.hash, { kind: 'cellbase', block: c.block });
    for (const tx of c.block.txs) out.set(tx.hash, { kind: 'committed', tx, block: c.block });
  }
  return out;
}

interface AbsSquare {
  X: number;
  Y: number;
  s: number;
  stage: SceneSquare['stage'];
}

const centreOf = (q: { X: number; Y: number; s: number }) => ({
  X: q.X + q.s / 2,
  Y: q.Y + q.s / 2,
});

function Ring({
  q,
  pad,
  testId,
  dashed,
}: {
  q: AbsSquare;
  pad: number;
  testId: string;
  dashed?: boolean;
}) {
  return (
    <rect
      data-hover={testId}
      x={q.X - pad}
      y={q.Y - pad}
      width={q.s + 2 * pad}
      height={q.s + 2 * pad}
      rx={2}
      fill="none"
      strokeWidth={1.5}
      strokeDasharray={dashed ? '3 2' : undefined}
      vectorEffect="non-scaling-stroke"
      style={{ stroke: tok('ink-bright') }}
    />
  );
}

function SceneSvg({ scene, ctx, reduced }: SceneSvgProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  const tipRef = useRef<HTMLDivElement>(null);
  const pointer = useRef({ x: 0, y: 0 });
  const hoverBlock = usePipelineHoverStore((s) => s.hoverBlock);
  const hoverTx = usePipelineHoverStore((s) => s.hoverTx);
  const pinned = usePipelineHoverStore((s) => s.pinned);
  const windowBlock = usePipelineHoverStore((s) => s.windowBlock);
  const setHover = usePipelineHoverStore((s) => s.setHover);
  const clearHover = usePipelineHoverStore((s) => s.clearHover);
  const togglePin = usePipelineHoverStore((s) => s.togglePin);
  const clearPin = usePipelineHoverStore((s) => s.clearPin);
  const [lane, setLane] = useState(false);
  const [arcBlock, setArcBlock] = useState<number | null>(null);
  const [cursor, setCursor] = useState<'' | 'pointer' | 'help'>('');
  const [tip, setTip] = useState<TipContent | null>(null);

  // The animator reads the hover-dependent bits of the latest render through this.
  const live = useRef({ anchor: 0, highlighted: null as string | null });
  const motionView = useMemo<MotionView>(
    () => ({
      highlighted: (id) => live.current.highlighted === id,
      anchor: () => live.current.anchor,
    }),
    []
  );

  const { snapshot } = scene;
  const { consensus } = snapshot;
  const index = useMemo(() => txIndex(scene), [scene]);
  const targets = useMemo(() => pickTargets(scene), [scene]);
  const byNumber = useMemo(() => new Map(scene.cards.map((c) => [c.block.number, c])), [scene]);
  const squares = useMemo(() => {
    const out = new Map<string, AbsSquare>();
    const add = (q: SceneSquare, dx: number) =>
      out.set(q.id, { X: dx + q.x, Y: q.y, s: q.s, stage: q.stage });
    if (scene.pool)
      for (const q of [...scene.pool.pending.squares, ...scene.pool.proposed.squares]) add(q, 0);
    for (const c of scene.cards) for (const q of c.squares) add(q, c.x);
    return out;
  }, [scene]);
  // Predicted txs on the NEXT card, by hash, in absolute coordinates.
  const ghostOfTx = useMemo(() => {
    const out = new Map<string, { X: number; Y: number; s: number; size: number }>();
    const next = scene.next;
    if (!next) return out;
    const sizes = new Map(next.next.txs.map((t) => [t.hash, t.size]));
    for (const g of next.ghosts) {
      if (g.id) out.set(g.id, { X: next.x + g.x, Y: g.y, s: g.s, size: sizes.get(g.id) as number });
    }
    return out;
  }, [scene]);
  const chipOfTx = useMemo(() => {
    const out = new Map<string, { X: number; Y: number }>();
    for (const c of scene.cards) {
      for (const k of c.chips) {
        const h = k.proposal.txHash;
        if (h && !out.has(h)) out.set(h, { X: c.x + k.x, Y: k.y });
      }
    }
    return out;
  }, [scene]);

  const defaultH = scene.windowTip + 1;
  const anchor = windowBlock ?? defaultH;
  const highlighted = hoverBlock === null ? null : (byNumber.get(hoverBlock)?.id ?? null);
  live.current.anchor = anchor;
  live.current.highlighted = highlighted;

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
  const laneGeom = windowRuler(rulerSlots, defaultH, consensus, scene.viewW).hit;
  const paths = chainPaths(
    chainSlots,
    scene.next ? { x: scene.next.x, w: scene.next.w } : null,
    anchor,
    consensus,
    scene.viewW
  );

  // Pointing at a block makes it H: blocks outside its window step back (prototype paintCard).
  const dimAnchor = windowBlock ?? (lane ? anchor : null);
  const cardDim = (n: number): DimLevel =>
    dimAnchor === null || n === dimAnchor || inWindow(n, dimAnchor, consensus) ? 'false' : 'true';
  const poolFocus = lane && anchor === defaultH;
  const poolDim = (hash: string): DimLevel => {
    if (poolFocus) {
      const info = index.get(hash);
      const p = info?.kind === 'proposed' ? info.tx.proposedIn : null;
      return p && inWindow(p.block, anchor, consensus) ? 'false' : 'true';
    }
    return windowBlock !== null ? 'soft' : 'false';
  };

  const hovered = hoverTx ? (squares.get(hoverTx) ?? null) : null;
  const pinnedSq = pinned && pinned !== hoverTx ? (squares.get(pinned) ?? null) : null;
  const chip = hoverTx ? (chipOfTx.get(hoverTx) ?? null) : null;
  const ghost = hoverTx ? (ghostOfTx.get(hoverTx) ?? null) : null;
  const onIds = useMemo(() => new Set(hovered && hoverTx ? [hoverTx] : []), [hovered, hoverTx]);

  // The reward link of a hovered cellbase, to the cellbase of the block it pays.
  const arc = (() => {
    if (arcBlock === null) return null;
    const c = byNumber.get(arcBlock);
    const pays = c?.block.cellbase.pays;
    const payee = pays ? scene.cards.find((k) => k.id === pays.hash) : undefined;
    if (!c || !pays || !payee) return null;
    const a = squares.get(c.block.cellbase.hash) as AbsSquare;
    const b = squares.get(payee.block.cellbase.hash) as AbsSquare;
    return { from: c.block.number, to: pays.number, d: arcPath(centreOf(a), centreOf(b)).d };
  })();

  const place = () => {
    const el = tipRef.current;
    if (!el) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    let x = pointer.current.x + 14;
    let y = pointer.current.y + 14;
    if (x + w > window.innerWidth - 8) x = pointer.current.x - w - 14;
    if (y + h > window.innerHeight - 8) y = pointer.current.y - h - 14;
    el.style.left = `${Math.max(8, x)}px`;
    el.style.top = `${Math.max(8, y)}px`;
  };
  useLayoutEffect(place, [tip]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') clearPin();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [clearPin]);

  const toLocal = (e: { clientX: number; clientY: number }) => {
    const r = (svgRef.current as SVGSVGElement).getBoundingClientRect();
    return {
      x: (e.clientX - r.left) * (scene.viewW / r.width),
      y: (e.clientY - r.top) * (G.H / r.height),
    };
  };
  const show = (content: TipContent | null) =>
    setTip((prev) => (prev?.key === content?.key ? prev : content));
  const inLane = (p: { x: number; y: number }) => {
    const g = laneGeom;
    if (!g) return false;
    if (p.y >= G.cardTop - 16 && p.y < G.cardTop) return p.x >= g.x0 && p.x <= g.x1;
    return p.y >= G.cardTop && p.y <= G.cardTop + G.headH && p.x >= g.xn && p.x <= g.xf;
  };

  const onPointerMove = (e: ReactPointerEvent<SVGSVGElement>) => {
    pointer.current = { x: e.clientX, y: e.clientY };
    place();
    const p = toLocal(e);
    const hit = pick(targets, p.x, p.y);
    let nextLane = false;
    let nextArc: number | null = null;
    let nextCursor: '' | 'pointer' | 'help' = '';
    if (hit?.kind === 'tx') {
      const info = index.get(hit.id);
      if (!info) throw new Error(`pipeline: picked tx ${hit.id} is not in the snapshot`);
      const block =
        info.kind === 'committed' || info.kind === 'cellbase'
          ? info.block.number
          : info.kind === 'proposed'
            ? (info.tx.proposedIn as NonNullable<typeof info.tx.proposedIn>).block
            : null;
      setHover({ block, tx: hit.id, window: info.kind === 'committed' ? info.block.number : null });
      nextArc = info.kind === 'cellbase' ? info.block.number : null;
      nextCursor = 'pointer';
      show(txTip(info, snapshot, Date.now()));
    } else if (hit?.kind === 'ghost') {
      // A predicted tx: linked to its pool square by hash when the listed pool has it.
      const g = ghostOfTx.get(hit.id);
      if (!g) throw new Error(`pipeline: picked ghost ${hit.id} is not on the NEXT card`);
      const info = index.get(hit.id);
      setHover({ block: null, tx: hit.id, window: null });
      nextCursor = 'pointer';
      show(
        predictedTip(
          { hash: hit.id, size: g.size },
          info?.kind === 'proposed' ? info.tx : null,
          snapshot
        )
      );
    } else if (hit?.kind === 'chip') {
      const card = scene.cards.find((c) => c.id === hit.card);
      const k = card?.chips.find((x) => x.key === hit.key);
      if (!card || !k) throw new Error(`pipeline: picked chip ${hit.key} is not on screen`);
      setHover({ block: card.block.number, tx: k.proposal.txHash, window: null });
      nextCursor = 'pointer';
      show(chipTip(card.block, k.proposal, snapshot));
    } else {
      const el = e.target as Element;
      const cardEl = el.closest('[data-card]');
      const uncleEl = el.closest('[data-uncle]');
      if (el.closest('[data-next]') && snapshot.next) {
        clearHover();
        show(nextTip(snapshot));
      } else if (cardEl) {
        const n = Number(cardEl.getAttribute('data-number'));
        const card = byNumber.get(n);
        if (!card) throw new Error(`pipeline: hovered card #${n} is not in the scene`);
        if (uncleEl) {
          setHover({ block: n, tx: null, window: null });
          show(uncleTip(card.block, Number(uncleEl.getAttribute('data-uncle'))));
        } else {
          setHover({ block: n, tx: null, window: n });
          show(blockTip(card.block, snapshot));
        }
      } else if (inLane(p)) {
        clearHover();
        nextLane = true;
        nextCursor = 'help';
        show(windowTip(defaultH, snapshot));
      } else {
        clearHover();
        show(null);
      }
    }
    setLane(nextLane);
    setArcBlock(nextArc);
    setCursor(nextCursor);
  };
  const onPointerLeave = () => {
    clearHover();
    setLane(false);
    setArcBlock(null);
    setCursor('');
    show(null);
  };
  const onClick = (e: ReactMouseEvent<SVGSVGElement>) => {
    const p = toLocal(e);
    const hit = pick(targets, p.x, p.y);
    const id =
      hit?.kind === 'tx' || hit?.kind === 'ghost'
        ? hit.id
        : hit?.kind === 'chip'
          ? (scene.cards.find((c) => c.id === hit.card)?.chips.find((k) => k.key === hit.key)
              ?.proposal.txHash ?? null)
          : null;
    if (id && squares.has(id)) togglePin(id);
    else clearPin();
  };

  const focus = hovered !== null;
  return (
    <>
      <MotionSvg
        svgRef={svgRef}
        scene={scene}
        ctx={ctx}
        reduced={reduced}
        view={motionView}
        attrs={
          {
            'data-testid': 'pipeline-svg',
            className: `pipeline-svg block h-[240px] w-full min-w-[760px] select-none font-mono tabular-nums${focus ? ' is-focus' : ''}`,
            style: cursor ? { cursor } : undefined,
            viewBox: `0 0 ${scene.viewW} ${G.H}`,
            role: 'img',
            'aria-label': SVG_LABEL,
            onPointerMove,
            onPointerLeave,
            onClick,
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
          <ChainLine paths={paths} tailOpen={ruler.tailOpen} windowEmphasis={lane} />
          <WindowTicks ruler={ruler} faded={arc !== null} />
        </g>
        <PoolZone scene={scene} dimOf={poolDim} onIds={onIds} />
        <g data-layer="cards">
          {scene.next && (
            <NextCard
              next={scene.next}
              highlighted={false}
              dim={dimAnchor === null || dimAnchor === scene.next.number ? 'false' : 'true'}
            />
          )}
          {scene.cards.map((c) => (
            <BlockCard
              key={c.id}
              card={c}
              numberInWindow={inWindow(c.block.number, anchor, consensus)}
              highlighted={highlighted === c.id}
              dim={cardDim(c.block.number)}
              onIds={onIds}
            />
          ))}
        </g>
        <g data-layer="labels" pointerEvents="none">
          <NextColumn available={snapshot.next !== null} />
          <text x={G.chainX0} y={16} className={TZ}>
            Chain
          </text>
          {arc && (
            <text data-chain-sub x={G.chainX0} y={30} className={T10C}>
              {`the cellbase of ${tail4(arc.from)} pays the reward of ${tail4(arc.to)}, ${arc.from - arc.to} blocks back`}
            </text>
          )}
          <WindowLabel ruler={ruler} emphasis={lane} faded={arc !== null} />
        </g>
        <g data-layer="hover" pointerEvents="none">
          {arc && (
            <path
              data-hover="reward"
              d={arc.d}
              fill="none"
              strokeWidth={1}
              strokeOpacity={0.8}
              strokeDasharray="2 3"
              vectorEffect="non-scaling-stroke"
              style={{ stroke: tok('jade') }}
            />
          )}
          {chip && (
            <rect
              data-hover="chip-ring"
              x={chip.X - 1.5}
              y={chip.Y - 1.5}
              width={G.chip + 3}
              height={G.chip + 3}
              rx={1.5}
              fill="none"
              strokeWidth={1.5}
              vectorEffect="non-scaling-stroke"
              style={{ stroke: tok('ink-bright') }}
            />
          )}
          {chip &&
            hovered &&
            (() => {
              const a = { X: chip.X + G.chip / 2, Y: chip.Y + G.chip / 2 };
              const b = centreOf(hovered);
              const top = Math.min(a.Y, b.Y) - 26;
              return (
                <path
                  data-hover="link"
                  d={`M${a.X.toFixed(1)} ${a.Y.toFixed(1)}Q${((a.X + b.X) / 2).toFixed(1)} ${top.toFixed(1)} ${b.X.toFixed(1)} ${b.Y.toFixed(1)}`}
                  fill="none"
                  strokeWidth={1}
                  strokeOpacity={0.9}
                  strokeDasharray="2 2"
                  vectorEffect="non-scaling-stroke"
                  style={{ stroke: tok('gold') }}
                />
              );
            })()}
          {hovered && (
            <>
              <rect
                data-hover="halo"
                x={hovered.X - 4}
                y={hovered.Y - 4}
                width={hovered.s + 8}
                height={hovered.s + 8}
                rx={3}
                fill="none"
                strokeWidth={3}
                strokeOpacity={0.45}
                vectorEffect="non-scaling-stroke"
                style={{ stroke: mixColor(STAGE_BASE[hovered.stage], 'white', 0.4) }}
              />
              <Ring q={hovered} pad={2} testId="ring" />
            </>
          )}
          {pinnedSq && <Ring q={pinnedSq} pad={2.5} testId="pin-ring" dashed />}
          {ghost && (
            <Ring q={{ ...ghost, stage: 'committed' }} pad={2} testId="ghost-ring" dashed />
          )}
        </g>
      </MotionSvg>
      <PipelineTooltip ref={tipRef} content={tip} />
    </>
  );
}

const SELECT_HINT =
  'Hover a transaction or a proposal to see how they connect. Click to pin a transaction and follow it from the pool to the block that commits it.';

const TAG_TONE = {
  pending: 'text-aqua border-aqua/50',
  proposed: 'text-gold border-gold/50',
  committed: 'text-jade border-jade/50',
  cellbase: 'text-jade border-jade/50',
} as const;

/** Where the pinned tx is now, in one line, read from the snapshot on screen. */
function selectionText(
  hash: string,
  s: PipelineSnapshot
): { tag: keyof typeof TAG_TONE; text: string } | null {
  const pending = s.pool?.pending.find((t) => t.hash === hash);
  if (pending) {
    return {
      tag: 'pending',
      text: `${fmtBytes(pending.size)} · fee rate ${fmtInt(Number(pending.feeRate))} sh/kB · waiting to be proposed by the next block`,
    };
  }
  const proposed = s.pool?.proposed.find((t) => t.hash === hash);
  if (proposed?.proposedIn) {
    const block = s.blocks.find((b) => b.number === proposed.proposedIn?.block);
    const state = block?.proposals.find((p) => p.txHash === hash)?.state;
    return {
      tag: 'proposed',
      text: `proposal in #${fmtInt(proposed.proposedIn.block)} · can commit next? ${commitNote(proposed.proposedIn.block, state === 'early', s)}`,
    };
  }
  for (const b of s.blocks) {
    if (b.cellbase.hash === hash) {
      return {
        tag: 'cellbase',
        text: `first tx of #${fmtInt(b.number)}${b.cellbase.pays ? ` · pays the reward of #${fmtInt(b.cellbase.pays.number)}` : ''}`,
      };
    }
    const tx = b.txs.find((t) => t.hash === hash);
    if (tx) {
      return {
        tag: 'committed',
        text: `committed in #${fmtInt(b.number)} · its proposal is in #${fmtInt(tx.proposedIn.block)}, ${b.number - tx.proposedIn.block} blocks earlier`,
      };
    }
  }
  return null;
}

function SelectionLine({ snapshot }: { snapshot: PipelineSnapshot | undefined }) {
  const pinned = usePipelineHoverStore((s) => s.pinned);
  const clearPin = usePipelineHoverStore((s) => s.clearPin);
  const sel = pinned && snapshot ? selectionText(pinned, snapshot) : null;
  return (
    <div
      data-testid="pipeline-selection"
      aria-live="polite"
      className="text-text mb-1.5 flex min-h-6 flex-wrap items-center gap-x-3 gap-y-1 font-mono text-[11px]"
    >
      {pinned ? (
        <>
          <span className="text-text-dim text-[10px] uppercase tracking-wider">Selected</span>
          <span className="text-text-bright">{shortHash(pinned)}</span>
          {sel ? (
            <>
              <span
                className={`rounded border px-1.5 py-[3px] text-[10px] leading-none ${TAG_TONE[sel.tag]}`}
              >
                {sel.tag}
              </span>
              <span>{sel.text}</span>
            </>
          ) : (
            <span>no longer in view</span>
          )}
          <Link href={`/tx/${pinned}`} className="text-aqua border-aqua/40 border-b no-underline">
            open tx ↗
          </Link>
          <button
            type="button"
            aria-label="Clear selection"
            onClick={clearPin}
            className="border-base-border text-text hover:text-text-bright rounded border px-[7px] py-[3px] text-[11px]"
          >
            ×
          </button>
        </>
      ) : (
        <>
          <span className="text-text-dim text-[10px] uppercase tracking-wider">Select</span>
          <span>{SELECT_HINT}</span>
        </>
      )}
    </div>
  );
}

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
        <SelectionLine snapshot={data} />
        <PipelineLegend />
      </TerminalPanelFooter>
    </TerminalPanel>
  );
}
