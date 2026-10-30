'use client';

import type { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { notFound, useParams } from '@/src/navigation';
import {
  api,
  isNotFoundApiError,
  type CanonicalBranchBlock,
  type ForkEventRef,
  type OrphanedBlock,
  type OrphanedTransaction,
  type ReorgDetail,
} from '@/lib/api';
import { Header } from '@/components/layout/header';
import { NotFoundPage } from '@/components/not-found-page';
import { ExactCkb } from '@/components/activity-event-row';
import { DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import { ListTable } from '@/components/ui/list-table';
import { ForkTree, TIP_MARKS } from '@/components/forks/fork-tree';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import {
  blockClock,
  forkRefHref,
  missingAddressWords,
  orphanFateWords,
  parseForkRoute,
} from '@/lib/fork-format';
import { MinerLink } from '@/components/forks/miner-link';
import { forkTreeModel } from '@/lib/fork-tree';
import { formatUtc8Date, formatUtc8DateTime, formatUtc8Time } from '@/lib/utils';

type MinerBlock = { number: number; minerLockHash: string | null; minerAddress: string | null };

/** A block's miner address; the API gives one exactly when it knows the miner's lock (R2b-b-12). */
function minerAddressOf(block: MinerBlock): string {
  if (block.minerLockHash === null || block.minerAddress === null)
    throw new Error(
      `Fork block ${formatBlockNumber(block.number)}: miner lock ${block.minerLockHash} without an address ${block.minerAddress}`
    );
  return block.minerAddress;
}

function Miner({ block }: { block: MinerBlock }) {
  if (block.minerLockHash === null && block.minerAddress === null)
    return <span className="text-text-dim">witness unavailable</span>;
  return <MinerLink address={minerAddressOf(block)} />;
}
const hashShort = (hash: string) => `${hash.slice(0, 10)}…${hash.slice(-6)}`;
const blockAt = (number: number) => (
  <Link href={`/blocks/${number}`} className="hover:text-jade">
    {formatBlockNumber(number)}
  </Link>
);

export default function ForkDetailPage() {
  const params = useParams();
  const route = parseForkRoute(String(params.height ?? ''), String(params.orphan ?? ''));
  const query = useQuery({
    queryKey: ['fork', route?.height, route?.orphan],
    queryFn: () => api.getForkAt(route!.height, route!.orphan),
    enabled: route !== null,
  });
  if (!route) {
    notFound();
    return null;
  }
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
  let mismatch: string | null = null;
  if (query.data) {
    const { event, orphanedBlocks } = query.data;
    const first = orphanedBlocks[0];
    if (!first)
      mismatch = `API answered ${address} with a ${event.eventType === 'deep' ? 'deep detection' : 'switch'} at ${formatBlockNumber(event.forkPointNumber)} with no orphaned block`;
    else if (event.forkPointNumber !== route.height || first.hash.slice(2, 10) !== route.orphan)
      mismatch = `API answered ${address} with the fork at ${formatBlockNumber(event.forkPointNumber)} whose first orphan is ${first.hash.slice(0, 10)}`;
    else if (event.firstOrphanHash !== first.hash)
      mismatch = `API answered ${address} with first dropped block ${first.hash.slice(0, 10)}, but its event names first orphan ${event.firstOrphanHash?.slice(0, 14) ?? 'none'}`;
  }
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto min-w-0 max-w-[1280px] px-4 pb-[72px] pt-7">
        {query.isLoading && <p role="status">Loading fork evidence…</p>}
        {query.error && (
          <p role="alert" className="text-rouge text-[13px]">
            Fork evidence unavailable: {(query.error as Error).message}
          </p>
        )}
        {mismatch && (
          <p role="alert" className="text-rouge break-all text-[13px]">
            {mismatch}
          </p>
        )}
        {query.data && !mismatch && <ForkEvidence data={query.data} />}
      </main>
    </div>
  );
}

/** The story of one switch, sentence by sentence, from the API's facts only. */
function narrative(data: ReorgDetail): string[] {
  const { event, orphanedBlocks, orphanedTransactions } = data;
  const parts: string[] = [];
  orphanedBlocks
    .filter((block) => block.competingBlock && block.timestampGapMs !== null)
    .forEach((block, i) => {
      const gap = `${Math.abs(block.timestampGapMs!).toLocaleString('en-US')} ms apart`;
      const same =
        block.minerLockHash !== null && block.competingBlock!.minerLockHash === block.minerLockHash;
      const miner = same
        ? i === 0
          ? `, both by the same miner (…${minerAddressOf(block).slice(-6)})`
          : ', again both by it'
        : '';
      parts.push(
        i === 0
          ? `On ${formatUtc8Date(block.timestamp)} two blocks were found at ${formatBlockNumber(block.number)}, ${gap}${miner}.`
          : `At ${formatBlockNumber(block.number)} two more, ${gap}${miner}.`
      );
    });
  const back = orphanedBlocks.filter((block) => block.fate === 'canonical_now');
  for (const block of back)
    parts.push(
      `The block this node dropped at ${formatBlockNumber(block.number)} is back on the main chain.`
    );
  const lost = orphanedBlocks.length - back.length;
  if (lost > 0)
    parts.push(
      `This node had followed ${lost === 1 ? 'the block' : 'the blocks'} that lost and switched to the other branch.`
    );
  if (event.revertedByEvent) parts.push('A later switch on this node reversed it.');
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
    parts.push(
      `${formatBlockNumber(number)} included ${which} as ${blocks.length === 1 ? 'an uncle' : 'uncles'}.`
    );
  }
  for (const block of orphanedBlocks) {
    if (block.fate === 'not_included_as_uncle')
      parts.push(`${formatBlockNumber(block.number)}′ was not taken as an uncle.`);
    if (block.fate === 'uncle_fate_pending')
      parts.push(`${formatBlockNumber(block.number)}′ can still be taken as an uncle.`);
  }
  const droppedHashes = new Set(
    orphanedBlocks.filter((block) => block.fate !== 'canonical_now').map((block) => block.hash)
  );
  if (droppedHashes.size > 0) {
    const txs = orphanedTransactions.filter(
      (tx) => tx.txIndex > 0 && droppedHashes.has(tx.blockHash)
    );
    if (txs.length === 0)
      parts.push(
        droppedHashes.size === 1
          ? 'The dropped block held only its cellbase, so no transaction was undone.'
          : 'The dropped blocks held only their cellbases, so no transaction was undone.'
      );
    else {
      const committed = txs.filter(
        (tx) => tx.fate === 'committed_later' || tx.fate === 'committed_on_canonical_branch'
      ).length;
      const waiting = txs.filter((tx) => tx.fate === 'not_committed_as_of_tip').length;
      parts.push(
        `${committed} of its ${formatCount(txs.length, 'transaction')} ${committed === 1 ? 'was' : 'were'} committed later.` +
          (waiting
            ? ` ${waiting} ${waiting === 1 ? 'is' : 'are'} not committed as of the tip.`
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
  transactionsCount: number;
  dropped: boolean;
  fate: ReactNode;
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
        transactionsCount: block.transactionsCount,
        dropped: true,
        fate: block.uncleIn ? (
          <Link href={`/blocks/${block.uncleIn.blockNumber}`} className="text-jade hover:underline">
            {`uncle in ${formatBlockNumber(block.uncleIn.blockNumber)}`}
          </Link>
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
    <Link href={href} className="hover:text-jade underline decoration-dotted">
      {text}
    </Link>
  ) : (
    <span>
      {text}{' '}
      <span className="text-text-dim">({missingAddressWords(target.firstOrphanStatus)})</span>
    </span>
  );
}

function ForkEvidence({ data }: { data: ReorgDetail }) {
  const { event, orphanedBlocks, orphanedTransactions } = data;
  const model = forkTreeModel(data);
  const where = (hash: string) =>
    model.main.some((tile) => tile.hash === hash)
      ? 'on what is the main chain today'
      : 'on a branch that lost';
  const rows = blockRows(data);
  const inclusions = [
    ...new Set(
      orphanedBlocks.flatMap((block) => (block.uncleIn ? [block.uncleIn.blockNumber] : []))
    ),
  ];
  const nonCellbase = orphanedTransactions.filter((tx) => tx.txIndex > 0);
  return (
    <div className="min-w-0">
      <nav aria-label="Breadcrumb" className="text-text-dim text-[12px]">
        <Link href="/forks" className="hover:text-text">
          Forks
        </Link>
      </nav>
      <div className="mt-2.5 flex flex-wrap items-baseline gap-3">
        <h1 className="text-text-bright text-[26px] font-medium">
          Fork at {formatBlockNumber(event.forkPointNumber)}
        </h1>
        <span className="text-text-dim text-[13px]">
          {formatCount(event.depth, 'block')} deep · detected {formatUtc8DateTime(event.detectedAt)}{' '}
          · this node switched once
        </span>
      </div>
      <p
        data-testid="fork-narrative"
        className="text-text mt-3 max-w-[900px] text-[15px] leading-relaxed"
      >
        {narrative(data).join(' ')}
      </p>

      <div className="mt-11 flex flex-col gap-11">
        <DetailSection
          title="The two branches"
          actions={
            <span className="text-text-dim flex flex-wrap gap-x-4 gap-y-1 text-[11.5px]">
              <span>dashed: blocks that lost</span>
              <span className="text-jade">→ included as an uncle</span>
              <span className="text-text-bright">① ② this node’s tip, in turn</span>
            </span>
          }
        >
          <div className="min-w-0 overflow-x-auto pt-6">
            <ForkTree model={model} />
          </div>
          <p className="text-text-dim mt-1 text-[11px] md:hidden">
            scroll sideways for both branches
          </p>
          <div className="mt-2 grid grid-cols-[34px_minmax(0,1fr)] gap-x-2.5 gap-y-1.5 text-[12.5px] sm:grid-cols-[34px_190px_minmax(0,1fr)]">
            <span className="text-text-bright">{TIP_MARKS[1]}</span>
            <span className="text-text-dim">before {formatUtc8Time(event.detectedAt)}</span>
            <span className="text-text col-span-2 sm:col-span-1">
              this node’s tip is {formatBlockNumber(event.oldTipNumber)}{' '}
              <span className="text-text-dim">{hashShort(event.oldTipHash)}</span>{' '}
              {where(event.oldTipHash)}
            </span>
            <span className="text-text-bright">{TIP_MARKS[2]}</span>
            <span className="text-text-dim">{formatUtc8Time(event.detectedAt)}</span>
            <span className="text-text col-span-2 sm:col-span-1">
              the node switches to {formatBlockNumber(event.newTipNumber)}{' '}
              <span className="text-text-dim">{hashShort(event.newTipHash)}</span> and drops{' '}
              {orphanedBlocks.map((block) => `${formatBlockNumber(block.number)}′`).join(' and ')}
              <span className="text-text-dim"> · depth {event.depth}</span>
            </span>
          </div>
        </DetailSection>

        <DetailSection title="Blocks">
          <ListTable
            label="Blocks"
            rows={rows}
            rowKey={(row) => row.key}
            columns={[
              {
                key: 'height',
                label: 'Height',
                minWidth: 120,
                render: (row) =>
                  row.dropped ? `${formatBlockNumber(row.number)}′` : blockAt(row.number),
              },
              {
                key: 'hash',
                label: 'Hash',
                minWidth: 170,
                render: (row) => (
                  <span className="text-[#6aa9c4]" title={row.hash}>
                    {hashShort(row.hash)}
                  </span>
                ),
              },
              {
                key: 'time',
                label: 'Time (UTC+8)',
                minWidth: 120,
                render: (row) => <span className="text-text-dim">{blockClock(row.timestamp)}</span>,
              },
              {
                key: 'miner',
                label: 'Miner',
                minWidth: 150,
                render: (row) => <Miner block={row} />,
              },
              {
                key: 'txs',
                label: 'Txs',
                minWidth: 60,
                numeric: true,
                render: (row) => row.transactionsCount.toLocaleString('en-US'),
              },
              { key: 'fate', label: 'Fate', minWidth: 200, render: (row) => row.fate },
            ]}
          />
          <p className="text-text-dim mt-2 text-[12px]">
            An uncle earns its miner nothing; it counts toward the uncle rate that sets the
            difficulty.
          </p>
        </DetailSection>

        {nonCellbase.length > 0 && (
          <DetailSection title="Transactions from the dropped branch">
            <ListTable
              label="Dropped branch transactions"
              rows={nonCellbase}
              rowKey={(tx: OrphanedTransaction) => `${tx.blockHash}-${tx.txIndex}`}
              columns={[
                {
                  key: 'tx',
                  label: 'Transaction',
                  minWidth: 300,
                  render: (tx: OrphanedTransaction) =>
                    tx.committedAt ? (
                      <Link
                        href={`/tx/${tx.hash}`}
                        className="text-emphasis break-all hover:underline"
                      >
                        {tx.hash}
                      </Link>
                    ) : (
                      <span className="break-all">{tx.hash}</span>
                    ),
                },
                {
                  key: 'origin',
                  label: 'Dropped block',
                  minWidth: 110,
                  render: (tx: OrphanedTransaction) => `${formatBlockNumber(tx.blockNumber)}′`,
                },
                {
                  key: 'fate',
                  label: 'Fate now',
                  minWidth: 170,
                  render: (tx: OrphanedTransaction) => tx.fate.replaceAll('_', ' '),
                },
                {
                  key: 'commit',
                  label: 'Committed in',
                  minWidth: 130,
                  render: (tx: OrphanedTransaction) =>
                    tx.committedAt ? blockAt(tx.committedAt.blockNumber) : '—',
                },
                {
                  key: 'capacity',
                  label: 'Output capacity',
                  minWidth: 170,
                  numeric: true,
                  render: (tx: OrphanedTransaction) =>
                    tx.totalCapacity == null ? (
                      'Unavailable'
                    ) : (
                      <ExactCkb shannons={BigInt(tx.totalCapacity)} signed={false} tone="plain" />
                    ),
                },
              ]}
            />
          </DetailSection>
        )}
      </div>

      <DetailTier2>
        <div className="grid min-w-0 items-start gap-x-14 gap-y-10 lg:grid-cols-2">
          <DetailSection title="This node’s fork event, as stored">
            <div className="grid grid-cols-1 gap-x-3 gap-y-1.5 text-[12px] sm:grid-cols-[110px_minmax(0,1fr)]">
              <span className="text-text-dim">detected</span>
              <span>{formatUtc8DateTime(event.detectedAt)}</span>
              <span className="text-text-dim">depth</span>
              <span>{formatCount(event.depth, 'block')}</span>
              <span className="text-text-dim">old tip</span>
              <span className="break-all">
                {formatBlockNumber(event.oldTipNumber)} {event.oldTipHash}
              </span>
              <span className="text-text-dim">new tip</span>
              <span className="break-all">
                {formatBlockNumber(event.newTipNumber)} {event.newTipHash}
              </span>
              {event.supersededByTipHash && (
                <>
                  <span className="text-text-dim">superseded</span>
                  <span className="break-all">
                    before replay; the canonical {formatBlockNumber(event.newTipNumber)} was{' '}
                    {event.supersededByTipHash}
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
          {inclusions.map((number) => {
            const uncles = orphanedBlocks.filter((block) => block.uncleIn?.blockNumber === number);
            const includer = data.canonicalBranch.find((block) => block.number === number);
            return (
              <DetailSection key={number} title={`Uncles in ${formatBlockNumber(number)}`}>
                <div className="grid grid-cols-1 gap-x-3 gap-y-1.5 text-[12px] sm:grid-cols-[110px_minmax(0,1fr)]">
                  {uncles.map((block, i) => (
                    <span key={block.hash} className="contents">
                      <span className="text-text-dim">uncle {i}</span>
                      <span className="break-all">{`${block.hash} · ${formatBlockNumber(block.number)}`}</span>
                    </span>
                  ))}
                  <span className="text-text-dim">included by</span>
                  <span className="break-all">
                    {includer && includer.minerLockHash !== null ? (
                      <Link
                        href={`/address/${minerAddressOf(includer)}`}
                        className="hover:text-jade"
                      >
                        {minerAddressOf(includer)}
                      </Link>
                    ) : (
                      <span className="text-text-dim">miner witness unavailable</span>
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
