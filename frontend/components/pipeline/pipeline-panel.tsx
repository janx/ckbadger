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
import { SectionHeading } from '@/components/home/section';
import { usePhosphor } from '@/hooks/usePhosphor';
import { usePipelineSnapshot } from '@/hooks/usePipelineSnapshot';
import { usePipelineHoverStore, type PipelinePin } from '@/hooks/usePipelineHoverStore';
import { usePrefersReducedMotion } from '@/hooks/usePrefersReducedMotion';
import type { PipelineBlock, PipelinePoolTx, PipelineProposal, PipelineSnapshot } from '@/lib/api';
import type { MotionCtx, MotionView } from '@/lib/pipeline/motion';
import { diffSnapshots, type Transition } from '@/lib/pipeline/transitions';
import { STAGE_BASE, mixColor, tok } from '@/lib/pipeline/encoding';
import {
  G,
  axisOf,
  buildScene,
  inWindow,
  pick,
  pickTargets,
  rewardTarget,
  windowLabel,
  type BlockSpan,
  type ChainSlotState,
  type Scene,
  type ScenePhase,
  type SceneSquare,
} from '@/lib/pipeline/layout';
import { BlockCard, type DimLevel } from '@/components/pipeline/block-card';
import { ChainLine } from '@/components/pipeline/chain-line';
import {
  ColumnBlock,
  ColumnNext,
  ColumnNextUnavailable,
  ColumnPool,
} from '@/components/pipeline/column-rows';
import { MotionSvg } from '@/components/pipeline/motion-svg';
import { NextCard, NextColumn } from '@/components/pipeline/next-card';
import { PipelineLegend } from '@/components/pipeline/pipeline-legend';
import { PoolZone } from '@/components/pipeline/pool-zone';
import Link, { useSiteLink } from '@/components/ui/link';
import {
  PipelineTooltip,
  blockFacts,
  blockTip,
  commitNote,
  fmtBytes,
  chipTip,
  nextTip,
  predictedFacts,
  predictedTip,
  txTip,
  uncleTip,
  windowTip,
  withOpen,
  type OpenPage,
  type TipContent,
  type TxInfo,
} from '@/components/pipeline/pipeline-tooltip';
import { fmtInt, FOOTER_ROW } from '@/components/pipeline/svg-text';
import { WindowLabel } from '@/components/pipeline/window-label';
import { formatBlockNumber } from '@/lib/format-block';
import { BlockLink } from '@/components/ui/block-link';
import { tapOrHover } from '@/components/ui/tap-or-hover-hint';
import { formatCount } from '@/lib/format-count';
import { ShortId } from '@/components/ui/short-id';

const SVG_LABEL = {
  row: 'One row of a transaction lifetime: the mempool and proposed pool wells, the predicted next block, and the chain of recent blocks as plates joined to their parents by one strap through their heads, gilt across the proposal window of the next block, each with its seal, its proposal dots and its committed transactions set in cuts, starting with the cellbase',
  column:
    'A transaction lifetime read top to bottom: the mempool and proposed pool wells, the predicted next block, then the tip and three older blocks as plates joined to their parents by one strap down through their seals, gilt across the proposal window of the next block, each with its seal, its proposal dots and a line of transactions set in cuts, starting with the cellbase',
} as const;

/**
 * Width available to the SVG, measured before the canvas mounts (null until then), so the first
 * frame is already laid out for the real width. The SVG is drawn 1:1 at that width: a row while two
 * whole blocks fit after NEXT, the column below that. It never scrolls and never scales.
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

/**
 * The heading's args (rubbing × Song print, phase 5, H1): `--tip #20,969,560`. The pool and
 * NEXT counts are on the canvas itself; the head names only the tip, a link to its block that
 * flares when a block lands (material pass v2, move 4), and a dash until the first snapshot.
 */
function PipelineTip({ snapshot }: { snapshot: PipelineSnapshot | undefined }) {
  const flare = usePhosphor(snapshot?.tip.number);
  return (
    <span data-testid="pipeline-tip" className="tabular-nums">
      {'--tip '}
      {snapshot ? <BlockLink number={snapshot.tip.number} className={flare} /> : '—'}
    </span>
  );
}

function StatusRow({ testId, tone, children }: { testId: string; tone: string; children: string }) {
  return (
    <div
      data-testid={testId}
      role={testId === 'pipeline-error' ? 'alert' : undefined}
      className={`text-body flex items-center px-3 font-mono ${tone}`}
      style={{ height: G.H }}
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
  const viewW = useMeasuredWidth(wrapRef);
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
    <div ref={wrapRef} data-testid="pipeline-canvas">
      {built === null ? (
        <div data-testid="pipeline-measuring" style={{ height: G.H }} />
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

/**
 * What lies under the pointer: a tx square of any stage (a cellbase too), a predicted tx on NEXT, a
 * proposal dot, a mined block (its plate, stone, number or row, a landing one too) or an uncle's
 * word on it, NEXT, the proposed well, or the window lane; null for anything else. The hover
 * explains it, and a click opens (a tap pins) what it opens (`opensOf`), all from this one reading.
 */
type PointerTarget =
  | { kind: 'tx'; hash: string; info: TxInfo }
  | { kind: 'ghost'; hash: string; size: number; pool: PipelinePoolTx | null }
  | { kind: 'chip'; block: PipelineBlock; proposal: PipelineProposal }
  | { kind: 'block'; block: PipelineBlock }
  | { kind: 'uncle'; block: PipelineBlock; index: number }
  | { kind: 'next' }
  | { kind: 'proposed-well' }
  | { kind: 'lane' };

/**
 * What a click on a target opens, which is what a tap pins: a tx or a predicted tx opens its
 * transaction; a proposal dot its transaction when the proposal's tx is known, else the block it
 * is drawn on; a block, or an uncle's word on it, that block. NEXT (not a block yet), the wells,
 * the lane and the empty canvas open nothing.
 */
function opensOf(t: PointerTarget | null): PipelinePin | null {
  switch (t?.kind) {
    case 'tx':
    case 'ghost':
      return { kind: 'tx', hash: t.hash };
    case 'chip':
      return t.proposal.txHash
        ? { kind: 'tx', hash: t.proposal.txHash }
        : { kind: 'block', number: t.block.number };
    case 'block':
    case 'uncle':
      return { kind: 'block', number: t.block.number };
    default:
      return null;
  }
}

/** The site path of what opens: its transaction's page or its block's. */
const hrefOf = (o: PipelinePin) => (o.kind === 'tx' ? `/tx/${o.hash}` : `/blocks/${o.number}`);
/** Its name in the closing sentence of a tip. */
const nounOf = (o: PipelinePin): OpenPage => (o.kind === 'tx' ? 'transaction' : 'block');
const samePin = (a: PipelinePin | null, b: PipelinePin) =>
  a !== null &&
  (a.kind === 'tx'
    ? b.kind === 'tx' && a.hash === b.hash
    : b.kind === 'block' && a.number === b.number);

/** A ring `pad` px around a mark's box (a square, a ghost), in canvas px. */
function Ring({
  q,
  pad,
  testId,
  dashed,
}: {
  q: { X: number; Y: number; s: number };
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
  const pin = usePipelineHoverStore((s) => s.pin);
  const clearPin = usePipelineHoverStore((s) => s.clearPin);
  const link = useSiteLink();
  // The pointer type of the gesture a click ends, as its pointerdown said ('' until one does).
  const downType = useRef('');
  const [lane, setLane] = useState(false);
  const [arcBlock, setArcBlock] = useState<number | null>(null);
  const [cursor, setCursor] = useState<'' | 'pointer' | 'help'>('');
  const [tip, setTip] = useState<TipContent | null>(null);

  // The animator reads the hover-dependent bits of the latest render through this.
  const live = useRef({
    anchor: 0,
    highlighted: null as string | null,
    pinned: null as string | null,
  });
  // A block's 1 px edge: while it is hovered (here or in Latest Blocks), and while a tap pins it.
  const motionView = useMemo<MotionView>(
    () => ({
      highlighted: (id) => live.current.highlighted === id || live.current.pinned === id,
      anchor: () => live.current.anchor,
    }),
    []
  );

  const { snapshot } = scene;
  const { consensus } = snapshot;
  const index = useMemo(() => txIndex(scene), [scene]);
  const targets = useMemo(() => pickTargets(scene), [scene]);
  const byNumber = useMemo(() => new Map(scene.cards.map((c) => [c.block.number, c])), [scene]);
  const axis = useMemo(() => axisOf(scene), [scene]);
  const squares = useMemo(() => {
    const out = new Map<string, AbsSquare>();
    const add = (q: SceneSquare, dx: number, dy: number) =>
      out.set(q.id, { X: dx + q.x, Y: dy + q.y, s: q.s, stage: q.stage });
    if (scene.pool)
      for (const q of [...scene.pool.pending.squares, ...scene.pool.proposed.squares]) add(q, 0, 0);
    for (const c of scene.cards) for (const q of c.squares) add(q, c.x, c.y);
    return out;
  }, [scene]);
  // Predicted txs on the NEXT card, by hash, in absolute coordinates.
  const ghostOfTx = useMemo(() => {
    const out = new Map<string, { X: number; Y: number; s: number; size: number }>();
    const next = scene.next;
    if (!next) return out;
    const sizes = new Map(next.next.txs.map((t) => [t.hash, t.size]));
    for (const g of next.ghosts) {
      if (g.id) {
        out.set(g.id, {
          X: next.x + g.x,
          Y: next.y + g.y,
          s: g.s,
          size: sizes.get(g.id) as number,
        });
      }
    }
    return out;
  }, [scene]);
  const chipOfTx = useMemo(() => {
    const out = new Map<string, { X: number; Y: number }>();
    for (const c of scene.cards) {
      for (const k of c.chips) {
        const h = k.proposal.txHash;
        if (h && !out.has(h)) out.set(h, { X: c.x + k.x, Y: c.y + k.y });
      }
    }
    return out;
  }, [scene]);

  const defaultH = scene.windowTip + 1;
  const anchor = windowBlock ?? defaultH;
  const highlighted = hoverBlock === null ? null : (byNumber.get(hoverBlock)?.id ?? null);
  live.current.anchor = anchor;
  live.current.highlighted = highlighted;
  live.current.pinned = pinned?.kind === 'block' ? (byNumber.get(pinned.number)?.id ?? null) : null;

  // Slots along the chain's direction: x and w in a row, a row's top and height in a column.
  const chainSlots: ChainSlotState[] = scene.cards.map((c) => ({
    number: c.block.number,
    ...axis.along(c),
    mined: true,
    hash: c.block.hash,
    parentHash: c.block.parentHash,
    plate: true,
  }));
  const nextSlot = scene.next ? axis.along(scene.next) : null;
  const windowSlots: BlockSpan[] =
    scene.next && nextSlot
      ? [{ number: scene.next.number, ...nextSlot, mined: false }, ...chainSlots]
      : chainSlots;
  const windowText = windowLabel(anchor, scene.windowTip, consensus);
  const windowWords = axis.lane(windowSlots, anchor, consensus, windowText).label;
  // The window's words are drawn only while something that has a window is hovered: NEXT, a block,
  // the proposed well or the lane itself. At rest (and under the pointer) the gilt strap says it.
  const showWindow = lane || windowBlock !== null;
  // The lane above the cards holds the window's words in a row; a column has no lane.
  const laneGeom = axis.lane(
    windowSlots,
    defaultH,
    consensus,
    windowLabel(defaultH, scene.windowTip, consensus)
  ).hit;
  const paths = axis.chainPaths(chainSlots, nextSlot, anchor, consensus);

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
  // The pinned tx's mark, where the readout under the canvas finds it: its square in a well or on
  // a plate, else (its pool tx not in the listed pool) its predicted ghost on NEXT.
  const pinnedMark =
    pinned?.kind === 'tx' && pinned.hash !== hoverTx
      ? (squares.get(pinned.hash) ?? ghostOfTx.get(pinned.hash) ?? null)
      : null;
  const chip = hoverTx ? (chipOfTx.get(hoverTx) ?? null) : null;
  const ghost = hoverTx ? (ghostOfTx.get(hoverTx) ?? null) : null;
  const onIds = useMemo(() => new Set(hovered && hoverTx ? [hoverTx] : []), [hovered, hoverTx]);

  // The reward link of a hovered cellbase, to the cellbase of the block it pays: dropping into it
  // when that block is on screen, else running off the right edge.
  const arc = (() => {
    if (arcBlock === null) return null;
    // The hovered block may have left the screen with a newer snapshot.
    const c = byNumber.get(arcBlock);
    if (!c) return null;
    const target = rewardTarget(scene.cards, c);
    if (target === null) return null;
    const cellbase = (k: typeof c) => {
      const q = squares.get(k.block.cellbase.hash);
      if (!q) throw new Error(`pipeline: the cellbase of #${k.block.number} is not drawn`);
      return centreOf(q);
    };
    const tail = axis.tail(chainSlots);
    if (!tail) throw new Error(`pipeline: #${c.block.number} is on screen but the chain is not`);
    return axis.rewardLink(cellbase(c), target === 'off' ? null : cellbase(target), tail);
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
      y: (e.clientY - r.top) * (scene.H / r.height),
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

  /** What lies under a pointer at `p` over `el`: the one reading of the hover and the click. */
  const targetAt = (p: { x: number; y: number }, el: Element): PointerTarget | null => {
    const hit = pick(targets, p.x, p.y);
    if (hit?.kind === 'tx') {
      const info = index.get(hit.id);
      if (!info) throw new Error(`pipeline: picked tx ${hit.id} is not in the snapshot`);
      return { kind: 'tx', hash: hit.id, info };
    }
    if (hit?.kind === 'ghost') {
      // A predicted tx: linked to its pool square by hash when the listed pool has it.
      const g = ghostOfTx.get(hit.id);
      if (!g) throw new Error(`pipeline: picked ghost ${hit.id} is not on the NEXT card`);
      const info = index.get(hit.id);
      return {
        kind: 'ghost',
        hash: hit.id,
        size: g.size,
        pool: info?.kind === 'proposed' ? info.tx : null,
      };
    }
    if (hit?.kind === 'chip') {
      const card = scene.cards.find((c) => c.id === hit.card);
      const k = card?.chips.find((x) => x.key === hit.key);
      if (!card || !k) throw new Error(`pipeline: picked chip ${hit.key} is not on screen`);
      return { kind: 'chip', block: card.block, proposal: k.proposal };
    }
    if (el.closest('[data-next]') && snapshot.next) return { kind: 'next' };
    if (el.closest('[data-pool-card="proposed"]')) return { kind: 'proposed-well' };
    const cardEl = el.closest('[data-card]');
    if (cardEl) {
      const n = Number(cardEl.getAttribute('data-number'));
      const card = byNumber.get(n);
      if (!card) throw new Error(`pipeline: card #${n} under the pointer is not in the scene`);
      const uncleEl = el.closest('[data-uncle]');
      return uncleEl
        ? { kind: 'uncle', block: card.block, index: Number(uncleEl.getAttribute('data-uncle')) }
        : { kind: 'block', block: card.block };
    }
    if (inLane(p)) return { kind: 'lane' };
    return null;
  };

  const onPointerMove = (e: ReactPointerEvent<SVGSVGElement>) => {
    pointer.current = { x: e.clientX, y: e.clientY };
    place();
    const t = targetAt(toLocal(e), e.target as Element);
    const opens = opensOf(t);
    let content: TipContent | null = null;
    switch (t?.kind) {
      case 'tx': {
        const { info } = t;
        const block =
          info.kind === 'committed' || info.kind === 'cellbase'
            ? info.block.number
            : info.kind === 'proposed'
              ? (info.tx.proposedIn as NonNullable<typeof info.tx.proposedIn>).block
              : null;
        setHover({
          block,
          tx: t.hash,
          window: info.kind === 'committed' ? info.block.number : null,
        });
        content = txTip(info, snapshot);
        break;
      }
      case 'ghost':
        setHover({ block: null, tx: t.hash, window: null });
        content = predictedTip({ hash: t.hash, size: t.size }, t.pool, snapshot);
        break;
      case 'chip':
        setHover({ block: t.block.number, tx: t.proposal.txHash, window: null });
        content = chipTip(t.block, t.proposal, snapshot);
        break;
      case 'next':
        // NEXT shows its own window: where the txs it can commit were proposed.
        clearHover();
        content = nextTip(snapshot);
        break;
      case 'proposed-well':
        // The proposed well holds exactly the proposals of NEXT's window (and the tip's, too early).
        clearHover();
        content = windowTip(defaultH, snapshot);
        break;
      case 'uncle':
        setHover({ block: t.block.number, tx: null, window: null });
        content = uncleTip(t.block, t.index);
        break;
      case 'block':
        setHover({ block: t.block.number, tx: null, window: t.block.number });
        content = blockTip(t.block, snapshot);
        break;
      case 'lane':
        clearHover();
        content = windowTip(defaultH, snapshot);
        break;
      default:
        clearHover();
    }
    show(content && withOpen(content, opens && nounOf(opens)));
    setLane(t?.kind === 'next' || t?.kind === 'proposed-well' || t?.kind === 'lane');
    setArcBlock(t?.kind === 'tx' && t.info.kind === 'cellbase' ? t.info.block.number : null);
    // A pointer exactly where a click opens a page; the lane explains, so it asks for help.
    setCursor(opens ? 'pointer' : t?.kind === 'lane' ? 'help' : '');
  };
  const onPointerLeave = () => {
    clearHover();
    setLane(false);
    setArcBlock(null);
    setCursor('');
    show(null);
  };
  /**
   * Open a page, through the router as the site's links do, or in a new tab (this one staying where
   * it is). Opening a page from the pipeline clears the pin, so it is gone when the reader returns.
   */
  const openPage = (o: PipelinePin, newTab: boolean) => {
    clearPin();
    const url = link.url(hrefOf(o));
    if (newTab) {
      window.open(url, '_blank', 'noopener,noreferrer');
      return;
    }
    if (!link.navigator) {
      throw new Error(`pipeline: a click opens ${url} through the router, and there is none`);
    }
    link.navigator.push(url);
  };
  const onPointerDown = (e: ReactPointerEvent<SVGSVGElement>) => {
    downType.current = e.pointerType;
  };
  /**
   * A pointer that hovers (a mouse, a pen, or a click no pointerdown began) opens what it clicks at
   * once, in a new tab with Ctrl or Meta held. A touch tap (the Touch rule: no hover to read the
   * tips by) pins what it would open, its readout under the canvas; a tap on what is pinned opens
   * it; a tap where nothing opens clears the pin.
   */
  const onClick = (e: ReactMouseEvent<SVGSVGElement>) => {
    const touch = downType.current === 'touch';
    downType.current = '';
    const opens = opensOf(targetAt(toLocal(e), e.target as Element));
    if (!touch) {
      if (opens) openPage(opens, e.ctrlKey || e.metaKey);
    } else if (opens === null) clearPin();
    else if (samePin(pinned, opens)) openPage(opens, false);
    else pin(opens);
  };
  /** A middle click opens what it clicks in a new tab. */
  const onAuxClick = (e: ReactMouseEvent<SVGSVGElement>) => {
    if (e.button !== 1) return;
    const opens = opensOf(targetAt(toLocal(e), e.target as Element));
    if (opens) openPage(opens, true);
  };

  const focus = hovered !== null;
  const column = scene.orientation === 'column';
  const nextDim: DimLevel =
    dimAnchor === null || (scene.next && dimAnchor === scene.next.number) ? 'false' : 'true';
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
            className: `pipeline-svg block w-full select-none font-mono tabular-nums${focus ? ' is-focus' : ''}`,
            // The canvas is drawn 1:1: its CSS height is the viewBox height and its width the
            // measured one, which picked the arrangement.
            style: cursor ? { height: scene.H, cursor } : { height: scene.H },
            viewBox: `0 0 ${scene.viewW} ${scene.H}`,
            role: 'img',
            'aria-label': SVG_LABEL[scene.orientation],
            onPointerMove,
            onPointerLeave,
            onPointerDown,
            onClick,
            onAuxClick,
          } as SVGProps<SVGSVGElement>
        }
      >
        <g pointerEvents="none">
          <ChainLine paths={paths} along={scene.orientation} dim={dimAnchor !== null} />
        </g>
        {column ? (
          <ColumnPool scene={scene} dimOf={poolDim} onIds={onIds} />
        ) : (
          <PoolZone scene={scene} dimOf={poolDim} onIds={onIds} />
        )}
        <g data-layer="cards">
          {scene.next &&
            (column ? (
              <ColumnNext next={scene.next} dim={nextDim} />
            ) : (
              <NextCard next={scene.next} dim={nextDim} />
            ))}
          {scene.cards.map((c, i) => {
            const Card = column ? ColumnBlock : BlockCard;
            return (
              <Card
                key={c.id}
                card={c}
                gilt={inWindow(c.block.number, anchor, consensus)}
                highlighted={motionView.highlighted(c.id)}
                dim={cardDim(c.block.number)}
                onIds={onIds}
                age={i}
              />
            );
          })}
        </g>
        <g data-layer="labels" pointerEvents="none">
          {scene.bands ? (
            <ColumnNextUnavailable available={snapshot.next !== null} band={scene.bands.next} />
          ) : (
            <NextColumn available={snapshot.next !== null} />
          )}
          {showWindow && (
            <WindowLabel
              label={windowWords}
              emphasis={lane}
              faded={arc !== null}
              text={windowText}
            />
          )}
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
          {arc?.tail && (
            <path
              data-hover="reward-tail"
              d={arc.tail}
              fill="none"
              strokeWidth={1}
              strokeOpacity={0.8}
              strokeDasharray="1 3"
              vectorEffect="non-scaling-stroke"
              style={{ stroke: tok('jade') }}
            />
          )}
          {chip && (
            <circle
              data-hover="chip-ring"
              cx={chip.X + G.chip / 2}
              cy={chip.Y + G.chip / 2}
              r={G.chip / 2 + 2}
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
                fill="none"
                strokeWidth={3}
                strokeOpacity={0.45}
                vectorEffect="non-scaling-stroke"
                style={{ stroke: mixColor(STAGE_BASE[hovered.stage], tok('step-to'), 0.4) }}
              />
              <Ring q={hovered} pad={2} testId="ring" />
            </>
          )}
          {pinnedMark && <Ring q={pinnedMark} pad={2.5} testId="pin-ring" dashed />}
          {ghost && <Ring q={ghost} pad={2} testId="ghost-ring" dashed />}
        </g>
      </MotionSvg>
      <PipelineTooltip ref={tipRef} content={tip} />
    </>
  );
}

/** The "?" beside the title: what the panel shows and how to use it, kept off the canvas. */
const PIPELINE_HELP = [
  'Two-step confirmation: a tx is proposed in one block and committed in a later block inside its proposal window.',
  'Square area = bytes; shade = cycles.',
  `${tapOrHover('a tx or a proposal', { capital: true })} to see how they connect; a click, or a second tap, opens the page of a block or a tx.`,
].join('\n');

const TAG_TONE = {
  pending: 'text-aqua border-aqua/50',
  proposed: 'text-gold border-gold/50',
  committed: 'text-jade border-jade/50',
  cellbase: 'text-jade border-jade/50',
  block: 'text-jade border-jade/50',
} as const;

/** A tip's facts said in one line: `label value · label value`. */
const factsLine = (facts: readonly [string, string][]) =>
  facts.map(([k, v]) => `${k} ${v}`).join(' · ');

/**
 * Where the pinned tx is now, in one line, read from the snapshot on screen: in the pool, on a
 * block, or found only among NEXT's predicted txs (its pool tx not in the listed pool, which is
 * truncated): then it is a proposed tx the next block is expected to commit, said in the facts
 * its tip opens with (`predictedFacts`). null when the snapshot has it nowhere.
 */
function selectionText(
  hash: string,
  s: PipelineSnapshot
): { tag: keyof typeof TAG_TONE; text: string } | null {
  const pending = s.pool?.pending.find((t) => t.hash === hash);
  if (pending) {
    return {
      tag: 'pending',
      text: `${fmtBytes(pending.size)} · fee rate ${fmtInt(BigInt(pending.feeRate))} sh/kB · waiting to be proposed by the next block`,
    };
  }
  const proposed = s.pool?.proposed.find((t) => t.hash === hash);
  if (proposed?.proposedIn) {
    const block = s.blocks.find((b) => b.number === proposed.proposedIn?.block);
    const state = block?.proposals.find((p) => p.txHash === hash)?.state;
    return {
      tag: 'proposed',
      text: `proposal in ${formatBlockNumber(proposed.proposedIn.block)} · can commit next? ${commitNote(proposed.proposedIn.block, state === 'early', s)}`,
    };
  }
  for (const b of s.blocks) {
    if (b.cellbase.hash === hash) {
      return {
        tag: 'cellbase',
        text: `first tx of ${formatBlockNumber(b.number)}${b.cellbase.pays ? ` · pays the reward of ${formatBlockNumber(b.cellbase.pays.number)}` : ''}`,
      };
    }
    const tx = b.txs.find((t) => t.hash === hash);
    if (tx) {
      return {
        tag: 'committed',
        text: `committed in ${formatBlockNumber(b.number)} · its proposal is in ${formatBlockNumber(tx.proposedIn.block)}, ${formatCount(b.number - tx.proposedIn.block, 'block')} earlier`,
      };
    }
  }
  const predicted = s.next?.txs.find((t) => t.hash === hash);
  if (predicted) return { tag: 'proposed', text: factsLine(predictedFacts(predicted, s)) };
  return null;
}

/** The pinned block in one line: the facts its tooltip opens with, while the snapshot lists it. */
function blockSelectionText(
  n: number,
  s: PipelineSnapshot
): { tag: keyof typeof TAG_TONE; text: string } | null {
  const b = s.blocks.find((x) => x.number === n);
  if (!b) return null;
  return { tag: 'block', text: factsLine(blockFacts(b)) };
}

/**
 * The line under the canvas: the legend, or — while a tap has pinned a tx or a block — where it
 * is now, a link that opens it and a button that clears it (the Touch rule's readout).
 */
function PipelineFooter({ snapshot }: { snapshot: PipelineSnapshot | undefined }) {
  const pinned = usePipelineHoverStore((s) => s.pinned);
  const clearPin = usePipelineHoverStore((s) => s.clearPin);
  if (!pinned) return <PipelineLegend />;
  const sel = !snapshot
    ? null
    : pinned.kind === 'tx'
      ? selectionText(pinned.hash, snapshot)
      : blockSelectionText(pinned.number, snapshot);
  return (
    <div
      data-testid="pipeline-selection"
      aria-live="polite"
      className={`${FOOTER_ROW} text-text text-meta gap-x-3`}
    >
      <span className="eyebrow text-text-dim text-meta">Selected</span>
      <span className="text-text-bright">
        {pinned.kind === 'tx' ? (
          <ShortId value={pinned.hash} kind="hash" size="row" />
        ) : (
          formatBlockNumber(pinned.number)
        )}
      </span>
      {sel ? (
        <>
          <span className={`text-meta border px-1.5 py-[3px] leading-none ${TAG_TONE[sel.tag]}`}>
            {sel.tag}
          </span>
          <span>{sel.text}</span>
        </>
      ) : (
        <span>no longer in view</span>
      )}
      <Link
        href={hrefOf(pinned)}
        onClick={clearPin}
        className="text-aqua border-aqua/40 border-b no-underline"
      >
        {pinned.kind === 'tx' ? 'open tx ↗' : 'open block ↗'}
      </Link>
      <button
        type="button"
        aria-label="Clear selection"
        onClick={clearPin}
        className="border-base-border text-text hover:text-text-bright text-meta border px-[7px] py-[3px]"
      >
        ×
      </button>
    </div>
  );
}

/**
 * Homepage transaction pipeline: mempool → next block → chain, from one backend snapshot. An
 * unframed section like the others on the home page (PlateHome.dc.html): the section's prompt
 * heading, the canvas right under it (its own 18 px lane is the gap), and a plain dim line under
 * the canvas for the legend, or for the pinned tx or block while one is pinned.
 */
export function PipelinePanel() {
  const { data, error } = usePipelineSnapshot();
  return (
    <section
      aria-label="Transaction pipeline"
      data-testid="pipeline-panel"
      className="flex flex-col"
    >
      <SectionHeading
        command={
          <span className="flex items-center gap-2.5">
            pipeline
            <span
              title={PIPELINE_HELP}
              className="border-text-dim/70 text-text-dim text-meta inline-block h-3.5 w-3.5 border text-center font-mono font-normal normal-case leading-3 tracking-normal"
            >
              ?
            </span>
          </span>
        }
        args={<PipelineTip snapshot={data} />}
        note={
          <Link
            href="/blocks"
            className="text-interactive hover:text-jade text-small font-mono transition-colors"
          >
            all blocks →
          </Link>
        }
      />
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
      <div className="mt-3.5">
        <PipelineFooter snapshot={data} />
      </div>
    </section>
  );
}
