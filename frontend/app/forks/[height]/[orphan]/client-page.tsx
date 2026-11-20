'use client';

import type { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import Link from '@/components/ui/link';
import { HeadId } from '@/components/ui/head-id';
import { NoBreak } from '@/components/ui/no-break';
import { ShortId } from '@/components/ui/short-id';
import { SHORT_FORMS, shortenHash, type ShortSize } from '@/lib/shorten';
import { columnPx, fontPx, textPx } from '@/lib/type-metrics';
import {
  WIDEST_BLOCK_NUMBER,
  WIDEST_COUNT,
  WIDEST_TX_VALUE_SUMMARY,
  shortChars,
} from '@/lib/list-bounds';
import { useParams } from '@/src/navigation';
import {
  api,
  isNotFoundApiError,
  type CanonicalBranchBlock,
  type ForkEventRef,
  type OrphanedBlock,
  type OrphanedTransaction,
  type ReorgDetail,
  type ReorgEvent,
} from '@/lib/api';
import { isBadRequestApiError } from '@/lib/api-errors';
import { NotFoundPage } from '@/components/not-found-page';
import { ReadFailure } from '@/components/read-failure';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import { CELL_GAP_PX, ListTable } from '@/components/ui/list-table';
import { Dotted, Labelled, PhoneRow } from '@/components/ui/phone-row';
import { ROW_SEAL, RowSeal } from '@/components/ui/row-seal';
import { ForkTree, TIP_MARKS } from '@/components/forks/fork-tree';
import { formatBlockNumber } from '@/lib/format-block';
import { BlockLink, SentenceNodes, type SentencePart } from '@/components/ui/block-link';
import { formatCount, countNoun } from '@/lib/format-count';
import {
  blockClock,
  forkRefHref,
  missingAddressWords,
  orphanFateWords,
  parseForkRoute,
} from '@/lib/fork-format';
import { MinerLink, minerName } from '@/components/forks/miner-link';
import { forkTreeModel } from '@/lib/fork-tree';
import { formatUtc8Date, formatUtc8Time } from '@/lib/utils';
import { DocumentTitle } from '@/lib/use-document-title';
import { Utc8DateTime } from '@/components/ui/utc8-date';
import { unitNode } from '@/components/ui/units';
import { useTightBelowSm } from '@/hooks/useTightBelowSm';

type MinerBlock = {
  number: number;
  minerLockHash: string | null;
  minerAddress: string | null;
  minerPool: string | null;
};

/** A block's miner address; the API gives one exactly when it knows the miner's lock (R2b-b-12). */
function minerAddressOf(block: MinerBlock): string {
  if (block.minerLockHash === null || block.minerAddress === null)
    throw new Error(
      `Fork block ${formatBlockNumber(block.number)}: miner lock ${block.minerLockHash} without an address ${block.minerAddress}`
    );
  return block.minerAddress;
}

function Miner({ block, size }: { block: MinerBlock; size: ShortSize }) {
  if (block.minerLockHash === null && block.minerAddress === null)
    return <span className="text-text-dim">witness unavailable</span>;
  return (
    <MinerLink address={minerAddressOf(block)} pool={block.minerPool} size={size} look="inline" />
  );
}

/**
 * A full hash in tier 2 (U-16; lane P4: one id is wrapped, never scrolled): set as a head's id is
 * (HeadId), one line where its measured line holds it, else equal unbreakable lines — a hash's two
 * halves, `0x` and 32 digits over 32 — never broken where the browser pleases and never a box that
 * scrolls sideways. At the facts' 13 px a whole hash is 66 × 7.8 = 514.8 px and a half 34 × 7.8 =
 * 265.2: the tier's columns (auto-fill of 581 px) give a hash's cell 474.6 px at 1280 (588 less the
 * 101.4 px label column, "first dropped", and the 12 px gap: two halves), 862.6 at 1024 and 494.6
 * at 640 (608 less the label and the gap: two halves), and on a phone the facts stand in one
 * column, the hash under its label: 358 or 328 px, two halves.
 */
const FullHash = ({ hash }: { hash: string }) => <HeadId value={hash} kind="hash" />;
/** A canonical block's height, linked: a row's key, or a value inside a line or a cell. */
const blockAt = (number: number, look: 'key' | 'inline') => (
  <Link href={`/blocks/${number}`} look={look}>
    {formatBlockNumber(number)}
  </Link>
);

/**
 * The two tables' columns, each budgeted from the widest value it holds (readability F3-e) at the
 * cells' small step (13 px), a head or a unit at the meta step (12) and the gap after every column
 * but the last.
 */
const cell = (chars: number) => textPx(chars, fontPx('small'));
/** A dropped block's height and its prime, "#99,999,999′". */
const DROPPED_HEIGHT = `${WIDEST_BLOCK_NUMBER}′`;

/** What `parseForkRoute` accepts, as the not-an-id page says it. */
const FORK_ROUTE_RULE =
  'a fork is named by its height and the first 8 hex digits of the first orphan’s hash.';

export default function ForkDetailPage() {
  const params = useParams();
  const route = parseForkRoute(String(params.height ?? ''), String(params.orphan ?? ''));
  const query = useQuery({
    queryKey: ['fork', route?.height, route?.orphan],
    queryFn: () => api.getForkAt(route!.height, route!.orphan),
    enabled: route !== null,
  });
  if (!route) return <NotFoundPage notAnId={{ kind: 'fork', reason: FORK_ROUTE_RULE }} />;
  const address = `/forks/${route.height}/${route.orphan}`;
  if (!query.data && isNotFoundApiError(query.error)) {
    return (
      <NotFoundPage
        entity={{
          kind: 'fork',
          identifier: `${route.height}/${route.orphan}`,
          checkedEndpoint: address,
        }}
      />
    );
  }
  if (!query.data && isBadRequestApiError(query.error))
    return <NotFoundPage notAnId={{ kind: 'fork', reason: query.error.apiMessage }} />;
  const mismatch = query.data ? answerMismatch(address, route, query.data) : null;
  return (
    <div className="min-h-screen">
      <main className="container mx-auto min-w-0 max-w-[1280px] pb-[72px] pt-7">
        <DocumentTitle parts={[formatBlockNumber(route.height), route.orphan, 'fork']} />
        {/* awaited, not isLoading: a read paused offline or between retries is still awaited */}
        {isAwaited(query) && <p role="status">Loading fork evidence…</p>}
        {query.error && <ReadFailure what="fork" error={query.error} />}
        {mismatch && (
          <p role="alert" className="text-rouge text-body max-w-measure [overflow-wrap:anywhere]">
            {mismatch}
          </p>
        )}
        {query.data &&
          !mismatch &&
          (query.data.evidenceStatus === 'available' ? (
            <ForkEvidence data={query.data} />
          ) : (
            <StoredFork event={query.data.event} />
          ))}
      </main>
    </div>
  );
}

/**
 * Why an answer is not the fork its address names (contracts W5-2): the event must be the
 * recorded switch at that fork point and first orphan; branch evidence, when the node store is
 * open, must start at that first orphan, and without the node store there is none.
 */
function answerMismatch(
  address: string,
  route: { height: number; orphan: string },
  data: ReorgDetail
): string | null {
  const { event, evidenceStatus, orphanedBlocks } = data;
  const at = formatBlockNumber(event.forkPointNumber);
  if (event.firstOrphanStatus !== 'recorded' || event.firstOrphanHash === null)
    return `API answered ${address} with ${event.eventType === 'deep' ? 'a deep detection' : 'a switch'} at ${at}, which has no first orphan to be addressed by`;
  if (event.forkPointNumber !== route.height || event.firstOrphanHash.slice(2, 10) !== route.orphan)
    return `API answered ${address} with the fork at ${at} whose first orphan is ${shortenHash(event.firstOrphanHash, 'row')}`;
  if (evidenceStatus === 'node_reader_unavailable') {
    const lists = [
      orphanedBlocks,
      data.orphanedTransactions,
      data.adoptedBranch,
      data.canonicalBranch,
    ];
    return lists.some((list) => list.length > 0) || data.canonicalTipNumber !== null
      ? `API answered ${address} with node_reader_unavailable but with branch evidence`
      : null;
  }
  if (evidenceStatus !== 'available')
    return `API answered ${address} with unknown evidence status ${String(evidenceStatus)}`;
  const first = orphanedBlocks[0];
  if (!first) return `API answered ${address} with branch evidence but no orphaned block`;
  if (first.hash !== event.firstOrphanHash)
    return `API answered ${address} with first dropped block ${shortenHash(first.hash, 'row')}, but its event names first orphan ${shortenHash(event.firstOrphanHash, 'row')}`;
  return null;
}

/** The recorded first dropped block of an addressable switch (checked by `answerMismatch`). */
function firstOrphanOf(event: ReorgEvent): string {
  if (event.firstOrphanHash === null)
    throw new Error(
      `Fork ${formatBlockNumber(event.forkPointNumber)} has no recorded first orphan (${event.firstOrphanStatus})`
    );
  return event.firstOrphanHash;
}

function ForkHeading({ event }: { event: ReorgEvent }) {
  return (
    <>
      <nav aria-label="Breadcrumb" className="text-small">
        <Link href="/forks" look="inline">
          Forks
        </Link>
      </nav>
      <div className="mt-2.5 flex flex-wrap items-baseline gap-3">
        {/* PB-06 / PA-32: an entity h1, one type step smaller below md (30 → 26 px). */}
        <h1 className="text-text-bright text-entity max-md:text-index font-medium">
          Fork at {formatBlockNumber(event.forkPointNumber)}
        </h1>
        <span className="text-text-dim text-body">
          {unitNode(formatCount(event.depth, 'block'))} deep · detected{' '}
          <Utc8DateTime timestamp={event.detectedAt} /> · this node switched once
        </span>
      </div>
    </>
  );
}

/**
 * contracts W5-2: the API runs without the node reader, so the address resolved from this node's
 * stored event alone and there is no branch evidence. Say so, and tell what the event records.
 */
function StoredFork({ event }: { event: ReorgEvent }) {
  // U-16: each shortened hash is one unit; the story is the page's lede: prose, the row size (R9)
  const story = (
    <>
      This node switched from {formatBlockNumber(event.oldTipNumber)}{' '}
      <ShortId value={event.oldTipHash} kind="hash" size="row" /> to{' '}
      {formatBlockNumber(event.newTipNumber)}{' '}
      <ShortId value={event.newTipHash} kind="hash" size="row" />
      {unitNode(
        ` and rolled back ${formatCount(event.orphanedBlocksCount, 'block')} and ${formatCount(event.orphanedTxsCount, 'transaction')}, starting with ${formatBlockNumber(event.forkPointNumber + 1)}′.`
      )}
      {event.revertedByEvent ? ' A later switch on this node reversed it.' : ''}
    </>
  );
  return (
    <div className="min-w-0">
      <ForkHeading event={event} />
      <p
        data-testid="fork-narrative"
        className="text-text text-body max-w-measure mt-3 leading-relaxed"
      >
        {story}
      </p>
      <p role="status" className="text-text text-body max-w-measure mt-3 leading-relaxed">
        Branch evidence needs the node store: this API runs without the node reader, so the blocks
        of both branches, their miners and what became of them are not shown. Below is what this
        node recorded when it switched.
      </p>
      <DetailTier2>
        <div className="grid min-w-0 grid-cols-[repeat(auto-fill,minmax(min(100%,581px),1fr))] items-start gap-x-14 gap-y-10">
          <StoredEvent event={event} />
        </div>
      </DetailTier2>
    </div>
  );
}

/** The story of one switch, sentence by sentence, from the API's facts only. */
function narrative(data: ReorgDetail): SentencePart[][] {
  const { event, orphanedBlocks, orphanedTransactions } = data;
  // Lane L: sentences as parts, so a canonical block they name renders as a link.
  const parts: SentencePart[][] = [];
  const push = (...sentence: SentencePart[]) => parts.push(sentence);
  orphanedBlocks
    .filter((block) => block.competingBlock && block.timestampGapMs !== null)
    .forEach((block, i) => {
      const gap = `${Math.abs(block.timestampGapMs!).toLocaleString('en-US')} ms apart`;
      const same =
        block.minerLockHash !== null && block.competingBlock!.minerLockHash === block.minerLockHash;
      const miner = same
        ? i === 0
          ? `, both by the same miner (${minerName(minerAddressOf(block), block.minerPool, 'row')})`
          : ', again both by it'
        : '';
      push(
        i === 0
          ? `On ${formatUtc8Date(block.timestamp)} two blocks were found at ${formatBlockNumber(block.number)}, ${gap}${miner}.`
          : `At ${formatBlockNumber(block.number)} two more, ${gap}${miner}.`
      );
    });
  const back = orphanedBlocks.filter((block) => block.fate === 'canonical_now');
  for (const block of back)
    push('The block this node dropped at ', { block: block.number }, ' is back on the main chain.');
  const lost = orphanedBlocks.length - back.length;
  if (lost > 0)
    push(
      `This node had followed ${countNoun(lost, 'the block', 'the blocks')} that lost and switched to the other branch.`
    );
  if (event.revertedByEvent) push('A later switch on this node reversed it.');
  const byInclusion = new Map<number, OrphanedBlock[]>();
  for (const block of orphanedBlocks)
    if (block.fate === 'included_as_uncle' && block.uncleIn)
      byInclusion.set(block.uncleIn.blockNumber, [
        ...(byInclusion.get(block.uncleIn.blockNumber) ?? []),
        block,
      ]);
  for (const [number, blocks] of byInclusion) {
    const which =
      blocks.length === 1
        ? lost === 1
          ? 'the dropped block'
          : `${formatBlockNumber(blocks[0].number)}′`
        : blocks.length === 2 && lost === 2
          ? 'both dropped blocks'
          : `${blocks.length} dropped blocks`;
    push(
      { block: number },
      ` included ${which} as ${countNoun(blocks.length, 'an uncle', 'uncles')}.`
    );
  }
  for (const block of orphanedBlocks) {
    if (block.fate === 'not_included_as_uncle')
      push(`${formatBlockNumber(block.number)}′ was not taken as an uncle.`);
    if (block.fate === 'uncle_fate_pending')
      push(`${formatBlockNumber(block.number)}′ can still be taken as an uncle.`);
  }
  const droppedHashes = new Set(
    orphanedBlocks.filter((block) => block.fate !== 'canonical_now').map((block) => block.hash)
  );
  if (droppedHashes.size > 0) {
    const txs = orphanedTransactions.filter(
      (tx) => tx.txIndex > 0 && droppedHashes.has(tx.blockHash)
    );
    if (txs.length === 0)
      push(
        droppedHashes.size === 1
          ? 'The dropped block held only its cellbase, so no transaction was undone.'
          : 'The dropped blocks held only their cellbases, so no transaction was undone.'
      );
    else {
      const committed = txs.filter(
        (tx) => tx.fate === 'committed_later' || tx.fate === 'committed_on_canonical_branch'
      ).length;
      const waiting = txs.filter((tx) => tx.fate === 'not_committed_as_of_tip').length;
      push(
        `${committed} of its ${formatCount(txs.length, 'transaction')} ${countNoun(committed, 'was', 'were')} committed later.` +
          (waiting
            ? ` ${waiting} ${countNoun(waiting, 'is', 'are')} not committed as of the tip.`
            : '')
      );
    }
  }
  return parts;
}

interface BlockRow {
  key: string;
  number: number;
  hash: string;
  timestamp: string;
  minerLockHash: string | null;
  minerAddress: string | null;
  minerPool: string | null;
  transactionsCount: number;
  dropped: boolean;
  fate: ReactNode;
}

/**
 * A block row's stone (lane D2; the UI review's U-03 and ruling R8): its hash and the transactions
 * it held, the API's count; a block on the main chain pressed in the cinnabar, a dropped one (an
 * uncle, an orphan, a branch adopted then dropped) in the dead ink, as a consumed cell's is.
 */
function BlockRowStone({ row }: { row: BlockRow }) {
  return (
    <RowSeal
      id={{ kind: 'block', hash: row.hash, txs: row.transactionsCount }}
      state={row.dropped ? 'ink' : 'pressed'}
    />
  );
}

function blockRows(data: ReorgDetail): BlockRow[] {
  const orphans = new Map(data.orphanedBlocks.map((block) => [block.hash, block]));
  const canonical = new Set(data.canonicalBranch.map((block) => block.hash));
  const main = data.canonicalBranch.map(
    (block: CanonicalBranchBlock): BlockRow => ({
      key: block.hash,
      ...block,
      dropped: false,
      fate: orphans.has(block.hash) ? 'back on the main chain' : 'main chain',
    })
  );
  const dropped = data.orphanedBlocks
    .filter((block) => !canonical.has(block.hash))
    .map(
      (block): BlockRow => ({
        key: block.hash,
        number: block.number,
        hash: block.hash,
        timestamp: block.timestamp,
        minerLockHash: block.minerLockHash,
        minerAddress: block.minerAddress,
        minerPool: block.minerPool,
        transactionsCount: block.transactionsCount,
        dropped: true,
        // a fate's label quiet, its block the value (R3): "uncle in #N", its words free to break
        // in the table's Fate column as anywhere (ListTable: words may always break between them)
        fate: block.uncleIn ? (
          <span>
            <span className="text-text-dim">uncle in</span>{' '}
            {blockAt(block.uncleIn.blockNumber, 'inline')}
          </span>
        ) : (
          orphanFateWords(block.fate)
        ),
      })
    );
  const adopted = data.adoptedBranch
    .filter((block) => !canonical.has(block.hash) && !orphans.has(block.hash))
    .map(
      (block): BlockRow => ({
        key: block.hash,
        ...block,
        dropped: true,
        fate: 'adopted then, dropped since',
      })
    );
  return [...main, ...dropped, ...adopted];
}

/** Another switch, linked by its D7 address when it has one (R2b-b). */
function EventRef({ target, text }: { target: ForkEventRef; text: string }) {
  const href = forkRefHref(target);
  return href ? (
    <Link href={href} look="inline">
      {text}
    </Link>
  ) : (
    <span>
      {text}{' '}
      <span className="text-text-dim">({missingAddressWords(target.firstOrphanStatus)})</span>
    </span>
  );
}

/** Tier 2: this node's fork event exactly as the store keeps it. */
function StoredEvent({
  event,
  onMain,
}: {
  event: ReorgEvent;
  /** Lane L: whether a block is on the main chain today; unknown without branch evidence. */
  onMain?: (hash: string) => boolean;
}) {
  return (
    <DetailSection title="This node’s fork event, as stored">
      <div className="text-small grid grid-cols-1 gap-x-3 gap-y-1.5 sm:grid-cols-[max-content_minmax(0,1fr)]">
        <span className="text-text-dim">fork point</span>
        <span className="min-w-0">
          {blockAt(event.forkPointNumber, 'inline')} <FullHash hash={event.forkPointHash} />
        </span>
        <span className="text-text-dim">first dropped</span>
        <span className="min-w-0">
          {formatBlockNumber(event.forkPointNumber + 1)}′ <FullHash hash={firstOrphanOf(event)} />
        </span>
        <span className="text-text-dim">detected</span>
        <span>
          <Utc8DateTime timestamp={event.detectedAt} />
        </span>
        <span className="text-text-dim">depth</span>
        <span>{unitNode(formatCount(event.depth, 'block'))}</span>
        <span className="text-text-dim">old tip</span>
        <span className="min-w-0">
          {formatBlockNumber(event.oldTipNumber)} <FullHash hash={event.oldTipHash} />
        </span>
        <span className="text-text-dim">new tip</span>
        <span className="min-w-0">
          {onMain?.(event.newTipHash) ? (
            <BlockLink number={event.newTipNumber} look="inline" />
          ) : (
            formatBlockNumber(event.newTipNumber)
          )}{' '}
          <FullHash hash={event.newTipHash} />
        </span>
        {event.supersededByTipHash && (
          <>
            <span className="text-text-dim">superseded</span>
            <span className="min-w-0">
              before replay; the canonical {formatBlockNumber(event.newTipNumber)} was{' '}
              <FullHash hash={event.supersededByTipHash} />
            </span>
          </>
        )}
        {event.reconcileInterrupted && (
          <>
            <span className="text-text-dim">reconcile</span>
            <span>interrupted by a later rollback</span>
          </>
        )}
        {event.revertedByEvent && (
          <>
            <span className="text-text-dim">later</span>
            <EventRef
              target={event.revertedByEvent}
              text="a later switch on this node reversed it"
            />
          </>
        )}
        {event.reversesEvents.map((ref, index) => (
          <span key={index} className="contents">
            <span className="text-text-dim">earlier</span>
            <EventRef
              target={ref}
              text={`it reversed an earlier switch at ${formatBlockNumber(ref.forkPointNumber)}`}
            />
          </span>
        ))}
      </div>
    </DetailSection>
  );
}

function ForkEvidence({ data }: { data: ReorgDetail }) {
  const { event, orphanedBlocks, orphanedTransactions } = data;
  const model = forkTreeModel(data);
  const onMain = (hash: string) => model.main.some((tile) => tile.hash === hash);
  const where = (hash: string) =>
    onMain(hash) ? 'on what is the main chain today' : 'on a branch that lost';
  const rows = blockRows(data);
  const inclusions = [
    ...new Set(
      orphanedBlocks.flatMap((block) => (block.uncleIn ? [block.uncleIn.blockNumber] : []))
    ),
  ];
  const nonCellbase = orphanedTransactions.filter((tx) => tx.txIndex > 0);
  // The blocks fall to their phone rows under the table's 820 px (at 640 and 768 too), beside the
  // narrative and the tree, which write the same miner at the row size: tight only below sm (R9).
  const phoneSize = useTightBelowSm();
  return (
    <div className="min-w-0">
      <ForkHeading event={event} />
      <p
        data-testid="fork-narrative"
        className="text-text text-body max-w-measure mt-3 leading-relaxed"
      >
        <SentenceNodes parts={narrative(data).flatMap((p, i) => (i ? [' ', ...p] : p))} />
      </p>

      <div className="mt-11 flex flex-col gap-11">
        {/* The commands run on what they show (rubbing × Song print, phase 5): the switch's depth,
            the blocks listed, the dropped transactions listed. */}
        <DetailSection
          title="The two branches"
          args={`depth ${event.depth.toLocaleString('en-US')}`}
          actions={
            <span className="text-text-dim text-meta flex flex-wrap gap-x-4 gap-y-1">
              <span>dashed: blocks that lost</span>
              <span className="text-jade">→ included as an uncle</span>
              <span className="text-text-bright">
                {model.marks.some((mark) => mark.n === 3) ? '① ② ③' : '① ②'} this node’s tip, in
                turn
              </span>
            </span>
          }
        >
          <div className="min-w-0 pt-6">
            <ForkTree model={model} />
          </div>
          <div className="text-body mt-2 grid grid-cols-[34px_minmax(0,1fr)] gap-x-2.5 gap-y-1.5 sm:grid-cols-[34px_190px_minmax(0,1fr)]">
            <span className="text-text-bright">{TIP_MARKS[1]}</span>
            <span className="text-text-dim">
              before {unitNode(formatUtc8Time(event.detectedAt))}
            </span>
            {/* R3: the step's hash is a value, in the step's reading ink */}
            <span className="text-text col-span-2 sm:col-span-1">
              this node’s tip is {formatBlockNumber(event.oldTipNumber)}{' '}
              <ShortId value={event.oldTipHash} kind="hash" size="row" /> {where(event.oldTipHash)}
            </span>
            <span className="text-text-bright">{TIP_MARKS[2]}</span>
            <span className="text-text-dim">{unitNode(formatUtc8Time(event.detectedAt))}</span>
            <span className="text-text col-span-2 sm:col-span-1">
              the node switches to{' '}
              {onMain(event.newTipHash) ? (
                <BlockLink number={event.newTipNumber} look="inline" />
              ) : (
                formatBlockNumber(event.newTipNumber)
              )}{' '}
              <ShortId value={event.newTipHash} kind="hash" size="row" /> and drops{' '}
              {orphanedBlocks.map((block) => `${formatBlockNumber(block.number)}′`).join(' and ')}{' '}
              {/* the dot goes with its item to a new line (PA-25) */}
              {/* the depth is a value, the line's reading ink; "· depth" its label */}
              <NoBreak>
                <span className="text-text-dim">· depth</span> {event.depth}
              </NoBreak>
            </span>
            {event.revertedByEvent && (
              <span data-testid="fork-step-3" className="contents">
                <span className="text-text-bright">{TIP_MARKS[3]}</span>
                <span className="text-text-dim">later</span>
                <span className="text-text col-span-2 sm:col-span-1">
                  <EventRef target={event.revertedByEvent} text="a later switch" /> moves this node
                  to {formatBlockNumber(event.revertedByEvent.newTip.number)}{' '}
                  <ShortId value={event.revertedByEvent.newTip.hash} kind="hash" size="row" />{' '}
                  {where(event.revertedByEvent.newTip.hash)}
                </span>
              </span>
            )}
          </div>
        </DetailSection>

        <DetailSection title="Blocks" args={rows.length.toLocaleString('en-US')}>
          <ListTable
            label="Blocks"
            rows={rows}
            rowKey={(row) => row.key}
            columns={[
              {
                key: 'height',
                label: 'Height',
                // the block's row stone and its gap (ROW_SEAL.block.footprint, 24; lane D2), a
                // dropped height and its prime (12 characters, 93.6) and the gap: 134; it was 120,
                // then 110 without the stone
                minWidth: columnPx(
                  ROW_SEAL.block.footprint + cell(DROPPED_HEIGHT.length) + CELL_GAP_PX
                ),
                render: (row) => (
                  <span className="flex items-start">
                    <BlockRowStone row={row} />
                    {row.dropped ? `${formatBlockNumber(row.number)}′` : blockAt(row.number, 'key')}
                  </span>
                ),
              },
              {
                key: 'hash',
                label: 'Hash',
                // the hash at the row size (17 characters, 132.6) and the gap; it was 170. A value,
                // not a link: the reading ink (it was the boards' link ink on a span that is no link)
                minWidth: columnPx(cell(shortChars(SHORT_FORMS.row.hash)) + CELL_GAP_PX),
                render: (row) => <ShortId value={row.hash} kind="hash" size="row" />,
              },
              {
                key: 'time',
                label: 'Time (UTC+8)',
                // the clock to the millisecond, "09:12:32.761" (12 characters, 93.6), wider than
                // the head's "(UTC+8)", and the gap; it was 120
                minWidth: columnPx(
                  Math.max(
                    cell(blockClock('1970-01-01T00:00:00Z').length),
                    textPx('(UTC+8)'.length, fontPx('meta'))
                  ) + CELL_GAP_PX
                ),
                // a time is a value: the reading ink (R3)
                render: (row) => blockClock(row.timestamp),
              },
              {
                key: 'miner',
                label: 'Miner',
                // The flexible column (a pool name has no bound): an unnamed miner at the row size
                // (13 characters, 101.4) and the gap at its minimum, the rest of the container
                // beyond the sum. It was 150.
                minWidth: columnPx(cell(shortChars(SHORT_FORMS.row.address)) + CELL_GAP_PX),
                width: 'auto',
                render: (row) => <Miner block={row} size="row" />,
              },
              {
                key: 'txs',
                label: 'Txs',
                // a block's transactions, the consensus count WIDEST_COUNT states (at most 5,146
                // in a block of 597,000 bytes): four digits, "9,999" (39), and the gap; it was 60
                minWidth: columnPx(cell(WIDEST_COUNT.length) + CELL_GAP_PX),
                numeric: true,
                render: (row) => row.transactionsCount.toLocaleString('en-US'),
              },
              {
                key: 'fate',
                label: 'Fate',
                // the widest fate on its line, "uncle inclusion still possible" (30 characters,
                // 234); the last column, no gap. It was 200.
                minWidth: columnPx(cell('uncle inclusion still possible'.length)),
                render: (row) => row.fate,
              },
            ]}
            // No board for this list: board L1's rules — identity and time, who mined it and how
            // many transactions, then the fate in full.
            mobileRow={(row) => (
              <PhoneRow
                testId="fork-block-row-mobile"
                mark={<BlockRowStone row={row} />}
                lines={[
                  {
                    key: 'identity',
                    tone: 'head',
                    left: (
                      <>
                        <span className="font-medium">
                          {row.dropped
                            ? `${formatBlockNumber(row.number)}′`
                            : blockAt(row.number, 'key')}
                        </span>{' '}
                        <span className="text-small text-text">
                          <ShortId value={row.hash} kind="hash" size={phoneSize} />
                        </span>
                      </>
                    ),
                    right: blockClock(row.timestamp),
                  },
                  {
                    key: 'miner',
                    tone: 'body',
                    left: (
                      <>
                        <span className="text-text-dim">miner </span>
                        <Miner block={row} size={phoneSize} />
                      </>
                    ),
                    right: formatCount(row.transactionsCount, 'tx'),
                  },
                  { key: 'fate', tone: 'quiet', left: row.fate },
                ]}
              />
            )}
          />
          {/* a sentence that explains reads in the reading ink (R3) */}
          <p className="text-text text-small max-w-measure mt-2">
            An uncle earns its miner nothing; it counts toward the uncle rate that sets the
            difficulty.
          </p>
        </DetailSection>

        {nonCellbase.length > 0 && (
          <DetailSection
            title="Dropped transactions"
            args={nonCellbase.length.toLocaleString('en-US')}
          >
            <ListTable
              label="Dropped branch transactions"
              rows={nonCellbase}
              rowKey={(tx: OrphanedTransaction) => `${tx.blockHash}-${tx.txIndex}`}
              columns={[
                {
                  key: 'tx',
                  label: 'Transaction',
                  // The flexible column: the full hash breaks anywhere, so at its minimum two lines
                  // of 33 characters (257.4) and the gap; one line where the container gives the
                  // hash's 66. It was 300.
                  minWidth: columnPx(cell(66 / 2) + CELL_GAP_PX),
                  width: 'auto',
                  render: (tx: OrphanedTransaction) =>
                    tx.committedAt ? (
                      <Link href={`/tx/${tx.hash}`} look="key" className="break-all">
                        {tx.hash}
                      </Link>
                    ) : (
                      <span className="break-all">{tx.hash}</span>
                    ),
                },
                {
                  key: 'origin',
                  label: 'Dropped block',
                  // a dropped height and its prime (93.6) and the gap
                  minWidth: columnPx(cell(DROPPED_HEIGHT.length) + CELL_GAP_PX),
                  render: (tx: OrphanedTransaction) => `${formatBlockNumber(tx.blockNumber)}′`,
                },
                {
                  key: 'fate',
                  label: 'Fate now',
                  // the widest fate on its line, "committed on canonical branch" (29 characters,
                  // 226.2), and the gap; it was 170
                  minWidth: columnPx(cell('committed on canonical branch'.length) + CELL_GAP_PX),
                  render: (tx: OrphanedTransaction) => tx.fate.replaceAll('_', ' '),
                },
                {
                  key: 'commit',
                  label: 'Committed in',
                  // "#99,999,999" (85.8) and the gap; it was 130
                  minWidth: columnPx(cell(WIDEST_BLOCK_NUMBER.length) + CELL_GAP_PX),
                  render: (tx: OrphanedTransaction) =>
                    tx.committedAt ? blockAt(tx.committedAt.blockNumber, 'inline') : '—',
                },
                {
                  key: 'capacity',
                  label: 'Output capacity',
                  // A list column, so the summary form (R5; lane P4: it was the column form, every
                  // decimal padded): an output under a billion CKB, whole from 1,000 up,
                  // "999,999,999" (85.8), and its unit " CKB" at 12 px (28.8), wider than
                  // "Unavailable"; the last column, no gap: 115 (it was 185, and 170 before)
                  minWidth: columnPx(
                    Math.max(
                      cell(WIDEST_TX_VALUE_SUMMARY.length) + textPx(' CKB'.length, fontPx('meta')),
                      cell('Unavailable'.length)
                    )
                  ),
                  numeric: true,
                  render: (tx: OrphanedTransaction) =>
                    tx.totalCapacity == null ? (
                      'Unavailable'
                    ) : (
                      <CkbAmount
                        form="summary"
                        shannons={BigInt(tx.totalCapacity)}
                        signed={false}
                        tone="plain"
                      />
                    ),
                },
              ]}
              // No board: the full hash first (it is how the transaction is found again), then
              // where it was dropped and what became of it, then where it was committed.
              mobileRow={(tx: OrphanedTransaction) => (
                <PhoneRow
                  testId="fork-tx-row-mobile"
                  lines={[
                    {
                      key: 'hash',
                      tone: 'head',
                      left: tx.committedAt ? (
                        <Link href={`/tx/${tx.hash}`} look="key" className="text-small break-all">
                          {tx.hash}
                        </Link>
                      ) : (
                        <span className="text-small break-all">{tx.hash}</span>
                      ),
                    },
                    {
                      key: 'fate',
                      tone: 'body',
                      left: (
                        <Dotted
                          parts={[
                            `dropped in ${formatBlockNumber(tx.blockNumber)}′`,
                            <span key="f" className="text-text-bright">
                              {tx.fate.replaceAll('_', ' ')}
                            </span>,
                          ]}
                        />
                      ),
                      right:
                        tx.totalCapacity == null ? (
                          'output capacity unavailable'
                        ) : (
                          <CkbAmount
                            form="summary"
                            shannons={BigInt(tx.totalCapacity)}
                            signed={false}
                            tone="plain"
                          />
                        ),
                    },
                    {
                      key: 'committed',
                      tone: 'quiet',
                      // the label quiet, its block the value (R3)
                      left: tx.committedAt ? (
                        <Labelled label="committed in">
                          {blockAt(tx.committedAt.blockNumber, 'inline')}
                        </Labelled>
                      ) : (
                        'not committed'
                      ),
                    },
                  ]}
                />
              )}
            />
          </DetailSection>
        )}
      </div>

      <DetailTier2>
        <div className="grid min-w-0 grid-cols-[repeat(auto-fill,minmax(min(100%,581px),1fr))] items-start gap-x-14 gap-y-10">
          <StoredEvent event={event} onMain={onMain} />
          {inclusions.map((number) => {
            const uncles = orphanedBlocks.filter((block) => block.uncleIn?.blockNumber === number);
            const includer = data.canonicalBranch.find((block) => block.number === number);
            return (
              <DetailSection
                key={number}
                title={
                  <>
                    Uncles in <BlockLink number={number} look="inline" />
                  </>
                }
              >
                <div className="text-small grid grid-cols-1 gap-x-3 gap-y-1.5 sm:grid-cols-[max-content_minmax(0,1fr)]">
                  {uncles.map((block, i) => (
                    <span key={block.hash} className="contents">
                      <span className="text-text-dim">uncle {i}</span>
                      {/* as the stored event's facts: the dropped height, then its hash in
                          designed lines (lane P4) */}
                      <span className="min-w-0">
                        {formatBlockNumber(block.number)}′ <FullHash hash={block.hash} />
                      </span>
                    </span>
                  ))}
                  <span className="text-text-dim">included by</span>
                  <span className="min-w-0">
                    {includer && includer.minerLockHash !== null ? (
                      // the whole address in designed lines, as the hashes (lane P4); "not
                      // recorded" breaks between words
                      <Link href={`/address/${minerAddressOf(includer)}`} look="inline">
                        <HeadId value={minerAddressOf(includer)} kind="address" />
                      </Link>
                    ) : (
                      // U-15: the miner's witness is not in the store; said as a fact, not a state
                      <span className="text-text-dim">not recorded</span>
                    )}
                  </span>
                </div>
              </DetailSection>
            );
          })}
        </div>
      </DetailTier2>
    </div>
  );
}
