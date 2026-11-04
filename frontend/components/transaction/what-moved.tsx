'use client';
import { useState, type Ref } from 'react';
import Link from '@/components/ui/link';
import { ClassMark } from '@/components/ui/class-mark';
import { CompositionBadge } from '@/components/ui/composition-mark';
import { LockKind } from '@/components/ui/lock-kind';
import { TapOrHoverHint } from '@/components/ui/tap-or-hover-hint';
import { TerminalPanel } from '@/components/ui/terminal-panel';
import { ExactCkb, formatStandard } from '@/components/activity-event-row';
import { ParticipantRefView } from '@/components/participant-ref';
import { useElementWidth } from '@/hooks/useElementWidth';
import type { GlobalActivity, ParticipantInfo } from '@/lib/api';
import { FLOW_COLORS, type AssetClass } from '@/lib/design-tokens';
import {
  getIdentityItemDetailHref,
  getObjectDetailHref,
  getTokenDetailHref,
} from '@/lib/detail-routes';
import { formatTokenBalanceWithRawMarker } from '@/lib/format-asset';
import { formatCount } from '@/lib/format-count';
import {
  foldEnds,
  ribbonWeight,
  txFlows,
  type FlowAsset,
  type FlowEnd,
  type TxFlow,
} from '@/lib/tx-flows';
import { cn, truncateHash } from '@/lib/utils';

/** What the page lights in the cells diagram when a flow's party or asset is hovered. */
export type WhatMovedHover =
  | { t: 'party'; party: ParticipantInfo }
  | { t: 'asset'; asset: FlowAsset };

/** The ribbon form needs this much width; narrower, each flow is a list (RESPONSIVE_LAYOUT rule 4). */
export const RIBBON_FORM_MIN_W = 700;
/** A party end: two 18 px lines; the pitch leaves 10 px between neighbours. */
const NODE_H = 40;
const PITCH = 50;
/** The ribbons stack on the name plate 3 px apart. */
const STACK_GAP = 3;

function markOf(asset: FlowAsset): AssetClass {
  return asset.kind === 'token'
    ? 'tokens'
    : asset.kind === 'object'
      ? 'objects'
      : asset.kind === 'identity'
        ? 'identities'
        : 'ckb';
}

function colorOf(asset: FlowAsset): string {
  switch (asset.kind) {
    case 'ckb':
      return FLOW_COLORS.ckb;
    case 'token':
      return FLOW_COLORS.tokens;
    case 'object':
      return FLOW_COLORS.objects;
    case 'identity':
      return FLOW_COLORS.identities;
  }
}

/** The ribbon's colour for one end: the asset's, the fee's grey, compensation's gold. */
function endColor(asset: FlowAsset, end: FlowEnd): string {
  return end.t === 'miner'
    ? FLOW_COLORS.fee
    : end.t === 'compensation'
      ? FLOW_COLORS.compensation
      : colorOf(asset);
}

/** A faint ribbon for what is folded away or has no party on chain; the fee's is dashed. */
function endStroke(end: FlowEnd): { opacity: number | null; dashed: boolean } {
  return {
    opacity: end.t === 'more' || end.t === 'burnt' || end.t === 'minted' ? 0.28 : null,
    dashed: end.t === 'miner',
  };
}

/** The asset's name, for the plate and for a reader. */
function assetName(asset: FlowAsset): string {
  switch (asset.kind) {
    case 'ckb':
      return 'CKB';
    case 'token':
      return asset.item.symbol?.trim() || truncateHash(asset.item.typeScriptHash, 8, 6);
    case 'object':
      return asset.item.collection?.name?.trim() || 'Object';
    case 'identity':
      return `${formatStandard(asset.item.standard)} ${truncateHash(asset.item.identityId, 8, 4)}`;
  }
}

function assetHref(asset: FlowAsset): string | null {
  switch (asset.kind) {
    case 'ckb':
      return null;
    case 'token':
      return getTokenDetailHref(asset.item.typeScriptHash);
    case 'object':
      return getObjectDetailHref(asset.item.objectId);
    case 'identity':
      return getIdentityItemDetailHref(asset.item.standard, asset.item.identityId);
  }
}

/** An end's exact amount in the asset's unit; an object or identity moves whole, so none. */
function Amount({ asset, amount }: { asset: FlowAsset; amount: bigint }) {
  if (asset.kind === 'ckb') return <ExactCkb shannons={amount} signed={false} tone="plain" />;
  if (asset.kind !== 'token') return null;
  const decimals = asset.item.decimals ?? null;
  // A token without known decimals is written in raw units and marked so; its name stays on the
  // plate, so a long hash never wraps a 40 px end onto a second line.
  return (
    <span className="text-text-bright tabular-nums">
      {formatTokenBalanceWithRawMarker(amount.toString(), decimals)}
      {decimals !== null && (
        <>
          {' '}
          <span className="text-text-dim text-meta">{assetName(asset)}</span>
        </>
      )}
    </span>
  );
}

/** The words of a terminal end: who or what stands there, and its small-caps role. */
function terminalWords(end: FlowEnd): { who: string; role: string } {
  switch (end.t) {
    case 'miner':
      return { who: 'the miner', role: 'fee' };
    case 'compensation':
      return { who: 'Nervos DAO', role: 'compensation' };
    case 'genesis':
      return { who: 'the genesis block', role: 'issued' };
    case 'minted':
      return { who: 'no holder before', role: 'minted' };
    case 'burnt':
      return { who: 'no holder after', role: 'burnt' };
    case 'more':
      return { who: `${formatCount(end.count, 'more party', 'more parties')}`, role: 'expand' };
    case 'party':
      throw new Error('a party is not a terminal');
  }
}

/**
 * One end of a flow as text: a party (its address and lock kind, then its amount), or a terminal.
 * The `more` end is a button that unfolds its side.
 */
function EndText({
  asset,
  end,
  lockCodeHashes,
  align,
  onExpand,
}: {
  asset: FlowAsset;
  end: FlowEnd;
  lockCodeHashes: ReadonlyMap<string, string>;
  align: 'left' | 'right';
  onExpand?: () => void;
}) {
  const alignClass = align === 'right' ? 'text-right' : 'text-left';
  if (end.t === 'party') {
    const address = end.party.address;
    const codeHash = address === null ? null : (lockCodeHashes.get(address) ?? null);
    return (
      <div className={cn('min-w-0 leading-[18px]', alignClass)}>
        {/* One line, never two: the address whole, the lock kind truncated if the column is
            narrow (its hover carries the code hash). */}
        <div
          className={cn(
            'flex min-w-0 items-baseline gap-x-1.5 whitespace-nowrap',
            align === 'right' && 'justify-end'
          )}
        >
          <ParticipantRefView participant={end.party} compact quiet className="shrink-0" />
          {codeHash !== null && (
            <span className="min-w-0 truncate">
              <LockKind lockKind={end.party.lockKind ?? null} lockCodeHash={codeHash} />
            </span>
          )}
        </div>
        <div className="text-small">
          <Amount asset={asset} amount={end.amount} />
        </div>
      </div>
    );
  }
  const { who, role } = terminalWords(end);
  const body = (
    <>
      <div className="whitespace-nowrap">
        <span className={end.t === 'compensation' ? 'text-gold' : 'text-text'}>{who}</span>{' '}
        <span className="text-text-dim small-caps lowercase">{role}</span>
      </div>
      <div className="text-small">
        <Amount asset={asset} amount={end.amount} />
      </div>
    </>
  );
  if (end.t === 'more')
    return (
      <button
        type="button"
        onClick={onExpand}
        aria-expanded={false}
        className={cn('hover:text-jade block min-w-0 leading-[18px]', alignClass)}
      >
        {body}
      </button>
    );
  return <div className={cn('min-w-0 leading-[18px]', alignClass)}>{body}</div>;
}

/** The asset's name plate: its mark and name (a link to its page), an object's composition and id. */
function Plate({
  asset,
  flow,
  plateRef,
}: {
  asset: FlowAsset;
  flow: TxFlow;
  plateRef?: Ref<HTMLDivElement>;
}) {
  const href = assetHref(asset);
  const name = assetName(asset);
  const parties = (ends: readonly FlowEnd[]) => ends.filter((e) => e.t === 'party').length;
  const showTotal =
    (asset.kind === 'ckb' || asset.kind === 'token') &&
    (parties(flow.from) >= 2 || parties(flow.to) >= 2);
  return (
    <div
      ref={plateRef}
      data-flow-plate
      className="bg-base-bg/95 text-small flex max-w-full flex-col items-center gap-0.5 whitespace-nowrap rounded border px-3 py-1.5"
      style={{ borderColor: colorOf(asset) }}
    >
      <span className="text-text-bright flex items-center gap-1.5 font-medium">
        <ClassMark kind={markOf(asset)} size={12} />
        {href ? (
          <Link href={href} className="hover:text-jade">
            {name}
          </Link>
        ) : (
          name
        )}
        {asset.kind === 'object' && asset.item.composition && (
          <CompositionBadge tier={asset.item.composition} size="sm" />
        )}
      </span>
      {asset.kind === 'object' && (
        <Link
          href={getObjectDetailHref(asset.item.objectId)}
          className="text-text-dim text-meta hover:text-jade"
        >
          {truncateHash(asset.item.objectId, 10, 6)}
        </Link>
      )}
      {showTotal && (
        <span className="text-meta" data-flow-total>
          <Amount asset={asset} amount={flow.total} /> <span className="text-text-dim">in all</span>
        </span>
      )}
    </div>
  );
}

interface Side {
  ends: FlowEnd[];
  weights: number[];
  /** The side's vertical offset: a lone end sits at the strip's middle. */
  offset: number;
}

function side(ends: FlowEnd[], height: number): Side {
  const total = ends.reduce((acc, e) => acc + e.amount, BigInt(0));
  return {
    ends,
    weights: ends.map((e) => ribbonWeight(e.amount, total)),
    offset: ends.length === 1 ? (height - NODE_H) / 2 : 0,
  };
}

/** The flow in its ribbon form: givers, the field with the ribbons and the plate, receivers. */
function RibbonStrip({
  flow,
  lockCodeHashes,
  onHover,
}: {
  flow: TxFlow;
  lockCodeHashes: ReadonlyMap<string, string>;
  onHover?: (hover: WhatMovedHover | null) => void;
}) {
  const [expanded, setExpanded] = useState<{ from: boolean; to: boolean }>({
    from: false,
    to: false,
  });
  const [fieldRef, fieldWidth] = useElementWidth<HTMLDivElement>();
  const [plateRef, plateWidth] = useElementWidth<HTMLDivElement>();
  const from = foldEnds(flow.from, expanded.from);
  const to = foldEnds(flow.to, expanded.to);
  const height = Math.max(from.length, to.length) * PITCH - (PITCH - NODE_H);
  const left = side(from, height);
  const right = side(to, height);
  const W = fieldWidth ?? 0;
  // The plate is measured in place; until then, or in a box too narrow, a nominal width.
  const pw = Math.min(plateWidth ?? 120, Math.max(0, W - 80));
  const plateLeft = (W - pw) / 2;
  const plateRight = plateLeft + pw;
  const stack = (s: Side) =>
    s.weights.reduce((acc, w) => acc + w, 0) + STACK_GAP * Math.max(0, s.weights.length - 1);
  const ribbons = (s: Side, towardsPlate: boolean) => {
    let y = height / 2 - stack(s) / 2;
    return s.ends.map((end, i) => {
      const w = s.weights[i];
      const yNode = s.offset + i * PITCH + NODE_H / 2;
      const yPlate = y + w / 2;
      y += w + STACK_GAP;
      const { opacity, dashed } = endStroke(end);
      const color = endColor(flow.asset, end);
      const x1 = towardsPlate ? 0 : plateRight;
      const x2 = towardsPlate ? plateLeft : W - 7;
      const yA = towardsPlate ? yNode : yPlate;
      const yB = towardsPlate ? yPlate : yNode;
      const cx = (x1 + x2) / 2;
      const key =
        end.t === 'party'
          ? `${end.t}:${end.party.lockHash ?? end.party.lockHashPrefix ?? i}`
          : end.t;
      return (
        <g key={key} data-ribbon={end.t}>
          <path
            d={`M ${x1} ${yA} C ${cx} ${yA} ${cx} ${yB} ${x2} ${yB}`}
            fill="none"
            stroke={color}
            strokeWidth={w}
            strokeOpacity={opacity ?? (w > 3 ? 0.5 : 0.8)}
            strokeDasharray={dashed ? '5 4' : undefined}
          />
          {!towardsPlate && (
            // the chevron lands on the receiver: the layout already says the direction
            <path
              d={`M ${x2 - 1} ${yNode - 5} L ${x2 + 6} ${yNode} L ${x2 - 1} ${yNode + 5} Z`}
              fill={color}
            />
          )}
        </g>
      );
    });
  };
  const column = (s: Side, which: 'from' | 'to') => (
    <div className="relative min-w-0" style={{ height }} data-flow-side={which}>
      {s.ends.map((end, i) => {
        const color = endColor(flow.asset, end);
        const terminal = end.t !== 'party';
        return (
          <div
            key={end.t === 'party' ? (end.party.address ?? end.party.lockHashPrefix ?? i) : end.t}
            data-flow-end={end.t}
            data-weight={s.weights[i]}
            className={cn(
              'absolute inset-x-0 flex items-stretch gap-3',
              which === 'from' ? 'flex-row-reverse' : 'flex-row'
            )}
            style={{ top: s.offset + i * PITCH, height: NODE_H }}
            onMouseEnter={() => end.t === 'party' && onHover?.({ t: 'party', party: end.party })}
            onMouseLeave={() => end.t === 'party' && onHover?.(null)}
            onFocus={() => end.t === 'party' && onHover?.({ t: 'party', party: end.party })}
            onBlur={() => end.t === 'party' && onHover?.(null)}
          >
            <span
              aria-hidden="true"
              className={cn('w-1 shrink-0 rounded-sm', terminal && 'border border-dashed')}
              style={terminal ? { borderColor: color } : { background: color, opacity: 0.9 }}
            />
            <div className="min-w-0 flex-1 py-0.5">
              <EndText
                asset={flow.asset}
                end={end}
                lockCodeHashes={lockCodeHashes}
                align={which === 'from' ? 'right' : 'left'}
                onExpand={() => setExpanded((e) => ({ ...e, [which]: true }))}
              />
            </div>
          </div>
        );
      })}
    </div>
  );
  return (
    <div
      role="group"
      aria-label={`${assetName(flow.asset)} from ${formatCount(from.length, 'party', 'parties')} to ${formatCount(to.length, 'party', 'parties')}`}
      data-flow={flow.key}
      data-form="ribbons"
      className="grid grid-cols-[3fr_minmax(140px,4fr)_3fr] gap-x-2"
    >
      {column(left, 'from')}
      <div ref={fieldRef} className="relative min-w-0" style={{ height }}>
        <svg aria-hidden="true" className="absolute inset-0 h-full w-full overflow-visible">
          {W > 0 && (
            <>
              {ribbons(left, true)}
              {ribbons(right, false)}
            </>
          )}
        </svg>
        <div
          className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2"
          onMouseEnter={() => onHover?.({ t: 'asset', asset: flow.asset })}
          onMouseLeave={() => onHover?.(null)}
        >
          <Plate asset={flow.asset} flow={flow} plateRef={plateRef} />
        </div>
      </div>
      {column(right, 'to')}
    </div>
  );
}

/** The flow in its list form (under 700 px): the givers, a step down through the plate, the receivers. */
function ListStrip({
  flow,
  lockCodeHashes,
  onHover,
}: {
  flow: TxFlow;
  lockCodeHashes: ReadonlyMap<string, string>;
  onHover?: (hover: WhatMovedHover | null) => void;
}) {
  const [expanded, setExpanded] = useState<{ from: boolean; to: boolean }>({
    from: false,
    to: false,
  });
  const list = (ends: FlowEnd[], which: 'from' | 'to') => (
    <ul data-flow-side={which} className="flex flex-col gap-1.5">
      {ends.map((end, i) => (
        <li
          key={end.t === 'party' ? (end.party.address ?? end.party.lockHashPrefix ?? i) : end.t}
          data-flow-end={end.t}
          className={cn('border-l-[3px] pl-2.5', end.t !== 'party' && 'border-dashed')}
          style={{ borderColor: endColor(flow.asset, end) }}
          onMouseEnter={() => end.t === 'party' && onHover?.({ t: 'party', party: end.party })}
          onMouseLeave={() => end.t === 'party' && onHover?.(null)}
        >
          <EndText
            asset={flow.asset}
            end={end}
            lockCodeHashes={lockCodeHashes}
            align="left"
            onExpand={() => setExpanded((e) => ({ ...e, [which]: true }))}
          />
        </li>
      ))}
    </ul>
  );
  const from = foldEnds(flow.from, expanded.from);
  const to = foldEnds(flow.to, expanded.to);
  return (
    <div
      role="group"
      aria-label={`${assetName(flow.asset)} from ${formatCount(from.length, 'party', 'parties')} to ${formatCount(to.length, 'party', 'parties')}`}
      data-flow={flow.key}
      data-form="list"
      className="flex flex-col gap-2"
    >
      <span className="eyebrow text-text-dim text-meta">from</span>
      {list(from, 'from')}
      <div className="flex items-center gap-2 py-0.5">
        <svg aria-hidden="true" width="12" height="22" viewBox="0 0 12 22" className="shrink-0">
          <path d="M6 0v15" stroke={colorOf(flow.asset)} strokeWidth="2" />
          <path d="M0 14l6 8 6-8z" fill={colorOf(flow.asset)} />
        </svg>
        <div
          onMouseEnter={() => onHover?.({ t: 'asset', asset: flow.asset })}
          onMouseLeave={() => onHover?.(null)}
        >
          <Plate asset={flow.asset} flow={flow} />
        </div>
      </div>
      <span className="eyebrow text-text-dim text-meta">to</span>
      {list(to, 'to')}
    </div>
  );
}

/**
 * What moved (review 2026-11-03 v2): the transaction's asset flows as one instrument, under the
 * summary sentence and above the ledger. Every asset the transaction moved is one flow from the
 * parties it came from, on the left, to the parties it went to, on the right, through the asset's
 * name plate; nothing flows right to left, so a swap is two flows with the same parties on opposite
 * sides. Ribbon weight is the end's share of its side; the fee leaves to the miner, Nervos DAO
 * compensation enters in gold. Under 700 px of measured width each flow is a list: from, a step
 * down through the plate, to. Hovering a party or a plate tells the page, which lights the cells.
 */
export function WhatMoved({
  hash,
  activity,
  fee,
  compensation,
  genesis,
  lockCodeHashes,
  onHover,
}: {
  hash: string;
  activity: GlobalActivity;
  fee: string;
  compensation: bigint | null;
  genesis: boolean;
  /** Address → lock code hash, from the transaction's cells (`lockCodeHashesByAddress`). */
  lockCodeHashes: ReadonlyMap<string, string>;
  onHover?: (hover: WhatMovedHover | null) => void;
}) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const flows = txFlows({ hash, activity, fee, compensation, genesis });
  const form: 'ribbons' | 'list' =
    width === null || width >= RIBBON_FORM_MIN_W ? 'ribbons' : 'list';
  const Strip = form === 'ribbons' ? RibbonStrip : ListStrip;
  return (
    <TerminalPanel aria-label="What moved" data-testid="tx-what-moved" data-form={form}>
      <div className="border-base-border flex items-center justify-between gap-3 border-b px-3 py-1.5">
        <span className="eyebrow text-text-dim text-small">from</span>
        <TapOrHoverHint what="a party or an asset" className="text-text-ghost text-meta" />
        <span className="eyebrow text-text-dim text-small">to</span>
      </div>
      <div ref={ref} className="divide-base-border-subtle flex min-w-0 flex-col divide-y px-3">
        {flows.map((flow) => (
          <div key={flow.key} className="py-4">
            <Strip flow={flow} lockCodeHashes={lockCodeHashes} onHover={onHover} />
          </div>
        ))}
      </div>
    </TerminalPanel>
  );
}
