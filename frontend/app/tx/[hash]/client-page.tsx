'use client';
import { Fragment, useEffect, useId, useMemo, useRef, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import {
  useParams,
  usePathname,
  useRouter,
  useSearchAndHash,
  useSearchParams,
} from '@/src/navigation';
import { NotFoundPage } from '@/components/not-found-page';
import Link from '@/components/ui/link';
import { DetailLayout, DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import { DetailSkeleton } from '@/components/ui/detail-skeleton';
import { HeadId } from '@/components/ui/head-id';
import { SealedHead } from '@/components/ui/page-seal';
import { Comment } from '@/components/ui/comment';
import { Prose } from '@/components/ui/prose';
import { FoldToggle } from '@/components/ui/fold-toggle';
import { HexMap } from '@/components/ui/hex-map';
import { ClassMark } from '@/components/ui/class-mark';
import {
  ActivityReadError,
  activityClassMark,
  activityHeadline,
  formatStandard,
  formatScriptRef,
  protocolActionNoteParts,
  readable,
} from '@/components/activity-event-row';
import { CkbAmount, TokenAmount } from '@/components/ui/ckb-amount';
import {
  daoStepsAttributed,
  flowKey,
  onlyFeeMoved,
  txFlows,
  type FlowEnd,
  type TxFlow,
} from '@/lib/tx-flows';
import { anchoredOutPoint } from '@/lib/tx-cell-link';
import { occupiedBytesDelta, occupiedPairText } from '@/lib/occupied';
import { ParticipantRefView } from '@/components/participant-ref';
import {
  FOLD_HEAD,
  TxStructure,
  continuityColor,
  type TxSealId,
} from '@/components/transaction/tx-structure';
import { txContinuity } from '@/lib/tx-continuity';
import {
  CellTilesError,
  readCellTiles,
  registeredName,
} from '@/components/transaction/transaction-cell-tiles';
import { ScriptGroupLegend, ScriptGroupMatrix } from '@/components/transaction/script-group-matrix';
import {
  groupCells,
  groupHref,
  groupKey,
  groupLabel,
  scriptGroupRows,
  witnessKindLabel,
} from '@/components/transaction/script-group-rows';
import { BalanceChanges } from '@/components/transaction/balance-changes';
import { WhatMoved, type WhatMovedHover } from '@/components/transaction/what-moved';
import { ProposalRuler } from '@/components/transaction/proposal-ruler';
import { DotCellRecordsTable } from '@/components/identity/dotcell-records-table';
import { poolStatusLabel } from '@/components/ui/pool-status';
import { useCyclesCalculation } from '@/hooks/useCyclesCalculation';
import { PartReadError, ReadFailure, readPart } from '@/components/read-failure';
import {
  api,
  isNotFoundApiError,
  isServiceUnavailableError,
  type TransactionDetail,
  type TransactionWitness,
  type ScriptLookupResponse,
  type GlobalActivity,
  type CellDep,
  type ItemDelta,
  type ParticipantInfo,
  TAG_DAO,
} from '@/lib/api';
import { BlockLink } from '@/components/ui/block-link';
import { isBadRequestApiError } from '@/lib/api-errors';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import { txComment } from '@/lib/tx-comment';
import {
  getIdentityItemDetailHref,
  getObjectDetailHref,
  getTokenDetailHref,
} from '@/lib/detail-routes';
import { cn } from '@/lib/utils';
import { shortenHash } from '@/lib/shorten';
import { protocolActionMeta } from '@/lib/protocol-action-meta';
import { CLASS_COLORS } from '@/lib/design-tokens';
import {
  capacityEquation,
  txLinks,
  txMovements,
  type TxMovement,
  type TxHover,
  type TxLinkSets,
} from '@/lib/tx-detail-model';
import dynamic from '@/lib/dynamic-client';
import { isFieldLayoutLock, lockArgsLine } from '@/lib/lock-args';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import { DocumentTitle, titleHash } from '@/lib/use-document-title';
import type { SealState } from '@/lib/seal/stones';
import { NoBreak } from '@/components/ui/no-break';
import { ShortId } from '@/components/ui/short-id';
import { OutpointIndex } from '@/components/ui/outpoint-index';
import { Utc8Age, Utc8DateTime } from '@/components/ui/utc8-date';
import type { StripFact } from '@/components/ui/status-strip';
import { unitNode, unitText } from '@/components/ui/units';
import { cellLockFacts, lockWords, lockWordsText } from '@/components/ui/lock-kind';

const DeferredCellGraph = dynamic(() => import('@/components/cell-graph'), {
  loading: () => <p className="text-text-dim text-small">Loading graph section…</p>,
});
const POLL_MS = 3000;
function provisional(tx: TransactionDetail | undefined): boolean {
  return (
    !!tx &&
    (tx.status === 'pending' ||
      tx.status === 'proposed' ||
      tx.poolStatus === 'committed_awaiting_index')
  );
}
function required<T>(value: T | null | undefined, hash: string, field: string): T {
  if (value === null || value === undefined)
    throw new Error(`Transaction ${hash} missing required field "${field}"`);
  return value;
}
function indexParam(value: string | null): number | null {
  if (value === null) return null;
  if (!/^\d+$/.test(value) || !Number.isSafeInteger(Number(value)))
    throw new Error(`Invalid witness index ${value}`);
  return Number(value);
}
/**
 * The fold's open state in the URL (lane D3): one parameter, `cells`, naming the sheets drawn
 * whole — `consumed`, `created` or `all` — and absent when both are folded. A value it does not
 * know is refused with the transaction named.
 */
const CELLS_PARAM = 'cells';
function cellsParam(value: string | null, hash: string): Record<'consumed' | 'created', boolean> {
  switch (value) {
    case null:
      return { consumed: false, created: false };
    case 'consumed':
      return { consumed: true, created: false };
    case 'created':
      return { consumed: false, created: true };
    case 'all':
      return { consumed: true, created: true };
    default:
      throw new Error(
        `Transaction ${hash}: ${CELLS_PARAM}=${value} is none of consumed, created, all`
      );
  }
}
function cellsValue(open: Record<'consumed' | 'created', boolean>): string | null {
  return open.consumed && open.created
    ? 'all'
    : open.consumed
      ? 'consumed'
      : open.created
        ? 'created'
        : null;
}
function PendingValue() {
  return <span className="text-text-dim">pending…</span>;
}
/** Total DAO compensation the transaction's completed withdrawals pay, from their metadata. */
function daoCompensation(activity: GlobalActivity): bigint | null {
  const completes = activity.protocolActions.filter(
    (pa) => pa.protocol === 'dao' && pa.action === 'withdraw_complete'
  );
  if (completes.length === 0) return null;
  return completes.reduce((sum, pa) => sum + protocolActionMeta(pa).compensation!, BigInt(0));
}

/**
 * `A`, `A and B`, `A, B and 3 more`: the parties on one side of a movement, in the summary under
 * the headline, the page's lede: prose takes the row form (R9; U-09).
 */
function Parties({ parties }: { parties: readonly ParticipantInfo[] }) {
  const shown = parties.slice(0, 2);
  return (
    <>
      {shown.map((p, i) => (
        <Fragment key={i}>
          {i > 0 && (parties.length > 2 ? ', ' : ' and ')}
          <ParticipantRefView participant={p} size="row" compact quiet look="inline" />
        </Fragment>
      ))}
      {parties.length > 2 && ` and ${parties.length - 2} more`}
    </>
  );
}

/**
 * An item as the summary names it: its class mark, a token's amount (lane P1: the summary form,
 * its exact value in its title and in the balance table) and a link, the line's own words
 * (`Nervape 3x 0x51c7a1b2…6f96f2`, `36,583 Seal`); an object and an identity move whole.
 */
function ItemName({ item, amount }: { item: ItemDelta; amount: bigint }) {
  switch (item.kind) {
    case 'object':
      return (
        <>
          <ClassMark kind="objects" size={12} className="mr-1.5" />
          <Link look="inline" href={getObjectDetailHref(item.objectId)}>
            {item.collection?.name ? `${item.collection.name} ` : 'Object '}
            <ShortId value={item.objectId} kind="hash" size="row" />
          </Link>
        </>
      );
    case 'token':
      return (
        <>
          <ClassMark kind="tokens" size={12} className="mr-1.5" />
          <TokenAmount
            form="summary"
            amount={amount.toString()}
            decimals={item.decimals ?? null}
            tone="plain"
          />{' '}
          <Link look="inline" href={getTokenDetailHref(item.typeScriptHash)}>
            {item.symbol?.trim() || <ShortId value={item.typeScriptHash} kind="hash" size="row" />}
          </Link>
        </>
      );
    case 'identity':
      return (
        <>
          <ClassMark kind="identities" size={12} className="mr-1.5" />
          <Link look="inline" href={getIdentityItemDetailHref(item.standard, item.identityId)}>
            {formatStandard(item.standard)}{' '}
            <ShortId value={item.identityId} kind="hash" size="row" />
          </Link>
        </>
      );
  }
}

/**
 * Whom a cellbase pays (lane P1): its outputs' owners by their addresses, each once, in the row
 * form a lede writes (R9), linked to the address page as a link inside the line. A cellbase that
 * pays a reward has a cell to pay it into, and that cell has an address: an output without one is
 * refused with the transaction named, in the lede's place.
 */
function CellbasePayees({ tx }: { tx: TransactionDetail }) {
  const outputs = required(tx.outputs, tx.hash, 'outputs');
  if (outputs.length === 0)
    throw new Error(`Cellbase ${tx.hash} pays a block's reward but creates no cell`);
  const payees = [
    ...new Set(outputs.map((o, i) => required(o.address, tx.hash, `output ${i} address`))),
  ];
  return (
    <>
      {payees.map((address, i) => (
        <Fragment key={address}>
          {i > 0 && (i === payees.length - 1 ? ' and ' : ', ')}
          <Link look="inline" href={`/address/${address}`} title={address}>
            <ShortId value={address} kind="address" size="row" />
          </Link>
        </Fragment>
      ))}
    </>
  );
}

/** `A`, `A and B`, `A, B and C`. */
function listWords(words: readonly string[]): string {
  return words.length <= 1
    ? (words[0] ?? '')
    : `${words.slice(0, -1).join(', ')} and ${words[words.length - 1]}`;
}

/**
 * The plain summary under the headline (TX-11): what moved from whom to whom and how much, read
 * off the participants' signed changes, or the protocol action's own facts, or the scripts it ran.
 * The fee is said once, in the status strip (lane D2, gate D0′): the sentence that repeated it
 * ("The sender paid a 0.00064119 CKB fee.") is gone. Lane P1: a movement states its amount from
 * the flows the What moved figure draws (`txFlows`; what reached the receiving parties, or what
 * left the giving ones where none received), in the summary form; a transaction that moved only
 * its fee says so in one sentence, and with that sentence the figure and a one-party table, which
 * would restate the fee alone, are not drawn (`onlyFeeMoved`).
 */
/**
 * A transaction's Nervos DAO steps in the lede, from the activity's own kind (lane P1 follow-up;
 * review chain-b tx-dao-deposit: the page drew a deposit as a payment): each kind of step once, with
 * its amount where the transaction takes one such step and its count where it takes several (the
 * API states no total, and the page sums none), and the parties the activity tags DAO, whose cells
 * hold the step. Compensation is the DAO's word, never interest.
 */
function DaoSentences({ activity }: { activity: GlobalActivity }) {
  const steps = activity.protocolActions.filter((pa) => pa.protocol === 'dao');
  if (steps.length === 0) return null;
  const holders = activity.participants.filter((p) => (p.tags & TAG_DAO) !== 0);
  if (holders.length === 0)
    throw new Error(`${activity.txHash}: a Nervos DAO step and no party the activity tags DAO`);
  // whose each step is, where the activity says it (lane P1, third round; review p-logic-chain
  // SHOULD 2): one party tagged DAO, or steps of one kind. Otherwise each step is said without a
  // party, and the parties whose cells took them are named once after them.
  const attributed = daoStepsAttributed(activity);
  const who = <Parties parties={holders} />;
  const amount = (shannons: bigint) => (
    <CkbAmount form="summary" shannons={shannons} signed={false} tone="plain" />
  );
  const kinds = [...new Set(steps.map((pa) => pa.action))];
  return (
    <>
      {kinds.map((kind) => {
        const of = steps.filter((pa) => pa.action === kind);
        const one = of.length === 1 ? protocolActionMeta(of[0]) : null;
        const count = of.length.toLocaleString('en-US');
        switch (kind) {
          case 'deposit':
            return one ? (
              <Fragment key={kind}>
                {amount(one.capacity!)} went into a Nervos DAO deposit
                {attributed && <> held by {who}</>}.{' '}
              </Fragment>
            ) : (
              <Fragment key={kind}>
                {count} Nervos DAO deposits were made{attributed && <>, held by {who}</>}.{' '}
              </Fragment>
            );
          case 'withdraw_request':
            if (!attributed)
              return one ? (
                <Fragment key={kind}>
                  A Nervos DAO deposit of {amount(one.capacity!)}, made at{' '}
                  <BlockLink look="inline" number={one.depositBlock!} />, was asked to be
                  withdrawn.{' '}
                </Fragment>
              ) : (
                <Fragment key={kind}>
                  {count} Nervos DAO deposits were asked to be withdrawn.{' '}
                </Fragment>
              );
            return one ? (
              <Fragment key={kind}>
                {who} asked to withdraw a Nervos DAO deposit of {amount(one.capacity!)}, made at{' '}
                <BlockLink look="inline" number={one.depositBlock!} />.{' '}
              </Fragment>
            ) : (
              <Fragment key={kind}>
                {who} asked to withdraw {count} Nervos DAO deposits.{' '}
              </Fragment>
            );
          case 'withdraw_complete':
            if (!attributed)
              return one ? (
                <Fragment key={kind}>
                  {amount(one.capacity!)} came out of the Nervos DAO, with{' '}
                  {amount(one.compensation!)} of compensation.{' '}
                </Fragment>
              ) : (
                <Fragment key={kind}>
                  {count} Nervos DAO deposits were withdrawn, each with its compensation.{' '}
                </Fragment>
              );
            return one ? (
              <Fragment key={kind}>
                {who} withdrew {amount(one.capacity!)} from the Nervos DAO, with{' '}
                {amount(one.compensation!)} of compensation.{' '}
              </Fragment>
            ) : (
              <Fragment key={kind}>
                {who} withdrew {count} Nervos DAO deposits, each with its compensation.{' '}
              </Fragment>
            );
          default:
            throw new Error(`${activity.txHash}: a Nervos DAO step "${kind}" has no sentence`);
        }
      })}
      {!attributed && <>The parties whose cells took these steps: {who}. </>}
    </>
  );
}

/**
 * One movement in the lede, as the figure draws it (lane P1, third round; review p-logic-chain
 * SHOULD 5 and NOTE 2): what left the givers, what reached the receivers, and, where the flow has
 * one, the part minted here or burnt here (a flow has at most one of each terminal, `txFlows`).
 * "100 Seal moved from A to B." / "100 Seal reached B: 50 from A, and 50 minted here." / "100 Seal
 * left A: 60 to B, and 40 burnt." / "100 Seal was minted to B." / "100 Seal left A and was burnt."
 */
function MovementSentence({ m, flow }: { m: TxMovement; flow: TxFlow }) {
  const minted = flow.from.find((e) => e.t === 'minted')?.amount;
  const burnt = flow.to.find((e) => e.t === 'burnt')?.amount;
  const named = (amount: bigint) =>
    m.item ? (
      <ItemName item={m.item} amount={amount} />
    ) : (
      <CkbAmount form="summary" shannons={amount} signed={false} tone="plain" />
    );
  // a part of a token's flow, in its units (the name stands once, at the sentence's head)
  const part = (amount: bigint) => {
    if (m.item?.kind !== 'token')
      throw new Error(`${flow.key}: a part minted or burnt beside its parties, of no token`);
    return (
      <TokenAmount
        form="summary"
        amount={amount.toString()}
        decimals={m.item.decimals ?? null}
        tone="plain"
      />
    );
  };
  if (m.from.length > 0 && m.to.length > 0) {
    if (minted === undefined && burnt === undefined)
      return (
        <>
          {named(flow.received)} moved from <Parties parties={m.from} /> to{' '}
          <Parties parties={m.to} />.{' '}
        </>
      );
    if (burnt === undefined)
      return (
        <>
          {named(flow.received)} reached <Parties parties={m.to} />: {part(flow.sent)} from{' '}
          <Parties parties={m.from} />, and {part(minted!)} minted here.{' '}
        </>
      );
    if (minted === undefined)
      return (
        <>
          {named(flow.sent)} left <Parties parties={m.from} />: {part(flow.received)} to{' '}
          <Parties parties={m.to} />, and {part(burnt)} burnt.{' '}
        </>
      );
    return (
      <>
        {named(flow.sent)} left <Parties parties={m.from} /> and {part(minted)} were minted here;{' '}
        {part(flow.received)} reached <Parties parties={m.to} /> and {part(burnt)} were burnt.{' '}
      </>
    );
  }
  if (m.to.length > 0)
    return m.item && minted !== undefined ? (
      <>
        {named(flow.received)} was minted to <Parties parties={m.to} />.{' '}
      </>
    ) : (
      <>
        {named(flow.received)} arrived at <Parties parties={m.to} />.{' '}
      </>
    );
  return m.item && burnt !== undefined ? (
    <>
      {named(flow.sent)} left <Parties parties={m.from} /> and was burnt.{' '}
    </>
  ) : (
    <>
      {named(flow.sent)} left <Parties parties={m.from} />.{' '}
    </>
  );
}

function TxSummary({
  activity,
  fee,
  compensation,
}: {
  activity: GlobalActivity;
  fee: string;
  compensation: bigint | null;
}) {
  const items = txMovements(activity);
  const flows = txFlows({ hash: activity.txHash, activity, fee, compensation });
  const feeOnly = onlyFeeMoved(activity, flows);
  // the CKB leg beside the items (lane P1, third round; review p-logic-chain NOTE 2: an NFT sale
  // said the NFT and not the payment): where an item moved, the CKB that reached parties is said
  // after it, from the CKB flow's parties
  const ckbFlow = flows.find((f) => f.key === 'ckb');
  const partiesOf = (ends: readonly FlowEnd[]) =>
    ends.flatMap((e) => (e.t === 'party' ? [e.party] : []));
  const moves: TxMovement[] =
    items.some((m) => m.item !== null) && ckbFlow && ckbFlow.received > BigInt(0)
      ? [...items, { item: null, from: partiesOf(ckbFlow.from), to: partiesOf(ckbFlow.to) }]
      : items;
  const flowOf = (item: ItemDelta | null) => {
    const key = item ? flowKey(item) : 'ckb';
    const flow = flows.find((f) => f.key === key);
    if (!flow) throw new Error(`${activity.txHash}: no flow ${key} for a movement it states`);
    return flow;
  };
  const mark = activityClassMark(activity);
  const ran =
    moves.length === 0 && activity.protocolActions.length === 0
      ? [
          ...new Set(
            activity.typeCalls.map((sc) =>
              formatScriptRef({ codeHash: sc.typeCodeHash, scriptName: sc.scriptName }, 'row')
            )
          ),
        ]
      : [];
  if (moves.length === 0 && ran.length === 0 && activity.protocolActions.length === 0 && !feeOnly)
    return null;
  return (
    <p data-testid="tx-summary" className="text-text text-body max-w-measure">
      {moves.length === 0 && mark && <ClassMark kind={mark} size={12} className="mr-1.5" />}
      {ran.length > 0 && `It ran ${listWords(ran)}. `}
      {moves.map((m, i) => (
        <MovementSentence key={i} m={m} flow={flowOf(m.item)} />
      ))}
      {feeOnly && (
        <>
          {'Only the fee moved: '}
          <Parties parties={activity.participants} />
          {` kept the rest of ${activity.participants.length === 1 ? 'its' : 'their'} CKB. `}
        </>
      )}
      {/* the DAO's steps beside whatever else moved (lane P1, third round; NOTE 2: they were
          skipped when an item moved, an iCKB deposit saying only its iCKB) */}
      {readable(activity.txHash, () => (
        <DaoSentences activity={activity} />
      ))}
      {moves.length === 0 &&
        // the DAO's steps are said above, in their sentences; the other protocols' facts here
        activity.protocolActions
          .filter((pa) => pa.protocol !== 'dao')
          .map((pa, i) => (
            <Fragment key={i}>
              {readable(activity.txHash, () => {
                // prose: the facts in a sentence, joined by commas (a "·" list is for rows); the
                // transaction's own record, so its amounts are exact (the P3 follow-up's 2c)
                const parts = protocolActionNoteParts(pa, 'row', 'exact');
                return parts.length ? (
                  <>
                    {parts.map((part, j) => (
                      <Fragment key={j}>
                        {j > 0 && ', '}
                        {part}
                      </Fragment>
                    ))}
                    {'. '}
                  </>
                ) : null;
              })}
            </Fragment>
          ))}
    </p>
  );
}

const OVERHEAD_FIELD = /header|length|union|offset/i;

/**
 * One witness row (Main.dc.html Witnesses): kind and size, which groups read it, the bytes as a
 * bar per field, and what the API decoded (CoBuild actions, the cellbase miner lock and message).
 */
function WitnessRow({
  tx,
  w,
  readers,
  scale,
  selected,
  lit,
  lookup,
  groupNameByHash,
  minerMessage,
  onSelect,
  onHover,
}: {
  tx: TransactionDetail;
  w: TransactionWitness;
  /** A cellbase's client message as the block's one decoder reads it; undefined while loading. */
  minerMessage?: { raw: string | null; text: string | null };
  readers: string[];
  scale: number;
  selected: boolean;
  lit: TxLinkSets | null;
  lookup: ScriptLookupResponse | undefined;
  groupNameByHash: Map<string, string>;
  onSelect: () => void;
  onHover: (hover: TxHover | null) => void;
}) {
  const on = lit?.witnesses.has(w.index) ?? false;
  const fields = w.fields.filter((f) => f.end > f.start);
  // What the witness says, under its bar; nothing when there is nothing to name.
  const noteText = (): string => {
    if (w.kind === 'cobuild' && w.actions && w.actions.length > 0) {
      const names = w.actions.map((a) => {
        const script = groupNameByHash.get(a.scriptHash) ?? shortenHash(a.scriptHash, 'row');
        return a.name
          ? `${script}: ${a.name}`
          : `${script} (script info ${shortenHash(a.scriptInfoHash, 'row')})`;
      });
      return `message: ${formatCount(w.actions.length, 'action')} for ${[...new Set(names)].join(', ')}`;
    }
    if (w.kind === 'cellbase') {
      const witness = required(tx.cellbaseWitness, tx.hash, 'cellbaseWitness');
      // the miner lock by the one decision (lane P1 follow-up): its name, or "lock 0x…" and its
      // hash type where the name does not cover it (a lock on the default lock's code hash by data)
      const words = lockWords(
        cellLockFacts(
          {
            lock: witness.lock,
            name: registeredName(lookup, witness.lock.codeHash),
            address: null,
          },
          `Transaction ${tx.hash} cellbase witness`
        ),
        'column'
      );
      if (words === null)
        throw new Error(`${tx.hash}: a column's lock words are never empty (its cellbase witness)`);
      const named = words.form !== 'not-indexed' && words.name !== null;
      const lock = `${named ? 'lock ' : ''}${lockWordsText(words, 'row')}`;
      if (minerMessage && minerMessage.raw !== witness.message)
        throw new Error(
          `${tx.hash}: the block's miner message ${minerMessage.raw} is not its cellbase witness message ${witness.message}`
        );
      const message =
        minerMessage === undefined
          ? 'reading…'
          : minerMessage.text === null
            ? `${formatCount((witness.message.length - 2) / 2, 'byte')}, not text`
            : `“${minerMessage.text}”`;
      return `miner ${lock}(${shortenHash(witness.lock.args, 'row')}) · message ${message}`;
    }
    return fields.map((f) => `${f.name} ${formatCount(f.end - f.start, 'B', 'B')}`).join(' · ');
  };
  const note = readable(tx.hash, () => {
    const text = noteText();
    return text ? (
      <span className="text-text-dim [overflow-wrap:anywhere]">{unitText(text)}</span>
    ) : null;
  });
  return (
    <button
      type="button"
      data-testid={`tx-witness-item-${w.index}`}
      data-lit={on}
      aria-pressed={selected}
      onClick={onSelect}
      onMouseEnter={() => onHover({ t: 'witness', index: w.index })}
      onMouseLeave={() => onHover(null)}
      onFocus={() => onHover({ t: 'witness', index: w.index })}
      onBlur={() => onHover(null)}
      className={cn(
        'border-row-rule text-small -mx-1.5 flex w-[calc(100%+12px)] flex-col gap-1 border-b px-1.5 py-2.5 text-left transition-opacity',
        on && 'bg-base-surface',
        lit !== null && !on && 'opacity-25'
      )}
    >
      <span className="flex w-full flex-wrap justify-between gap-3">
        <span className={selected ? 'text-jade' : 'text-text-bright'}>
          #{w.index} · {witnessKindLabel(w.kind)}
          {tx.dotcellNames?.some((name) => name.outputIndex === w.index) && (
            <span className="text-text-dim ml-2">.cell records</span>
          )}
        </span>
        <span className="text-text-dim">
          {unitNode(formatCount(w.size, 'B', 'B'))}
          {readers.length > 0 && ` · read by ${readers.join(', ')}`}
        </span>
      </span>
      {fields.length > 0 && (
        <span aria-hidden="true" className="flex h-2 gap-0.5">
          {fields.map((f, i) => (
            <span
              key={i}
              className={OVERHEAD_FIELD.test(f.name) ? undefined : 'bg-jade'}
              style={{
                width: Math.max(2, Math.round(((f.end - f.start) / scale) * 320)),
                backgroundColor: OVERHEAD_FIELD.test(f.name) ? CLASS_COLORS.ckb : undefined,
              }}
            />
          ))}
        </span>
      )}
      {note}
    </button>
  );
}

function WitnessInspector({ tx, witness }: { tx: TransactionDetail; witness: TransactionWitness }) {
  return (
    <div data-testid="tx-witness-deterministic-section" className="mt-4 space-y-4">
      <h3 className="text-text-bright text-small">
        {witness.kind === 'witness_args'
          ? 'WitnessArgs'
          : witness.kind === 'opaque'
            ? 'Opaque witness bytes'
            : witnessKindLabel(witness.kind)}
      </h3>
      <HexMap
        subject={`transaction ${tx.hash} witness ${witness.index}`}
        hex={required(tx.witnesses[witness.index], tx.hash, `witness ${witness.index} bytes`)}
        fields={witness.fields
          .filter((field) => field.end > field.start)
          .map((field, i) => ({
            id: `${witness.index}:${i}`,
            label: field.name,
            offset: field.start,
            length: field.end - field.start,
            description: `${field.name}: ${field.hex}`,
          }))}
      />
      {tx.dotcellNames
        ?.filter((name) => name.outputIndex === witness.index)
        .map((name) => (
          <div key={name.identityId} data-testid="tx-witness-dotcell-records">
            {/* which name's records these are: a statement, the reading ink (R3; P1 follow-up) */}
            <p className="text-text text-small max-w-measure mb-3">
              records payload of{' '}
              <Link look="inline" href={getIdentityItemDetailHref('dotcell', name.label)}>
                {name.name}
              </Link>{' '}
              (
              <Link
                look="inline"
                href={`/cell/${tx.hash}-${name.outputIndex}`}
                title={`${tx.hash}:${name.outputIndex}`}
              >
                output #{name.outputIndex}
              </Link>
              )
            </p>
            <DotCellRecordsTable records={name.records} />
          </div>
        ))}
    </div>
  );
}

/**
 * One cell dep (TX-15): its out point, then one statement led by the kind of dep — what it supplies
 * code for ("code for Spore · 76,752 B", "dep group for Default Lock"), or whose code it holds and
 * that it does not run here — then a dep group's members. PA-12: the statement is said once, at
 * every width (it was a left "supplies code for X" beside a right "for X").
 */
function DepRow({
  dep,
  index,
  lit,
  groupNameByHash,
  onHover,
}: {
  dep: CellDep;
  index: number;
  lit: TxLinkSets | null;
  groupNameByHash: Map<string, string>;
  onHover: (hover: TxHover | null) => void;
}) {
  const on = lit?.deps.has(index) ?? false;
  const names = (hashes: readonly string[]) => [
    ...new Set(hashes.flatMap((h) => (groupNameByHash.has(h) ? [groupNameByHash.get(h)!] : []))),
  ];
  const serves = names(dep.scriptHashes);
  const attached = names(dep.outputLockScriptHashes);
  const size = (bytes: number) => formatCount(bytes, 'B', 'B');
  const codeSize =
    dep.depType === 'code'
      ? ` · ${size(required(dep.resolvedCells[0], dep.outPointTxHash, 'code dep cell').codeSize)}`
      : '';
  const kind = dep.depType === 'dep_group' ? 'dep group' : 'code';
  return (
    <div
      id={`tx-dep-${index}`}
      data-lit={on}
      tabIndex={0}
      onMouseEnter={() => onHover({ t: 'dep', index })}
      onMouseLeave={() => onHover(null)}
      onFocus={() => onHover({ t: 'dep', index })}
      onBlur={() => onHover(null)}
      className={cn(
        'border-row-rule text-small -mx-1.5 flex flex-col gap-y-0.5 border-b px-1.5 py-2.5 transition-opacity',
        on && 'bg-base-surface',
        lit !== null && !on && 'opacity-25'
      )}
    >
      <Link
        look="key"
        href={`/cell/${dep.outPointTxHash}-${dep.outPointIndex}`}
        className="min-w-0 self-start transition-colors [overflow-wrap:anywhere]"
      >
        <ShortId value={dep.outPointTxHash} kind="hash" size="row" />
        <OutpointIndex index={dep.outPointIndex} />
      </Link>
      <span className="text-text-dim">
        {serves.length > 0 ? (
          <>
            {kind} for <span className="text-text">{serves.join(', ')}</span>
          </>
        ) : attached.length > 0 ? (
          `${kind} of ${attached.join(', ')}, not run here`
        ) : (
          `${kind}, not run here`
        )}
        {codeSize}
      </span>
      {dep.depType === 'dep_group' && (
        <div className="space-y-1 pl-3 pt-1">
          {dep.resolvedCells.map((cell) => {
            const memberServes = names([...cell.scriptHashes, ...cell.outputLockScriptHashes]);
            return (
              // a member of the group, its cell and what it serves: values, the reading ink
              <span key={`${cell.txHash}:${cell.index}`} className="text-text block">
                <Link look="inline" href={`/cell/${cell.txHash}-${cell.index}`}>
                  ↳ <ShortId value={cell.txHash} kind="hash" size="row" />
                  <OutpointIndex index={cell.index} />
                </Link>
                {memberServes.length > 0
                  ? ` · ${memberServes.join(', ')} code, ${size(cell.codeSize)}`
                  : ` · a ${size(cell.codeSize)} cell no script here names`}
              </span>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** A witness drawn as its row, or a run of empty witnesses drawn as one line. */
type WitnessItem =
  | { kind: 'witness'; w: TransactionWitness }
  | { kind: 'run'; from: number; to: number; count: number };

/**
 * The witness list as the developer tier draws it (lane D3; the board's "#1 to #157 are empty"):
 * every witness in order, a run of two or more empty ones (0 bytes) that `folds` lets go — no
 * group reads them, the page has not selected one — as one item; a lone empty witness keeps its
 * row, and a witness with bytes is always written whole.
 */
function witnessItems(
  witnesses: readonly TransactionWitness[],
  folds: (w: TransactionWitness) => boolean
): WitnessItem[] {
  const out: WitnessItem[] = [];
  let run: TransactionWitness[] = [];
  const close = () => {
    if (run.length > 1)
      out.push({
        kind: 'run',
        from: run[0].index,
        to: run[run.length - 1].index,
        count: run.length,
      });
    else for (const w of run) out.push({ kind: 'witness', w });
    run = [];
  };
  for (const w of witnesses) {
    if (w.kind === 'empty' && w.size === 0 && folds(w)) run.push(w);
    else {
      close();
      out.push({ kind: 'witness', w });
    }
  }
  close();
  return out;
}

/**
 * A run of empty witnesses on one line (the board's dashed line): which they are, by counting,
 * "#1 to #157 are empty: 157 witnesses of 0 B". The way to every row is the fold's one control
 * after the list (`FoldToggle`; review of phases C and D, SHOULD 5), which stays where it is, and
 * keeps its focus, as the runs open and fold.
 */
function EmptyWitnesses({ run }: { run: Extract<WitnessItem, { kind: 'run' }> }) {
  return (
    <div
      data-testid={`tx-witness-run-${run.from}`}
      className="border-text-dim/60 text-small my-2 border border-dashed px-3 py-1.5"
    >
      <span data-run-words="" className="text-text min-w-0">
        {unitText(
          `#${run.from} to #${run.to} are empty: ${formatCount(run.count, 'witness', 'witnesses')} of ${formatCount(0, 'B', 'B')}`
        )}
      </span>
    </div>
  );
}

function EvidenceHeading({ title, note }: { title: string; note?: ReactNode }) {
  return (
    <div className="border-row-rule flex flex-wrap items-baseline justify-between gap-2 border-b pb-1.5">
      <h2 className="text-body text-text font-medium">{title}</h2>
      {note && <span className="text-text-dim text-small">{note}</span>}
    </div>
  );
}

export default function TransactionDetailPage() {
  const { hash } = useParams() as { hash: string };
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const router = useRouter();
  const read = useQuery<TransactionDetail>({
    queryKey: ['transaction', hash],
    queryFn: () => api.getTransactionDetail(hash),
    refetchInterval: (q) =>
      provisional(q.state.data) || isServiceUnavailableError(q.state.error) ? POLL_MS : false,
    refetchIntervalInBackground: true,
  });
  const { data: tx, error } = read;
  const isProvisional = provisional(tx);
  const awaitingInputs = isServiceUnavailableError(error);
  const { data: activity, error: activityError } = useQuery({
    queryKey: ['transaction-activity', hash],
    queryFn: () => api.getTransactionActivity(hash),
    enabled: !!tx && !isProvisional && !tx.isCellbase,
  });
  const { data: deps, error: depsError } = useQuery({
    queryKey: ['txCellDeps', hash],
    queryFn: () => api.getTransactionCellDeps(hash),
    enabled: !!tx && !isProvisional,
  });
  const { data: cellbaseBlock } = useQuery({
    queryKey: ['block', tx?.blockNumber],
    queryFn: () => api.getBlock(tx!.blockNumber!),
    enabled: !!tx && tx.isCellbase && !provisional(tx) && tx.blockNumber !== undefined,
  });
  const { data: lifecycle } = useQuery({
    queryKey: ['txLifecycle', hash],
    queryFn: () => api.getTransactionLifecycle(hash),
    enabled: !!tx && !isProvisional && !tx.isCellbase,
  });
  const [graphOpen, setGraphOpen] = useState(searchParams.get('tab') === 'graph');
  const { data: graph, error: graphError } = useQuery({
    queryKey: ['txGraph', hash],
    queryFn: () => api.getTransactionGraph(hash),
    enabled: !!tx && !isProvisional && graphOpen,
  });
  const codeHashes = useMemo(
    () =>
      Array.from(
        new Set(
          [...(tx?.inputs ?? []), ...(tx?.outputs ?? [])]
            .flatMap((c) => [c.lock?.codeHash, c.type?.codeHash])
            .concat(tx?.cellbaseWitness ? [tx.cellbaseWitness.lock.codeHash] : [])
            .filter((v): v is string => v !== undefined)
        )
      ),
    [tx]
  );
  const { data: lookup } = useQuery({
    queryKey: ['scriptLookup', codeHashes, hash],
    queryFn: () => api.lookupScripts(codeHashes, hash),
    enabled: codeHashes.length > 0,
    staleTime: Infinity,
  });
  const { cycles, hasCycles, isCalculating, hasFailed, isPaused } = useCyclesCalculation(
    hash,
    tx?.cycles,
    (tx?.isCellbase ?? false) || isProvisional || tx?.blockNumber === 0,
    tx?.cyclesStatus,
    !isProvisional && tx?.scriptGroups?.some((group) => group.ran && group.cycles === null) === true
  );
  const [selectedWitness, setSelectedWitness] = useState<number | null>(() =>
    indexParam(searchParams.get('witness'))
  );
  const [hover, setHover] = useState<TxHover | null>(null);
  const [selectedGroup, setSelectedGroup] = useState<string | null>(searchParams.get('wg'));
  // the developer tier's runs of empty witnesses, each one line until opened (lane D3)
  const [allWitnesses, setAllWitnesses] = useState(false);
  const witnessListId = useId();
  useEffect(() => {
    setSelectedWitness(indexParam(searchParams.get('witness')));
    setSelectedGroup(searchParams.get('wg'));
  }, [hash, searchParams]);
  // the URL's anchor, `#…`: a cell's out point keeps that cell drawn under the fold
  const searchAndHash = useSearchAndHash();
  const fragment = searchAndHash.includes('#')
    ? searchAndHash.slice(searchAndHash.indexOf('#'))
    : '';
  // The cell the URL's anchor names, brought into view once its tile is drawn (lane P1, lane D3's
  // note): the browser's own jump to an anchor comes before the cells are read, so it finds
  // nothing; the page scrolls to the tile once, per anchor, when it stands.
  const scrolledTo = useRef<string | null>(null);
  useEffect(() => {
    const anchored = anchoredOutPoint(fragment.slice(1));
    const t = read.data;
    if (anchored === null || !t || scrolledTo.current === anchored) return;
    const at = (side: 'in' | 'out', i: number) => `[data-cell-id="${side}:${i}"]`;
    const input = (t.inputs ?? []).findIndex(
      (c) =>
        c.previousOutput !== undefined &&
        `${c.previousOutput.txHash.toLowerCase()}-${c.previousOutput.index}` === anchored
    );
    const output = (t.outputs ?? []).findIndex(
      (_, i) => `${t.hash.toLowerCase()}-${i}` === anchored
    );
    const selector = input >= 0 ? at('in', input) : output >= 0 ? at('out', output) : null;
    const tile = selector === null ? null : document.querySelector(selector);
    // every browser scrolls an element; jsdom, the tests' document, has no scrollIntoView
    if (!tile || typeof tile.scrollIntoView !== 'function') return;
    tile.scrollIntoView({ block: 'center' });
    scrolledTo.current = anchored;
  });
  /** This page's URL with `next` as its query, its anchor kept. */
  const urlWith = (next: URLSearchParams) => `${pathname}${next.size ? `?${next}` : ''}${fragment}`;
  const select = (index: number | null, group: string | null) => {
    setSelectedWitness(index);
    setSelectedGroup(group);
    const next = new URLSearchParams(searchParams.toString());
    if (index === null) next.delete('witness');
    else next.set('witness', String(index));
    if (group === null) next.delete('wg');
    else next.set('wg', group);
    router.replace(urlWith(next), { scroll: false });
  };
  // The fold (lane D3): which sheets are drawn whole, from the URL; opening or folding one is a
  // step of the history, so back walks it.
  const openSheets = cellsParam(searchParams.get(CELLS_PARAM), hash);
  const toggleSheet = (side: 'consumed' | 'created', open: boolean) => {
    const next = new URLSearchParams(searchParams.toString());
    const value = cellsValue({ ...openSheets, [side]: open });
    if (value === null) next.delete(CELLS_PARAM);
    else next.set(CELLS_PARAM, value);
    router.push(urlWith(next), { scroll: false });
  };
  const shell = (children: ReactNode) => (
    <div className="min-h-screen">
      <main className="container mx-auto py-8">{children}</main>
    </div>
  );
  // Awaited, fetching or paused (offline, or a hidden tab between retries): isLoading is pending
  // AND fetching, so a paused read would fall through as if it had completed.
  if (isAwaited(read)) return shell(<DetailSkeleton />);
  if (awaitingInputs && !tx)
    return shell(
      <DetailLayout
        title="Transaction"
        identity={hash}
        status={<span className="text-text-dim text-small">Pending</span>}
      >
        <div
          data-testid="tx-inputs-not-yet-resolvable"
          className="text-text-dim text-small space-y-3"
        >
          <p className="max-w-measure">
            waiting for the node… the inputs of this unconfirmed transaction cannot be resolved yet.
            Retrying automatically.
          </p>
          <p className="max-w-measure [overflow-wrap:anywhere]">{error.apiMessage}</p>
        </div>
      </DetailLayout>
    );
  if (error && !tx && isNotFoundApiError(error)) {
    return (
      <NotFoundPage
        entity={{
          kind: 'transaction',
          identifier: hash,
          checkedEndpoint: `/transactions/${encodeURIComponent(hash)}/detail`,
        }}
      />
    );
  }
  if (!tx && isBadRequestApiError(error))
    return <NotFoundPage notAnId={{ kind: 'transaction', reason: error.apiMessage }} />;
  if (!tx) {
    // past pending, no error means the read succeeded: with no transaction, an API invariant broke
    if (!error) throw new Error(`Transaction ${hash}: request completed without a transaction`);
    return shell(<ReadFailure what="transaction" error={error} />);
  }
  // The transaction's stone (the seals by kind), pressed on the head and, from lg, across the cells:
  // its hash, the cells it consumed — the API's count, as its row in a list, but none for a
  // cellbase (the API counts its one null input) — and the cells it created; dashed while it is
  // not yet indexed in a block.
  const seal: { id: TxSealId; state?: SealState } = {
    id: {
      kind: 'tx',
      hash: tx.hash,
      consumed: tx.isCellbase ? 0 : tx.inputsCount,
      created: tx.outputsCount,
    },
    state: isProvisional ? 'unpressed' : undefined,
  };
  // The status word in the strip, lowercase as the board writes it, and its dot: committed in the
  // jade, proposed in the gold, waiting (in the pool, or committed and not yet indexed) in the aqua.
  const status = (
    tx.poolStatus === 'committed_awaiting_index'
      ? poolStatusLabel(tx.poolStatus)
      : tx.status === 'pending'
        ? 'Pending'
        : tx.status === 'proposed'
          ? 'Proposed'
          : 'Committed'
  ).toLowerCase();
  const statusDot = tx.status === 'proposed' ? 'gold' : isProvisional ? 'aqua' : ('jade' as const);
  // The API serves script groups and witnesses for an uncommitted transaction too (TX-37).
  const groups = required(tx.scriptGroups, hash, 'scriptGroups');
  const witnesses = required(tx.witnessDetails, hash, 'witnessDetails');
  const headerDeps = required(tx.headerDeps, hash, 'headerDeps');
  const chosenWitness =
    selectedWitness === null
      ? null
      : required(
          witnesses.find((w) => w.index === selectedWitness),
          hash,
          `witness ${selectedWitness}`
        );
  if (selectedGroup !== null && !groups.some((g) => groupKey(g) === selectedGroup))
    throw new Error(`Transaction ${hash}: unknown script group ${selectedGroup}`);
  // the cells' tiles; a cell the lock decision refuses fails the Cells section in its place (D7)
  const tilesRead = isProvisional ? null : readCellTiles(tx, lookup);
  const tiles = tilesRead?.value ?? null;
  const compensation = activity ? daoCompensation(activity) : undefined;
  // Lane P1: only the fee moved (`onlyFeeMoved`, the figure's own flows); an activity whose flows
  // cannot be read is not one, and its sections say why in their places.
  const feeOnly =
    activity !== undefined &&
    activity !== null &&
    readPart(() =>
      onlyFeeMoved(
        activity,
        txFlows({ hash, activity, fee: tx.fee, compensation: compensation ?? null })
      )
    ).value === true;

  // ---- hover linking by group identity (TX-14) ----
  const linkGroups = groups.map((g) => ({
    id: groupKey(g),
    cellIds: groupCells(g),
    witnessIndex: g.witnessIndex,
    scriptHash: g.scriptHash,
  }));
  const linkDeps = (deps ?? []).map((d, index) => ({
    index,
    scriptHashes: [...d.scriptHashes, ...d.outputLockScriptHashes],
  }));
  const feePaidBy = required(tx.feePaidBy, hash, 'feePaidBy');
  // What moved (review 2026-11-03 v2): a hovered party lights its cells, a hovered asset the cells
  // that carry it; CKB, the ground, lights the cells that carry nothing else.
  const flowCells = (h: WhatMovedHover): string[] => {
    const cells = [
      ...(tx.inputs ?? []).map((c, i) => ({ c, id: `in:${i}` })),
      ...(tx.outputs ?? []).map((c, i) => ({ c, id: `out:${i}` })),
    ];
    if (h.t === 'party')
      return h.party.address === null
        ? []
        : cells.filter(({ c }) => c.address === h.party.address).map((x) => x.id);
    const a = h.asset;
    const id =
      a.kind === 'token'
        ? a.item.typeScriptHash
        : a.kind === 'object'
          ? a.item.objectId
          : a.kind === 'identity'
            ? a.item.identityId
            : null;
    return cells
      .filter(({ c }) =>
        id === null
          ? c.display === null && c.type === undefined
          : c.display?.kind === a.kind && c.display.id === id
      )
      .map((x) => x.id);
  };
  const focus: TxHover | null =
    hover ??
    (selectedGroup !== null
      ? { t: 'group', id: selectedGroup }
      : selectedWitness !== null
        ? { t: 'witness', index: selectedWitness }
        : null);
  const lit = focus ? txLinks(focus, linkGroups, linkDeps, hash) : null;
  const groupNameByHash = new Map(groups.map((g) => [g.scriptHash, groupLabel(g, lookup)]));
  const readersOf = (index: number) => [
    ...new Set(groups.filter((g) => g.witnessIndex === index).map((g) => groupLabel(g, lookup))),
  ];
  const witnessScale = Math.max(1, ...witnesses.map((w) => w.size));
  // a run folds the empty witnesses no group reads and the page did not select (lane D3)
  const foldsWitness = (w: TransactionWitness) =>
    readersOf(w.index).length === 0 && w.index !== selectedWitness;
  const ran = groups.filter((g) => g.ran).length;

  // ---- continuity (same Type ID / object / identity / file cell continues) ----
  const continuity = tiles ? txContinuity(tx, tiles.inputs) : [];
  const legend = [...new Set(continuity.map((l) => l.label))].map((label) => ({
    label,
    links: continuity.filter((l) => l.label === label),
  }));
  // The cell the URL points at by its out point (the cell page's link to this transaction): drawn
  // wherever it stands, its continuity partner with it (the fold keeps both). An anchor that names
  // no cell of this transaction keeps none, as a browser goes nowhere for an unknown anchor.
  const anchored = anchoredOutPoint(fragment.slice(1));
  const kept = tiles
    ? [...tiles.inputs, ...tiles.outputs].filter(
        (t) =>
          t.outPoint !== undefined &&
          `${t.outPoint.txHash.toLowerCase()}-${t.outPoint.index}` === anchored
      )
    : [];

  // ---- script-group matrix ----
  const matrix = scriptGroupRows(tx, {
    lookup,
    deps,
    depsError,
    provisional: isProvisional,
    depHref: (i) => `#tx-dep-${i}`,
    select: { group: selectedGroup, pick: select },
  });

  // ---- headline and summary (TX-11) ----
  const headline = isProvisional
    ? tx.poolStatus === 'committed_awaiting_index'
      ? 'Committed, being indexed'
      : 'Waiting in the pool'
    : tx.isCellbase
      ? 'Cellbase'
      : activity
        ? activityHeadline(activity)
        : 'Transaction';
  const summary = isProvisional ? (
    <p className="text-text text-small max-w-measure">
      Activity and canonical cell structure are available once this transaction is committed and
      indexed.
    </p>
  ) : tx.isCellbase ? (
    // Lane P1 (the plan's P1 paragraph; review chain-a tx-cellbase): who is paid, how much and for
    // which block, said once: the block it is in is the status strip's, and that it consumes no
    // cell the cells' note and the comment say.
    <p data-testid="tx-summary" className="text-text text-body max-w-measure">
      {tx.pays === undefined ? (
        <ActivityReadError txHash={hash} error={new Error('cellbase has no "pays" field')} />
      ) : tx.pays === null ? (
        // the API's null: the block whose reward it would pay is the genesis block or before it
        // (lane P1, third round; review p-logic-chain SHOULD 4: it read "no block before it is
        // final yet", false for block 0 and for the blocks after it)
        tx.blockNumber === 0 ? (
          readable(hash, () => (
            <>
              The genesis block’s cellbase pays no block’s reward: it created the chain’s first
              cells,{' '}
              <CkbAmount
                form="summary"
                shannons={exactUnsignedInteger(
                  required(tx.outputsCapacity, hash, 'outputsCapacity'),
                  `${hash}: outputs capacity`
                )}
                signed={false}
                tone="plain"
              />
              .
            </>
          ))
        ) : (
          'It pays no block’s reward: the block whose reward it would pay is the genesis block, which earns none, or would come before the chain began.'
        )
      ) : (
        readable(hash, () => {
          const pays = tx.pays!;
          return (
            <>
              It pays <CellbasePayees tx={tx} />{' '}
              <CkbAmount
                form="summary"
                shannons={exactUnsignedInteger(
                  required(tx.outputsCapacity, hash, 'outputsCapacity'),
                  `${hash}: outputs capacity`
                )}
                signed={false}
                tone="plain"
              />
              , the reward for block{' '}
              <Link look="inline" href={`/blocks/${pays.hash}`}>
                {formatBlockNumber(pays.number)}
              </Link>
              .
            </>
          );
        })
      )}
    </p>
  ) : activityError ? (
    <p role="alert" className="text-small max-w-measure">
      Cannot read activity for {hash}: {activityError.message}
    </p>
  ) : activity === undefined ? (
    <div className="bg-base-surface h-5 w-96 max-w-full animate-pulse" />
  ) : activity === null ? (
    <p role="alert" className="text-small max-w-measure">
      Transaction {hash} has no indexed activity.
    </p>
  ) : (
    readable(hash, () => (
      <TxSummary activity={activity} fee={tx.fee} compensation={compensation ?? null} />
    ))
  );

  // ---- capacity (TX-19, TX-33) ----
  const capacityFact = isProvisional ? (
    <PendingValue />
  ) : tx.isCellbase ? (
    <>
      block reward{' '}
      <CkbAmount
        form="exact"
        shannons={exactUnsignedInteger(
          required(tx.outputsCapacity, hash, 'outputsCapacity'),
          `${hash}: outputs capacity`
        )}
        signed={false}
        tone="plain"
      />{' '}
      = out
    </>
  ) : compensation === undefined ? (
    activityError ? (
      <span className="text-text-dim">unreadable without the transaction&rsquo;s activity</span>
    ) : (
      <span className="text-text-dim">Reading…</span>
    )
  ) : (
    readable(hash, () => {
      const eq = capacityEquation({
        hash,
        inputs: required(tx.inputsCapacity, hash, 'inputsCapacity'),
        outputs: required(tx.outputsCapacity, hash, 'outputsCapacity'),
        fee: tx.fee,
        compensation,
      });
      return (
        <span data-testid="tx-capacity-equation">
          <CkbAmount form="exact" shannons={eq.inputs} signed={false} tone="plain" /> in
          {eq.compensation !== null && (
            <>
              {' + '}
              <CkbAmount form="exact" shannons={eq.compensation} signed={false} tone="plain" />{' '}
              compensation
            </>
          )}
          {' = '}
          <CkbAmount form="exact" shannons={eq.outputs} signed={false} tone="plain" /> out +{' '}
          <CkbAmount form="exact" shannons={eq.fee} signed={false} tone="plain" /> fee
        </span>
      );
    })
  );
  const occupiedIn = isProvisional
    ? null
    : exactUnsignedInteger(
        required(tx.inputsCommonKnowledgeSize, hash, 'inputsCommonKnowledgeSize'),
        `${hash}: input occupied bytes`
      );
  const occupiedOut = isProvisional
    ? null
    : exactUnsignedInteger(
        required(tx.outputsCommonKnowledgeSize, hash, 'outputsCommonKnowledgeSize'),
        `${hash}: output occupied bytes`
      );
  const knowledgeDelta =
    occupiedIn !== null && occupiedOut !== null ? occupiedOut - occupiedIn : null;
  const feeShannons = exactUnsignedInteger(tx.fee, `${hash}: fee`);
  // The comment (rubbing × Song print, phase 5, the Transaction board): written only when every
  // figure it states is known, from the counts the cells heading draws and the common-knowledge
  // delta of the evidence tier. A pool transaction has no tiles and no occupied bytes yet, so it
  // has no comment. The fee is said once, in the status strip (lane D2, gate D0′'s "said once").
  const comment =
    tiles && knowledgeDelta !== null
      ? txComment({
          inputs: tiles.inputs.length,
          outputs: tiles.outputs.length,
          isCellbase: tx.isCellbase,
          knowledgeDeltaBytes: knowledgeDelta,
        })
      : null;
  // The status strip (lane D2, the transaction board passed at gate D0′): status, time, block and
  // fee under the id line. A fact the page does not know is not passed: a transaction waiting in
  // the pool has no block and no block time yet, and says how long it has waited instead.
  const stripFacts: StripFact[] = [
    {
      label: 'status',
      value: status,
      dot: statusDot,
      note: isProvisional
        ? undefined
        : unitNode(formatCount(required(tx.confirmations, hash, 'confirmations'), 'confirmation')),
    },
    ...(isProvisional
      ? tx.pendingSince
        ? [
            {
              label: 'pending since',
              value: <Utc8DateTime timestamp={tx.pendingSince} />,
              note: <Utc8Age timestamp={tx.pendingSince} />,
            },
          ]
        : []
      : [
          {
            label: 'time',
            value: <Utc8DateTime timestamp={required(tx.timestamp, hash, 'timestamp')} />,
            note: <Utc8Age timestamp={required(tx.timestamp, hash, 'timestamp')} />,
          },
        ]),
    ...(tx.blockNumber !== undefined
      ? [
          {
            label: 'block',
            value: (
              <Link look="inline" href={`/blocks/${tx.blockNumber}`}>
                {formatBlockNumber(tx.blockNumber)}
              </Link>
            ),
          },
        ]
      : []),
    {
      label: 'fee',
      // the fee exact, its trailing zeros dropped (the board's form), in the strip's bright ink
      value: <CkbAmount form="exact" shannons={feeShannons} signed={false} tone="inherit" />,
      note:
        feePaidBy.length > 0 ? (
          <>
            paid by{' '}
            <ParticipantRefView
              participant={{
                address: feePaidBy[0].address,
                lockHash: feePaidBy[0].lockHash,
                lockHashPrefix: null,
                roles: [],
                // the fee is paid from the inputs: its payer's lock holds one, so it sent
                sent: true,
              }}
              size="row"
              compact
              quiet
              look="inline"
            />
            {unitNode(
              feePaidBy.length > 1 && ` and ${formatCount(feePaidBy.length - 1, 'other lock')}`
            )}
          </>
        ) : undefined,
    },
  ];

  const inflow =
    tiles && tx.isCellbase
      ? {
          tone: 'reward' as const,
          label: 'block reward',
          amount: required(tx.outputsCapacity, hash, 'outputsCapacity'),
        }
      : compensation
        ? {
            tone: 'dao' as const,
            label: 'Nervos DAO compensation',
            amount: compensation.toString(),
          }
        : undefined;

  const raw = (
    <div className="space-y-5">
      <p className="max-w-measure">
        <Link look="inline" href={`/tx/${hash}.md`} target="_blank" rel="noreferrer">
          Markdown
        </Link>
      </p>
      <details>
        <summary className="text-text-dim cursor-pointer">Cells</summary>
        <pre className="mt-3 whitespace-pre-wrap break-all">
          {JSON.stringify({ inputs: tx.inputs, outputs: tx.outputs }, null, 2)}
        </pre>
      </details>
      <details>
        <summary className="text-text-dim cursor-pointer">Scripts</summary>
        <div className="mt-3 space-y-3">
          {groups.map((g) => (
            <div key={groupKey(g)}>
              <Link look="inline" href={groupHref(g, lookup)}>
                {groupLabel(g, lookup)}
              </Link>
              <pre className="mt-2 whitespace-pre-wrap break-all">
                {JSON.stringify(g.script, null, 2)}
              </pre>
            </div>
          ))}
        </div>
      </details>
      <details>
        <summary className="text-text-dim cursor-pointer">
          Witness bytes · {tx.witnessesAvailable ? tx.witnesses.length : 'unavailable'}
        </summary>
        <div className="mt-3 space-y-2">
          {tx.witnesses.map((w, i) => (
            <p key={i} className="break-all">
              <span className="text-text-dim">#{i} </span>
              {w}
            </p>
          ))}
        </div>
      </details>
      <details>
        <summary className="text-text-dim cursor-pointer">
          Header deps · {headerDeps.length}
        </summary>
        <div className="mt-3 space-y-2">
          {headerDeps.map((h) => (
            <p key={h.blockHash} className="break-all">
              {h.blockHash}
            </p>
          ))}
        </div>
      </details>
      <details open={graphOpen} onToggle={(e) => setGraphOpen(e.currentTarget.open)}>
        <summary className="text-text-dim cursor-pointer">Cell graph</summary>
        {isProvisional ? (
          <PendingValue />
        ) : graphError ? (
          <p className="max-w-measure" role="alert">
            {graphError.message}
          </p>
        ) : graph ? (
          <DeferredCellGraph
            nodes={graph.nodes}
            links={graph.links}
            height={340}
            onNodeClick={(node) => {
              if (node.nodeType === 'transaction' && node.data?.hash)
                router.push(`/tx/${node.data.hash}`);
              if (
                node.nodeType === 'cell' &&
                node.data?.txHash !== undefined &&
                node.data.outputIndex !== undefined
              )
                router.push(`/cell/${node.data.txHash}-${node.data.outputIndex}`);
            }}
          />
        ) : (
          <p className="text-text-dim py-4">Loading graph…</p>
        )}
      </details>
      <details>
        <summary className="text-text-dim cursor-pointer">JSON</summary>
        <pre className="mt-3 whitespace-pre-wrap break-all">{JSON.stringify(tx, null, 2)}</pre>
      </details>
    </div>
  );

  return shell(
    <div className="text-body min-w-0 font-mono">
      {error && (
        <p role="alert" className="text-small max-w-measure mb-4">
          Unable to refresh transaction: {error.message}. Showing previously loaded data.
        </p>
      )}
      {/* Tier 1: what happened, to whom, for how much, and which cells changed hands, across the
          whole measure (U-13, gate D0′): the status, time, block and fee stand in the head's status
          strip under the hash, so no column is kept beside the sections for them. */}
      <div data-tier="1" className="grid grid-cols-[minmax(0,1fr)] items-start gap-y-8">
        <header className="min-w-0">
          <DocumentTitle parts={[titleHash(tx.hash), 'transaction']} />
          {/* The one corner (Transaction board): the transaction's long stone pressed beside its
              name plate (seals by kind, C10) — the kind line, the action, the hash and its copy —
              then the status strip, the summary and the board's comment under both, from the
              page's left edge; from lg its seam seal is pressed again between the cells below.
              The action is one type step smaller below md, as every name plate's name. */}
          <SealedHead
            id={seal.id}
            state={seal.state}
            kind="Transaction"
            // the id line, in the plate's voice (the plate sets it); the copy is the library's
            // CopyButton, its icon after the hash and its answer in words beside it, the head id's
            // one form (lane c2-finish, item 4: 8 px after the id, at its first line)
            identity={<HeadId value={tx.hash} kind="hash" copy="copy transaction hash" />}
            strip={{ label: 'Transaction status', facts: stripFacts }}
            below={
              <>
                {summary}
                {comment !== null && <Comment lines={comment} className="mt-4" />}
              </>
            }
          >
            <h1 className="text-text-bright text-entity max-md:text-index break-words font-medium">
              {headline}
            </h1>
          </SealedHead>
        </header>
        {activity && !feeOnly && (
          <div className="min-w-0">
            {/* What moved (review 2026-11-03 v2): the asset flows, from the left to the right,
                before the ledger; an activity whose flows do not conserve reads as an error here
                instead of taking the page down. A transaction that moved only its fee draws no
                figure: the lede says so in one line (lane P1). */}
            <DetailSection title="What moved">
              {readable(hash, () => (
                <WhatMoved
                  hash={hash}
                  activity={activity}
                  fee={tx.fee}
                  compensation={compensation ?? null}
                  onHover={(h) => setHover(h ? { t: 'cells', ids: flowCells(h) } : null)}
                />
              ))}
            </DetailSection>
          </div>
        )}
        {/* the balance table, but where only the fee moved from one party: its one row would
            restate the fee the strip and the lede state (lane P1) */}
        {activity && !(feeOnly && activity.participants.length === 1) && (
          <div className="min-w-0">
            <BalanceChanges activity={activity} fee={tx.fee} compensation={compensation ?? null} />
          </div>
        )}
        {tilesRead?.error !== undefined && (
          <div className="min-w-0">
            <DetailSection title="Cells">
              <CellTilesError tx={tx} error={tilesRead.error} />
            </DetailSection>
          </div>
        )}
        {tiles && (
          <div className="min-w-0">
            {/* The command and what it runs on (rubbing × Song print, phase 5, the Transaction
                board): `cells 3 in → 2 out`; the legend stays the row's right end. */}
            <DetailSection
              title="Cells"
              args={`${tiles.inputs.length.toLocaleString('en-US')} in → ${tiles.outputs.length.toLocaleString('en-US')} out`}
              actions={
                legend.length > 0 && (
                  <span className="text-text-dim text-meta flex flex-wrap items-center gap-x-3.5 gap-y-1">
                    {legend.map((l) => (
                      <button
                        key={l.label}
                        type="button"
                        onMouseEnter={() =>
                          setHover({ t: 'cells', ids: l.links.flatMap((x) => [x.from, x.to]) })
                        }
                        onMouseLeave={() => setHover(null)}
                        onFocus={() =>
                          setHover({ t: 'cells', ids: l.links.flatMap((x) => [x.from, x.to]) })
                        }
                        onBlur={() => setHover(null)}
                        className="hover:text-text flex items-center gap-1.5"
                      >
                        <svg aria-hidden="true" width="22" height="6" className="shrink-0">
                          <path
                            d="M0 3h22"
                            stroke={continuityColor(l.links[0].kind)}
                            strokeWidth="1.3"
                            strokeDasharray="4 3"
                          />
                        </svg>
                        {l.label}
                      </button>
                    ))}
                  </span>
                )
              }
            >
              {/* an explaining sentence: the reading ink (R3; lane P1 follow-up) */}
              <p className="text-text text-small max-w-measure -mt-2 mb-4">
                {tx.isCellbase
                  ? tx.blockNumber === 0
                    ? 'The genesis cellbase consumes no cell: it creates the chain’s first cells.'
                    : tx.pays === null
                      ? 'A cellbase consumes no cell; this one pays no block’s reward.'
                      : 'A cellbase consumes no cell: it creates the reward of a block that is now final.'
                  : compensation
                    ? 'A transaction spends whole cells and creates new ones. The Nervos DAO adds the compensation; the rest of the difference is the fee.'
                    : 'A transaction spends whole cells and creates new ones. Capacity is conserved; the difference is the fee.'}
              </p>
              <TxStructure
                txHash={hash}
                inputs={tiles.inputs}
                outputs={tiles.outputs}
                fee={tx.fee}
                // C11: the sheets' heads end with the API's exact totals
                consumedCapacity={required(tx.inputsCapacity, hash, 'inputsCapacity')}
                createdCapacity={required(tx.outputsCapacity, hash, 'outputsCapacity')}
                linking="page"
                inflow={inflow}
                continuityList={false}
                focusedCellIds={lit ? [...lit.cells] : []}
                onCellHover={(id) => setHover(id ? { t: 'cell', id } : null)}
                continuity={continuity}
                seamSeal={seal}
                // the fold (lane D3): six cells a sheet, the URL's cell kept, the open sheets the
                // URL's `cells`
                fold={{
                  head: FOLD_HEAD,
                  keep: kept.map((t) => t.id),
                  open: openSheets,
                  onToggle: toggleSheet,
                }}
                // A lit tile clears the selection on click only while there is one: a tile lit
                // by hover alone has no clearing click (review 2026-11-03 — the clearing replace
                // used to cancel the out point link's navigation to the cell page).
                onClearFocus={
                  selectedGroup !== null || selectedWitness !== null
                    ? () => select(null, null)
                    : undefined
                }
              />
            </DetailSection>
          </div>
        )}
      </div>

      {/* Tier 2, set quieter: the evidence behind tier 1, linked to it by hover. */}
      <DetailTier2 className="text-small">
        <section className="grid grid-cols-[repeat(auto-fit,minmax(min(100%,560px),1fr))] gap-x-12 gap-y-7">
          <div className="min-w-0">
            {/* the witnesses counted, each row its own size: the API states no total of their
                sizes, and the page adds none (lane P1, lane D3's note) */}
            <EvidenceHeading
              title={`Witnesses${tx.witnessesAvailable ? ` · ${witnesses.length}` : ' · unavailable'}`}
            />
            <div data-testid="tx-witness-tab">
              {!tx.witnessesAvailable ? (
                <p className="text-text-dim py-3">Witness bytes are unavailable</p>
              ) : (
                <>
                  <div id={witnessListId}>
                    {witnessItems(witnesses, (w) => !allWitnesses && foldsWitness(w)).map((item) =>
                      item.kind === 'run' ? (
                        <EmptyWitnesses key={`run-${item.from}`} run={item} />
                      ) : (
                        <WitnessRow
                          key={item.w.index}
                          tx={tx}
                          w={item.w}
                          readers={readersOf(item.w.index)}
                          scale={witnessScale}
                          selected={selectedWitness === item.w.index}
                          lit={lit}
                          lookup={lookup}
                          groupNameByHash={groupNameByHash}
                          minerMessage={
                            cellbaseBlock
                              ? {
                                  raw: cellbaseBlock.minerMessage,
                                  text: cellbaseBlock.minerMessageText,
                                }
                              : undefined
                          }
                          onSelect={() =>
                            selectedWitness === item.w.index
                              ? select(null, null)
                              : select(item.w.index, null)
                          }
                          onHover={setHover}
                        />
                      )
                    )}
                  </div>
                  {/* the fold's one control, after the list in both states: it stays mounted,
                      so its focus stays (review of phases C and D, SHOULD 5) */}
                  {witnessItems(witnesses, foldsWitness).some((item) => item.kind === 'run') && (
                    <div className="mt-2">
                      <FoldToggle
                        open={allWitnesses}
                        count={witnesses.length}
                        singular="witness"
                        plural="witnesses"
                        controls={witnessListId}
                        onToggle={setAllWitnesses}
                      />
                    </div>
                  )}
                  {chosenWitness ? (
                    <WitnessInspector tx={tx} witness={chosenWitness} />
                  ) : (
                    <p data-testid="tx-witness-selection-empty" className="text-text-dim mt-3">
                      Select a witness to inspect its bytes.
                    </p>
                  )}
                  {selectedGroup && (
                    <p
                      data-testid="tx-witness-focused-group"
                      className="text-text max-w-measure mt-3 [overflow-wrap:anywhere]"
                    >
                      Focused script group: {selectedGroup}{' '}
                      <button
                        type="button"
                        className="text-text hover:text-jade"
                        onClick={() => select(selectedWitness, null)}
                      >
                        Clear focus
                      </button>
                    </p>
                  )}
                  {selectedWitness !== null && (
                    <button
                      type="button"
                      data-testid="tx-witness-clear-selection"
                      className="text-text-dim hover:text-jade mt-3"
                      onClick={() => select(null, null)}
                    >
                      Clear witness selection
                    </button>
                  )}
                </>
              )}
            </div>
          </div>
          <div className="flex min-w-0 flex-col gap-7">
            <div>
              <EvidenceHeading
                title="Cell deps"
                note={deps ? `${deps.length} · read, not consumed` : 'read, not consumed'}
              />
              {isProvisional ? (
                <p className="max-w-measure py-3">
                  <PendingValue />
                </p>
              ) : depsError ? (
                <p role="alert" className="max-w-measure py-3">
                  {depsError.message}
                </p>
              ) : deps && deps.length === 0 ? (
                <p className="text-text-dim py-3">none</p>
              ) : deps ? (
                deps.map((dep, i) => (
                  <DepRow
                    key={`${dep.outPointTxHash}:${dep.outPointIndex}`}
                    dep={dep}
                    index={i}
                    lit={lit}
                    groupNameByHash={groupNameByHash}
                    onHover={setHover}
                  />
                ))
              ) : (
                <p className="text-text-dim py-3">Resolving cell deps…</p>
              )}
            </div>
            {headerDeps.length > 0 && (
              <div data-testid="tx-header-deps">
                <EvidenceHeading
                  title="Header deps"
                  note={`${headerDeps.length} · read, not consumed`}
                />
                {headerDeps.map((h) => (
                  <div
                    key={h.blockHash}
                    className="border-row-rule flex flex-wrap justify-between gap-3 border-b py-2.5"
                  >
                    {h.blockNumber === null ? (
                      <span className="text-text-dim">block not indexed yet</span>
                    ) : (
                      <Link look="inline" href={`/blocks/${h.blockNumber}`}>
                        {formatBlockNumber(h.blockNumber)}
                      </Link>
                    )}
                    <span className="text-text-dim">
                      <ShortId value={h.blockHash} kind="hash" size="row" />
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>
        </section>

        <DetailSection
          title="Script groups"
          actions={
            <ScriptGroupLegend>
              <span data-testid="tx-groups-ran">
                {isProvisional
                  ? 'will run when committed'
                  : tx.isCellbase
                    ? 'a cellbase runs no script'
                    : hasCycles && cycles !== null
                      ? `${ran} ran, ${cycles.toLocaleString('en-US')} cycles`
                      : `${ran} ran`}
              </span>
            </ScriptGroupLegend>
          }
        >
          <Prose className="-mt-2 mb-3">
            <p>Cells with the same script form a group, and each group runs once.</p>
          </Prose>
          <ScriptGroupMatrix
            txHash={hash}
            inputs={matrix.inputs}
            outputs={matrix.outputs}
            groups={matrix.groups}
            lit={lit}
            onHover={setHover}
          />
        </DetailSection>

        <dl className="grid grid-cols-[repeat(auto-fit,minmax(min(100%,260px),1fr))] gap-x-10 gap-y-5">
          <div className="flex flex-col gap-1">
            <dt className="text-text-dim">Proposed, then committed</dt>
            <dd className="flex flex-col gap-1">
              {tx.isCellbase ? (
                <span className="text-text-dim">
                  never proposed: a cellbase is its block&rsquo;s first transaction
                </span>
              ) : isProvisional ? (
                <PendingValue />
              ) : (
                <>
                  {lifecycle?.proposedIn && tx.blockNumber !== undefined && (
                    <ProposalRuler
                      proposedIn={lifecycle.proposedIn.blockNumber}
                      committedIn={tx.blockNumber}
                      window={lifecycle.commitmentWindow}
                    />
                  )}
                  <span>
                    {lifecycle?.proposedIn && (
                      <>
                        <Link look="inline" href={`/blocks/${lifecycle.proposedIn.blockNumber}`}>
                          {formatBlockNumber(lifecycle.proposedIn.blockNumber)}
                        </Link>{' '}
                        →{' '}
                      </>
                    )}
                    {tx.blockNumber !== undefined && (
                      <Link look="inline" href={`/blocks/${tx.blockNumber}`}>
                        {formatBlockNumber(tx.blockNumber)}
                      </Link>
                    )}
                    {tx.index !== undefined && lifecycle?.commitmentWindow && (
                      <span className="text-text-dim">
                        {' '}
                        · position {tx.index.toLocaleString('en-US')}
                        {lifecycle.commitmentDistance !== null &&
                          lifecycle.commitmentDistance !== undefined && (
                            <>
                              {' '}
                              · committed{' '}
                              {unitNode(formatCount(lifecycle.commitmentDistance, 'block'))} later
                            </>
                          )}{' '}
                        · window{' '}
                        <NoBreak>
                          {lifecycle.commitmentWindow.close}–{lifecycle.commitmentWindow.far}
                        </NoBreak>
                      </span>
                    )}
                  </span>
                </>
              )}
            </dd>
          </div>
          <div className="flex flex-col gap-0.5">
            <dt className="text-text-dim">Size · cycles</dt>
            <dd>
              {tx.txSize === null ? <PendingValue /> : <>{tx.txSize.toLocaleString('en-US')} B</>} ·{' '}
              {hasCycles && cycles !== null ? (
                cycles.toLocaleString('en-US')
              ) : isPaused ? null : isCalculating ? (
                'Calculating…'
              ) : hasFailed ? (
                'Calculation failed'
              ) : isProvisional ? (
                <PendingValue />
              ) : tx.isCellbase ? (
                'Cellbase does not execute scripts'
              ) : (
                'Not available'
              )}
              {isPaused && (
                <span className={hasCycles ? 'text-text-dim mt-1 block' : 'text-text-dim'}>
                  Paused: indexer is on a deep fork
                </span>
              )}
              {tx.feeRate !== null && (
                <span className="text-text-dim mt-1 block">
                  fee rate{' '}
                  {exactUnsignedInteger(tx.feeRate, `${hash}: fee rate`).toLocaleString('en-US')}{' '}
                  shannon/KB
                </span>
              )}
            </dd>
          </div>
          {(tx.outputs ?? []).flatMap((cell, i) => {
            const lockFact = (
              out: { lock: NonNullable<typeof cell.lock>; address?: string | null },
              at: number
            ) => {
              const read = readPart(() =>
                lockWordsText(
                  lockWords(
                    cellLockFacts(
                      {
                        lock: out.lock,
                        name: registeredName(lookup, out.lock.codeHash),
                        address: out.address ?? null,
                      },
                      `Transaction ${tx.hash} output ${at}`
                    ),
                    'column'
                  )!,
                  'row'
                )
              );
              return read.error !== undefined ? (
                <PartReadError
                  what={`the lock of output ${at}`}
                  where={`transaction ${tx.hash}`}
                  error={read.error}
                />
              ) : (
                read.value
              );
            };
            // R-8 (TX-20): an output lock read by a registry field layout, or — revised R-8 — one
            // whose args do not fit its layout, with the API's reason in place of the fields.
            const line = lockArgsLine(cell.lockArgsDecoded, cell.lockArgsDecodeError, 'row');
            const shown =
              cell.lock &&
              line !== null &&
              (cell.lockArgsDecodeError ||
                (cell.lockArgsDecoded && isFieldLayoutLock(cell.lockArgsDecoded)));
            if (!shown || !cell.lock) return [];
            return [
              <div
                key={`lock-${i}`}
                data-testid={`tx-fact-output-lock-${i}`}
                className="flex flex-col gap-0.5"
              >
                <dt className="text-text-dim">
                  {cell.display?.kind === 'object'
                    ? `New owner of the object in output ${i}`
                    : `Lock of output ${i}`}
                </dt>
                <dd>
                  {/* the lock as its tile names it, from the one decision (B4), here a fact that
                      is never empty: the default lock by its name; a refusal fails the fact in
                      its place (D7) */}
                  {lockFact({ lock: cell.lock, address: cell.address }, i)},{' '}
                  <span className={cell.lockArgsDecodeError ? 'text-text-dim' : undefined}>
                    {unitText(line)}
                  </span>
                </dd>
              </div>,
            ];
          })}
          <div className="flex flex-col gap-0.5 sm:col-span-2">
            <dt className="text-text-dim">Capacity</dt>
            <dd>{capacityFact}</dd>
            {knowledgeDelta !== null && (
              <dd>
                {/* "occupied" always carries its unit, the pair's once after it (B4) */}
                occupied {occupiedPairText(occupiedIn!, occupiedOut!, `transaction ${hash}`)}{' '}
                <span className="text-text-dim">
                  · common knowledge {occupiedBytesDelta(knowledgeDelta)}
                </span>
              </dd>
            )}
          </div>
        </dl>

        <details open={graphOpen} className="border-row-rule border-t pt-4">
          <summary className="text-text-dim hover:text-text text-small cursor-pointer">
            Raw data
          </summary>
          <div className="text-small mt-4 min-w-0 overflow-x-auto">{raw}</div>
        </details>
      </DetailTier2>
    </div>
  );
}
