'use client';

import { type ReactElement, type ReactNode } from 'react';
import Link from '@/components/ui/link';
import { ErrorBoundary } from '@/components/error-boundary';
import {
  getObjectDetailHref,
  getIdentityItemDetailHref,
  getTokenDetailHref,
} from '@/lib/detail-routes';
import { cn } from '@/lib/utils';
import { shortenHash, type ShortSize } from '@/lib/shorten';
import { formatTokenBalanceWithRawMarker } from '@/lib/format-asset';
import { classifyActivity, type ActivityBody } from '@/lib/activity-classify';
import { MINUS } from '@/lib/chart-ticks';
import { protocolActionMeta } from '@/lib/protocol-action-meta';
import type {
  ItemDelta,
  ActivityTypeCall,
  ActivityLockCall,
  ActivityProtocolAction,
} from '@/lib/api';
import { formatBlockNumber } from '@/lib/format-block';
import { ClassMark } from '@/components/ui/class-mark';
import { CompositionBadge } from '@/components/ui/composition-mark';
import type { AssetClass } from '@/lib/design-tokens';
import { ShortId } from '@/components/ui/short-id';
import { OutpointIndex } from '@/components/ui/outpoint-index';
import { ExactCkb } from '@/components/ui/ckb-amount';

// ---------------------------------------------------------------------------
// Shared helpers (extracted from latest-activities.tsx)
// ---------------------------------------------------------------------------

export function formatStandard(standard: string): string {
  if (standard === 'xudt') return 'xUDT';
  if (standard === 'xudt_compatible') return 'xUDT-compatible';
  if (standard === 'sudt') return 'sUDT';
  if (standard === 'spore') return 'Spore';
  if (standard === 'm-nft') return 'm-NFT'; // F2 objects (IV-11): the protocol's own spelling
  if (standard === 'dotbit') return '.bit';
  if (standard === 'bit_cell') return '.bit Cell';
  if (standard === 'dotcell') return '.cell';
  if (standard === 'did_ckb') return 'did:ckb';
  return standard.charAt(0).toUpperCase() + standard.slice(1);
}

/**
 * A call's script as a reader names it: the registry name, else its code hash (what kind of
 * script it is), never the script hash, which reads as `code(args)` (F2-tx, TX-31).
 */
export function formatScriptRef(
  sc: { codeHash: string; scriptName?: string },
  size: ShortSize
): string {
  if (sc.scriptName?.trim()) return sc.scriptName!.trim();
  return shortenHash(sc.codeHash, size);
}

/** The label of a DAO step, as its badge and the compact summary both print it. */
function daoActionLabel(action: string): string {
  switch (action) {
    case 'deposit':
      return 'DAO Deposit';
    case 'withdraw_request':
      return 'DAO Withdraw Request';
    case 'withdraw_complete':
      return 'DAO Withdraw Complete';
    default:
      return 'DAO';
  }
}

function formatProtocolAction(action: string): string {
  return action.replace(/_/g, ' ');
}

const FIBER_ACTION_LABELS: Record<string, string> = {
  channel_open: 'Channel Open',
  channel_close: 'Channel Close',
  force_close: 'Force Close',
  settlement: 'Settlement',
};

function getFiberActionLabel(action: string): string {
  return FIBER_ACTION_LABELS[action] ?? formatProtocolAction(action);
}

/** The label of a protocol action (`DAO Deposit`, `Fiber · Channel Open`, `UTXOSwap · swap`). */
/**
 * A `.cell` action in words. `ring_relink` is the indexer's own record of a ring pointer rewrite
 * nothing else in the tx explains; it names the neighbour whose pointer changed when known.
 */
export function dotcellActionLabel(action: string, name: string | null = null): string {
  if (action === 'ring_relink')
    return name ? `ring pointer changed for ${name}` : 'ring pointer changed';
  return formatProtocolAction(action);
}

export function protocolActionLabel(pa: ActivityProtocolAction): string {
  if (pa.protocol === 'dao') return daoActionLabel(pa.action);
  if (pa.protocol === 'dotcell') {
    const name = pa.metadata?.name;
    return `${pa.protocolName} \u00B7 ${dotcellActionLabel(pa.action, typeof name === 'string' ? name : null)}`;
  }
  if (pa.protocol === 'rgbpp') {
    const labels: Record<string, string> = {
      leap_to_btc: 'Leap to Bitcoin',
      btc_time_locked: 'Bitcoin time lock',
      leap_to_ckb: 'Leap to CKB',
      transfer: 'Transfer',
      receive: 'Receive',
    };
    return `${pa.protocolName} · ${labels[pa.action] ?? formatProtocolAction(pa.action)}`;
  }
  const actionLabel =
    pa.protocol === 'fiber' ? getFiberActionLabel(pa.action) : formatProtocolAction(pa.action);
  return `${pa.protocolName} \u00B7 ${actionLabel}`;
}

/** An activity part that cannot be read, said in its row with the tx it belongs to. */
export function ActivityReadError({ txHash, error }: { txHash: string; error: unknown }) {
  const message = error instanceof Error ? error.message : String(error);
  return (
    <span
      role="alert"
      data-testid="activity-read-error"
      className="text-rouge text-small block whitespace-normal font-mono [overflow-wrap:anywhere]"
    >
      {`Cannot read tx ${txHash}: ${message}`}
    </span>
  );
}

/** Calls `render` while React renders it, so what it throws lands in the boundary around it. */
function RenderedInBoundary({ render }: { render: () => ReactNode }) {
  return <>{render()}</>;
}

/**
 * A part the page cannot read is said in place, with the transaction it belongs to (review 45fd
 * ruling D7): `render` runs during React's render inside an error boundary keyed by the
 * transaction, so whatever it computes and every component it returns fail here, as an
 * ActivityReadError, instead of reaching the page-level boundary. Adds no element of its own.
 */
export function readable(txHash: string, render: () => ReactNode): ReactElement {
  return (
    <ErrorBoundary
      key={txHash}
      fallbackRender={(error) => <ActivityReadError txHash={txHash} error={error} />}
    >
      <RenderedInBoundary render={render} />
    </ErrorBoundary>
  );
}

/** `Cellbase` for a cellbase (the site's word), `CKB Transfer` when CKB is all the tx moves, else `CKB`. */
export function ckbEventLabel(isCellbase: boolean, isPrimaryAction: boolean): string {
  return isCellbase ? 'Cellbase' : isPrimaryAction ? 'CKB Transfer' : 'CKB';
}

// ---------------------------------------------------------------------------
// Global activity helpers — tx-centric layered view for multi-participant
// ---------------------------------------------------------------------------

/** One tx-level event of a global activity: an L3 protocol action or an L2 type / lock call. */
export type TxEvent =
  | { kind: 'protocol'; action: ActivityProtocolAction }
  | { kind: 'type'; call: ActivityTypeCall }
  | { kind: 'lock'; call: ActivityLockCall };

/**
 * The tx-level events of a GlobalActivity, in display order: L3 protocol actions, then the L2
 * catch-all type calls, then the lock calls a protocol action does not already explain. The one
 * classification both the full event rows and the compact summary render.
 */
export function globalTxEvents(activity: ActivityBody): TxEvent[] {
  const events: TxEvent[] = activity.protocolActions.map((action) => ({
    kind: 'protocol' as const,
    action,
  }));
  for (const call of activity.typeCalls) events.push({ kind: 'type', call });
  const protocolNames = new Set(
    activity.protocolActions.map((pa: ActivityProtocolAction) => pa.protocol)
  );
  for (const call of activity.lockCalls) {
    const decoded = call.decoded?.protocol as string | undefined;
    if (decoded && protocolNames.has(decoded)) continue;
    events.push({ kind: 'lock', call });
  }
  return events;
}

/** Net change of the same item across every participant decides mint/burn/transfer. */
function itemMovement(activity: ActivityBody, primary: ItemDelta | null): string {
  if (!primary)
    throw new Error(
      `transaction ${activity.txHash ?? 'uncommitted'}: item headline has no item delta`
    );
  const sameItem = (item: ItemDelta) => {
    if (primary.kind === 'token')
      return item.kind === 'token' && item.typeScriptHash === primary.typeScriptHash;
    if (primary.kind === 'object')
      return item.kind === 'object' && item.objectId === primary.objectId;
    return (
      item.kind === 'identity' &&
      item.identityId === primary.identityId &&
      item.standard === primary.standard
    );
  };
  let net = BigInt(0);
  for (const participant of activity.participants)
    for (const item of participant.itemDeltas) {
      if (!sameItem(item)) continue;
      const delta = String(item.delta);
      if (!/^-?\d+$/.test(delta))
        throw new Error(
          `transaction ${activity.txHash ?? 'uncommitted'}: invalid ${item.kind} delta ${delta}`
        );
      net += BigInt(delta);
    }
  return net > BigInt(0) ? 'mint' : net < BigInt(0) ? 'burn' : 'transfer';
}

/** The asset class a feed headline is about, or null for CKB transfers and script calls. */
export function activityClassMark(activity: ActivityBody): AssetClass | null {
  const displayType = classifyActivity(activity).displayType;
  return displayType.startsWith('dao')
    ? 'dao'
    : displayType === 'token'
      ? 'tokens'
      : displayType === 'object'
        ? 'objects'
        : displayType === 'identity'
          ? 'identities'
          : null;
}

/**
 * The Layer-3 narrative of a global activity, as the feed heads it: the classifier picks the
 * highest non-empty layer, and the existing labels name it.
 */
export function activityHeadline(activity: ActivityBody): string {
  const c = classifyActivity(activity);
  switch (c.displayType) {
    case 'daoDeposit':
    case 'daoWithdrawRequest':
    case 'daoWithdrawComplete':
    case 'protocolAction':
      return protocolActionLabel(c.primaryProtocolAction as ActivityProtocolAction);
    case 'token':
      return `Token ${itemMovement(activity, c.primaryItemDelta)}`;
    case 'object':
      return `Object ${itemMovement(activity, c.primaryItemDelta)}`;
    case 'identity':
      return `Identity ${itemMovement(activity, c.primaryItemDelta)}`;
    case 'typeCall':
      return 'Script call';
    case 'ckbTransfer':
      return ckbEventLabel(activity.isCellbase, true);
  }
}

/**
 * A participant's Layer-2 change as a fact: `±amount SYMBOL`, or `<id> arrived / departed`.
 * `tone="sign"` colours a token amount by its sign (the Address board, and the T7 activity rows);
 * the default draws it bright. `symbol="dim"` (T7) draws the symbol dim beside the amount, as the
 * unit beside an exact CKB amount; the default keeps it in the amount's ink.
 * `showComposition={false}` (consistency C3) leaves an object's composition badge to its caller,
 * which says a tier shared by a whole participant line once; the default draws it per object.
 * `size` is the size of the row the fact stands in, for an id it writes (U-09).
 */
export function ItemDeltaFact({
  item,
  size,
  tone = 'plain',
  symbol = 'same',
  showComposition = true,
}: {
  item: ItemDelta;
  size: ShortSize;
  tone?: 'plain' | 'sign';
  symbol?: 'same' | 'dim';
  showComposition?: boolean;
}) {
  switch (item.kind) {
    case 'token': {
      const delta = BigInt(item.delta);
      const sign = delta > BigInt(0) ? '+' : delta < BigInt(0) ? MINUS : '';
      const abs = item.delta.startsWith('-') ? item.delta.slice(1) : item.delta;
      const label = item.symbol?.trim() || (
        <ShortId value={item.typeScriptHash} kind="hash" size={size} />
      );
      return (
        <span className="inline-flex items-baseline gap-1.5">
          <ClassMark kind="tokens" size={12} className="self-center" />
          <Link
            href={getTokenDetailHref(item.typeScriptHash)}
            className={cn(
              'hover:text-jade tabular-nums transition-colors',
              tone === 'plain'
                ? 'text-text-bright'
                : delta > BigInt(0)
                  ? 'text-positive'
                  : delta < BigInt(0)
                    ? 'text-negative'
                    : 'text-text-bright'
            )}
            onClick={(e) => e.stopPropagation()}
          >
            {sign}
            {formatTokenBalanceWithRawMarker(abs, item.decimals ?? null)}{' '}
            {symbol === 'dim' ? <span className="text-text-dim">{label}</span> : label}
          </Link>
        </span>
      );
    }
    case 'object': {
      const collectionName = item.collection?.name?.trim();
      return (
        <span className="text-text-bright">
          <ClassMark kind="objects" size={12} className="mr-1.5" />
          {/* the id is the unit (ShortId); the collection's name is words (A4 Chrome sweep) */}
          {collectionName ? `${collectionName} ` : 'object '}
          <Link
            href={getObjectDetailHref(item.objectId)}
            className="hover:text-jade transition-colors"
            onClick={(e) => e.stopPropagation()}
          >
            <ShortId value={item.objectId} kind="hash" size={size} />
          </Link>{' '}
          <span className="text-text-dim">{item.delta > 0 ? 'arrived' : 'departed'}</span>
          {showComposition && item.composition && (
            <CompositionBadge tier={item.composition} size="sm" className="ml-1.5 align-middle" />
          )}
        </span>
      );
    }
    case 'identity':
      return (
        <span className="text-text-bright">
          <ClassMark kind="identities" size={12} className="mr-1.5" />
          {/* the id is the unit (ShortId); the standard's name is words (A4 Chrome sweep) */}
          {`${formatStandard(item.standard)} `}
          <Link
            href={getIdentityItemDetailHref(item.standard, item.identityId)}
            className="hover:text-jade transition-colors"
            onClick={(e) => e.stopPropagation()}
          >
            <ShortId value={item.identityId} kind="hash" size={size} />
          </Link>{' '}
          <span className="text-text-dim">{item.delta > 0 ? 'arrived' : 'departed'}</span>
        </span>
      );
  }
}

/**
 * One fact of a protocol action's note, with what a list lays out (lane F follow-up 2): the
 * capacity as shannons (its column is decimal-aligned) and the deposit block as a height (a link
 * there); every other fact is its node. `protocolActionNoteParts` is these nodes, in this order.
 */
export type ProtocolNoteFact =
  | { kind: 'capacity'; shannons: bigint; suffix: string; node: React.ReactNode }
  | { kind: 'depositBlock'; block: bigint; node: React.ReactNode }
  | { kind: 'other'; node: React.ReactNode };

/**
 * What a protocol action's metadata says, as quiet parts of one line: DAO amounts and the deposit
 * block, a Fiber channel and its capacity, an RGB++ BTC txid. Empty when the action carries none.
 */
export function protocolActionNoteFacts(
  pa: ActivityProtocolAction,
  size: ShortSize
): ProtocolNoteFact[] {
  const meta = protocolActionMeta(pa);
  const facts: ProtocolNoteFact[] = [];
  if (meta.channelOutpoint) {
    // `0x<funding tx hash>:<index>`: shorten the hash, keep the index whole. The out point names
    // the channel's funding cell, so it opens that cell's page.
    const [hash, index] = meta.channelOutpoint.split(':');
    facts.push({
      kind: 'other',
      node:
        index === undefined ? (
          <>
            channel <ShortId value={hash} kind="hash" size={size} />
          </>
        ) : (
          <>
            channel{' '}
            <Link
              href={`/cell/${hash}-${index}`}
              className="hover:text-jade transition-colors"
              onClick={(e) => e.stopPropagation()}
            >
              <ShortId value={hash} kind="hash" size={size} />
              <OutpointIndex index={index} />
            </Link>
          </>
        ),
    });
  }
  if (meta.capacity !== null) {
    const suffix = pa.protocol === 'dao' && pa.action === 'deposit' ? ' locked' : '';
    facts.push({
      kind: 'capacity',
      shannons: meta.capacity,
      suffix,
      node: (
        <>
          <ExactCkb shannons={meta.capacity} signed={false} tone="plain" />
          {suffix}
        </>
      ),
    });
  }
  if (meta.depositBlock !== null) {
    facts.push({
      kind: 'depositBlock',
      block: meta.depositBlock,
      // Lane L: the deposit block is a link wherever the note is read.
      node: (
        <>
          deposited at{' '}
          <Link
            href={`/blocks/${meta.depositBlock}`}
            className="hover:text-jade transition-colors"
            onClick={(e) => e.stopPropagation()}
          >
            {formatBlockNumber(meta.depositBlock)}
          </Link>
        </>
      ),
    });
  }
  if (meta.compensation !== null) {
    facts.push({
      kind: 'other',
      node: (
        <>
          +<ExactCkb shannons={meta.compensation} signed={false} tone="plain" /> compensation
        </>
      ),
    });
  }
  if (pa.protocol === 'rgbpp') {
    const transitions: Record<string, string> = {
      leap_to_btc: 'CKB lock → Bitcoin-controlled lock',
      btc_time_locked: 'RGB++ lock → Bitcoin time lock',
      leap_to_ckb: 'Bitcoin-linked lock → CKB lock',
      transfer: 'Bitcoin-controlled cells change ownership',
      receive: 'New Bitcoin-controlled cells',
    };
    if (transitions[pa.action]) facts.push({ kind: 'other', node: transitions[pa.action] });
  }
  if (meta.btcTxid)
    facts.push({
      kind: 'other',
      node: (
        <span className="text-copper" title={meta.btcTxid}>
          Bitcoin tx <ShortId value={meta.btcTxid} kind="hash" size={size} />
          {meta.btcOutputIndex !== null && <OutpointIndex index={meta.btcOutputIndex.toString()} />}
        </span>
      ),
    });
  return facts;
}

export function protocolActionNoteParts(
  pa: ActivityProtocolAction,
  size: ShortSize
): React.ReactNode[] {
  return protocolActionNoteFacts(pa, size).map((f) => f.node);
}
