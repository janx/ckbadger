'use client';

import { useEffect, useState, type ReactNode } from 'react';
import { useQueries, useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { useParams, useRouter } from '@/src/navigation';
import { Header } from '@/components/layout/header';
import { NotFoundPage } from '@/components/not-found-page';
import { Hash } from '@/components/ui/hash';
import { Address } from '@/components/ui/address';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { ActivityRow } from '@/components/activity/activity-row';
import { ExactCkb } from '@/components/activity-event-row';
import { ResourceBar, resourceScale } from '@/components/blocks/block-visual';
import { BlockPosition } from '@/components/blocks/block-position';
import { BlockTransactionsTable } from '@/components/blocks/block-transactions-table';
import { RewardBreakdown, RewardPaidBy } from '@/components/blocks/reward-breakdown';
import { decodeDaoHeader } from '@/lib/dao-header';
import { DaoHeaderFields } from '@/components/blocks/dao-header-fields';
import { getUncleDetailHref } from '@/lib/detail-routes';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, isNotFoundApiError, type Block, type BlockUncle, type Transaction } from '@/lib/api';
import { isEditableElement } from '@/lib/search-focus';
import { parseShannons, formatPercent } from '@/lib/ckb-decimal';
import { formatCompactInteger } from '@/lib/exact-integer';
import {
  formatTimeAgo,
  formatUtc8Date,
  formatUtc8DateTime,
  formatUtc8Time,
  truncateHash,
} from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import { formatEpochPosition } from '@/lib/epoch';

/** A proposal with a committing tx was committed in a block; a missing number is a contract bug. */
function committedBlock(block: number | null, txHash: string): number {
  if (block === null)
    throw new Error(`Proposal committed by ${txHash} has no committed block number`);
  return block;
}

export default function BlockDetailPage() {
  const { id } = useParams();
  const router = useRouter();
  const [view, setView] = useState<'activities' | 'transactions'>('activities');
  const pagination = useCursorPagination();
  const activityPagination = useCursorPagination();
  const {
    data: block,
    isLoading,
    error,
  } = useQuery({
    queryKey: ['block', id],
    queryFn: () => api.getBlock(id as string),
    refetchInterval: (query) => (query.state.data?.cyclesStatus === 'pending' ? 3000 : false),
  });
  const transactions = useQuery({
    queryKey: ['block-transactions', id, block?.number, pagination.cursor],
    queryFn: () =>
      api.getTransactions({
        blockNumber: block!.number,
        limit: 100,
        ...(pagination.cursor ? { cursor: pagination.cursor } : {}),
      }),
    enabled: !!block,
  });
  const activities = useQuery({
    queryKey: ['block-activities', block?.number, activityPagination.cursor],
    queryFn: () =>
      api.getGlobalActivities({
        blockNumber: block!.number,
        limit: 100,
        cursor: activityPagination.cursor,
      }),
    enabled: !!block,
  });
  const { data: proposals, error: proposalsError } = useQuery({
    queryKey: ['block-proposals', id],
    queryFn: () => api.getBlockProposals(id as string),
    enabled: !!block && (block.proposalsCount > 0 || block.unclesCount > 0),
  });
  // The blocks list around this block: its neighbours' tiles and the list's shared bar scale.
  const neighbours = useQuery({
    queryKey: ['block-neighbours', block?.number],
    queryFn: () => api.getBlocks({ cursor: String(block!.number + 3), limit: 50 }),
    enabled: !!block,
  });
  const uncleDetails = useQueries({
    queries: (block?.uncles ?? []).map((uncle, index) => ({
      queryKey: ['block-uncle', String(block!.number), index],
      queryFn: () => api.getBlockUncle(block!.number, index),
      enabled: !!block,
    })),
  });
  const { data: hardforkTimeline } = useQuery({
    queryKey: ['hardforks-for-block', block?.hardforkActivation?.id],
    queryFn: () => api.getHardforks(),
    enabled: !!block?.hardforkActivation && !block.hardforkActivation.resources?.length,
  });
  useEffect(() => {
    if (!block) return;
    const navigate = (event: KeyboardEvent) => {
      if (
        isEditableElement(event.target) ||
        event.metaKey ||
        event.ctrlKey ||
        event.altKey ||
        event.shiftKey
      )
        return;
      const key = event.key.toLowerCase();
      if ((key === 'arrowleft' || key === 'h') && block.number > 0) {
        event.preventDefault();
        router.push(`/blocks/${block.number - 1}`);
      }
      if (key === 'arrowright' || key === 'l') {
        event.preventDefault();
        router.push(`/blocks/${block.number + 1}`);
      }
    };
    window.addEventListener('keydown', navigate);
    return () => window.removeEventListener('keydown', navigate);
  }, [block, router]);

  if (isLoading)
    return (
      <div>
        <Header />
        <main className="container mx-auto px-4 py-8">
          <p role="status">Loading block…</p>
        </main>
      </div>
    );
  if (error && !block && isNotFoundApiError(error)) {
    return (
      <NotFoundPage
        entity={{
          kind: 'block',
          identifier: String(id),
          checkedEndpoint: `/blocks/${encodeURIComponent(String(id))}`,
        }}
      />
    );
  }
  if (error && !block)
    return (
      <div>
        <Header />
        <main className="container mx-auto px-4 py-8">
          <p role="alert">Unable to read block: {error.message}</p>
        </main>
      </div>
    );
  if (!block) throw new Error(`Block ${id}: request completed without block evidence`);
  const activation = block.hardforkActivation;
  const resources = activation?.resources?.length
    ? activation.resources
    : hardforkTimeline?.events.find((event) => event.id === activation?.id)?.resources;
  const epochStart = block.number - block.epochIndex;
  const amount = (value: string, field: string) => (
    <ExactCkb
      shannons={parseShannons(value, `block ${block.number} ${field}`)}
      signed={false}
      tone="plain"
    />
  );
  const rawHeader = {
    parentHash: block.parentHash,
    compactTarget: block.compactTarget,
    nonce: block.nonce,
    epoch: block.rawHeader.epoch,
    transactionsRoot: block.transactionsRoot,
    proposalsHash: block.rawHeader.proposalsHash,
    extraHash: block.rawHeader.extraHash,
    dao: block.rawHeader.dao,
  };
  const dao = decodeDaoHeader(block.rawHeader.dao, `Block ${block.number}`);
  // Proposals are a second read: an uncle this block read does not list (a reorg between the
  // two) is shown as text, not linked by a guessed position (#6).
  const uncleIndex = (hash: string) => block.uncles.findIndex((uncle) => uncle.hash === hash);
  const listed = neighbours.data?.data;
  // The bars reuse the blocks list's scale: the round number above the 50 blocks up to #N+2.
  const sizeScale = listed ? resourceScale([block.size, ...listed.map((b) => b.size)]) : null;
  const cyclesScale = listed
    ? resourceScale(
        [block, ...listed].flatMap((b) => (b.cycles === null ? [] : [Number(b.cycles)]))
      )
    : null;
  const scaleNote = listed?.length
    ? `on the blocks list's scale for ${formatBlockNumber(listed[0].number)} to ${formatBlockNumber(listed[listed.length - 1].number)}`
    : undefined;
  // Block and proposals are two reads (a reorg may fall between them, #6): the summary counts
  // the proposals read itself.
  const ownProposals = proposals?.filter((p) => p.uncle === null);
  const activityRows = activities.data
    ? !activities.data.hasMore && !activityPagination.hasPrevious
      ? [...activities.data.data].sort((a, b) => a.txIndex - b.txIndex)
      : activities.data.data
    : undefined;
  const parentElapsedMs =
    block.parentTimestamp === null
      ? null
      : new Date(block.timestamp).getTime() - new Date(block.parentTimestamp).getTime();
  if (parentElapsedMs !== null && !Number.isSafeInteger(parentElapsedMs))
    throw new Error(`Block ${block.number}: invalid parent timestamp interval`);
  const cycles = block.cycles === null ? null : Number(block.cycles);
  const paidIn = block.rewardFinalizesAt === null ? null : block.rewardFinalizesAt - block.number;
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto min-w-0 px-4 py-8">
        {error && (
          <p role="alert">
            Unable to refresh block: {error.message}. Showing previously loaded data.
          </p>
        )}
        <DetailLayout
          title={
            <span className="flex items-center gap-2.5">
              {block.number > 0 && (
                <ArrowLink
                  href={`/blocks/${block.number - 1}`}
                  label={`Previous block, ${(block.number - 1).toLocaleString('en-US')}`}
                  d="M7.5 2 3.5 6l4 4"
                />
              )}
              {formatBlockNumber(block.number)}
              <ArrowLink
                href={`/blocks/${block.number + 1}`}
                label={`Next block, ${(block.number + 1).toLocaleString('en-US')}`}
                d="M4.5 2 8.5 6l-4 4"
              />
            </span>
          }
          identity={
            <span className="flex min-w-0 items-baseline gap-2.5">
              <span>Block</span>
              <Hash hash={block.hash} truncate={false} />
            </span>
          }
          status={
            activation && (
              <Link href="/hardforks" className="text-text-dim text-xs">
                HARDFORK ACTIVATION · {activation.shortName.toUpperCase()}
              </Link>
            )
          }
          rawOpen
          tier2={
            <>
              <dl
                data-testid="block-tier2-facts"
                className="grid gap-x-10 gap-y-5 text-xs [grid-template-columns:repeat(auto-fit,minmax(min(100%,260px),1fr))]"
              >
                <div>
                  <dt className="text-text-dim mb-1">Epoch</dt>
                  <dd className="flex items-center gap-2">
                    <Link href={`/blocks/${epochStart}`} className="text-text-bright">
                      {block.epochNumber.toLocaleString('en-US')}
                    </Link>
                    <EpochBar index={block.epochIndex} length={block.epochLength}>
                      Epoch {block.epochNumber.toLocaleString('en-US')}:{' '}
                      {block.epochIndex.toLocaleString('en-US')} of{' '}
                      {block.epochLength.toLocaleString('en-US')}
                    </EpochBar>
                    <span>
                      {block.epochIndex.toLocaleString('en-US')} /{' '}
                      {block.epochLength.toLocaleString('en-US')}
                    </span>
                  </dd>
                </div>
                <div>
                  <dt className="text-text-dim mb-1">Difficulty</dt>
                  <dd className="text-text-bright">
                    {formatCompactInteger(
                      block.difficulty,
                      `block ${block.number} difficulty`
                    ).replace(/([A-Z])$/, ' $1')}
                  </dd>
                  <dd className="text-text-dim break-all">
                    {BigInt(block.difficulty).toLocaleString('en-US')}
                  </dd>
                </div>
                <div>
                  <dt className="text-text-dim mb-1">Size, bytes</dt>
                  <dd title={scaleNote}>
                    {sizeScale === null ? (
                      <ScalePending error={neighbours.error} value={block.size} />
                    ) : (
                      <ResourceBar
                        label="Size, bytes"
                        value={block.size}
                        overhead={block.overheadSize}
                        scale={sizeScale}
                      />
                    )}
                  </dd>
                  <dd className="text-text-dim mt-1">
                    {formatPercent(BigInt(block.size), BigInt(block.maxBlockBytes))}% of{' '}
                    {block.maxBlockBytes.toLocaleString('en-US')} · overhead{' '}
                    {block.overheadSize.toLocaleString('en-US')} · transactions{' '}
                    {block.userTransactionsSize.toLocaleString('en-US')}
                  </dd>
                </div>
                <div>
                  <dt className="text-text-dim mb-1">Cycles</dt>
                  <dd title={scaleNote}>
                    {cycles === null ? (
                      block.cyclesStatus === 'failed' ? (
                        'Replay failed'
                      ) : (
                        'Not yet run'
                      )
                    ) : cyclesScale === null ? (
                      <ScalePending error={neighbours.error} value={cycles} />
                    ) : (
                      <ResourceBar label="Cycles" value={cycles} scale={cyclesScale} />
                    )}
                  </dd>
                  {block.cycles !== null && (
                    <dd className="text-text-dim mt-1">
                      {formatPercent(BigInt(block.cycles), BigInt(block.maxBlockCycles))}% of{' '}
                      {BigInt(block.maxBlockCycles).toLocaleString('en-US')}
                    </dd>
                  )}
                </div>
                <div className="min-w-0">
                  <dt className="text-text-dim mb-1">Miner client</dt>
                  <dd className="text-text-bright break-all">
                    {block.minerMessage === null
                      ? 'Genesis'
                      : block.minerMessageText === null
                        ? 'Non-UTF-8 message; see raw bytes below'
                        : block.minerMessageText}
                  </dd>
                  <dd className="text-text-dim">decoded from the cellbase message</dd>
                </div>
              </dl>
              <DetailSection title="Position in the chain">
                <BlockPosition
                  block={block}
                  transactions={transactions.data?.data}
                  proposals={proposals}
                  neighbours={listed}
                  page={pagination.page}
                  hasMore={transactions.data?.hasMore ?? false}
                  transactionsError={transactions.error?.message}
                  proposalsError={proposalsError?.message}
                />
              </DetailSection>
              <DetailSection title="Reward breakdown" actions={<RewardPaidBy block={block} />}>
                <RewardBreakdown block={block} />
              </DetailSection>
              <div className="grid gap-8 md:grid-cols-2">
                <DetailSection
                  title="Uncles"
                  actions={
                    <span className="text-text-dim text-xs">
                      {block.unclesCount.toLocaleString('en-US')} · counted toward the NC-Max orphan
                      rate
                    </span>
                  }
                >
                  {block.uncles.map((uncle, index) => (
                    <UncleRow
                      key={uncle.hash}
                      blockNumber={block.number}
                      index={index}
                      uncle={uncle}
                      detail={uncleDetails[index]?.data}
                      error={uncleDetails[index]?.error ?? null}
                    />
                  ))}
                  {block.unclesCount === 0 && (
                    <p className="text-text-dim text-xs">No uncles in this block.</p>
                  )}
                </DetailSection>
                <DetailSection
                  title="Proposals"
                  actions={
                    ownProposals && (
                      <span className="text-text-dim text-xs">
                        {proposalsSummary(
                          ownProposals.length,
                          ownProposals.filter((p) => p.committedTxHash !== null).length
                        )}
                      </span>
                    )
                  }
                >
                  {proposalsError ? (
                    <p
                      role="alert"
                      data-testid="block-proposals-error"
                      className="text-text-dim text-xs"
                    >
                      Failed to load proposals: {proposalsError.message}
                    </p>
                  ) : (
                    proposals?.map((proposal) => (
                      <article
                        key={`${proposal.uncle?.hash}:${proposal.proposalIndex}`}
                        data-testid="block-proposal-row"
                        className="border-base-border-subtle space-y-2 border-b py-3 text-xs"
                      >
                        <p className="flex flex-wrap gap-2">
                          <span className="text-gold">●</span>
                          {proposal.proposalId}
                          {proposal.uncle &&
                            (uncleIndex(proposal.uncle.hash) >= 0 ? (
                              <Link
                                href={getUncleDetailHref(
                                  block.number,
                                  uncleIndex(proposal.uncle.hash)
                                )}
                                className="text-text-dim"
                              >
                                uncle {formatBlockNumber(proposal.uncle.number)}
                              </Link>
                            ) : (
                              <span className="text-text-dim">
                                uncle {formatBlockNumber(proposal.uncle.number)}
                              </span>
                            ))}
                        </p>
                        {proposal.committedTxHash ? (
                          <p>
                            <Link href={`/tx/${proposal.committedTxHash}`}>
                              <Hash hash={proposal.committedTxHash} copyable={false} />
                            </Link>{' '}
                            in{' '}
                            <Link href={`/blocks/${proposal.committedBlockNumber}`}>
                              {formatBlockNumber(
                                committedBlock(
                                  proposal.committedBlockNumber,
                                  proposal.committedTxHash
                                )
                              )}
                            </Link>
                          </p>
                        ) : (
                          <p className="text-text-dim">
                            No indexed commitment in its proposal window
                          </p>
                        )}
                      </article>
                    ))
                  )}
                  {!proposalsError &&
                    (proposals?.length === 0 ||
                      (block.proposalsCount === 0 && block.unclesCount === 0)) && (
                      <p className="text-text-dim text-xs">No proposals in this block</p>
                    )}
                  {!proposalsError &&
                    proposals === undefined &&
                    (block.proposalsCount > 0 || block.unclesCount > 0) && (
                      <p className="text-text-dim text-xs">Loading proposals...</p>
                    )}
                </DetailSection>
              </div>
            </>
          }
          rawData={
            <dl className="grid min-w-0 gap-3 text-xs">
              <div className="flex flex-wrap items-baseline justify-between gap-3">
                <span className="text-text-dim">Header · version {block.version} · 208 bytes</span>
                <span>
                  <Link href={`/blocks/${block.number}.raw`} target="_blank" rel="noreferrer">
                    JSON
                  </Link>
                  {' · '}
                  <Link href={`/blocks/${block.number}.md`} target="_blank" rel="noreferrer">
                    Markdown
                  </Link>
                </span>
              </div>
              {Object.entries(rawHeader).map(([key, value]) => (
                <div key={key} className="grid gap-1 sm:grid-cols-[160px_minmax(0,1fr)]">
                  <dt className="text-text-dim">{key}</dt>
                  <dd className="min-w-0">
                    <span className="break-all">
                      {key === 'parentHash' ? (
                        <Link href={`/blocks/${value}`}>{value}</Link>
                      ) : (
                        value
                      )}
                    </span>
                    {key === 'epoch' && (
                      <span className="text-text-dim">
                        {' '}
                        ={' '}
                        {formatEpochPosition({
                          number: block.epochNumber,
                          index: block.epochIndex,
                          length: block.epochLength,
                        })}
                      </span>
                    )}
                    {key === 'dao' && <DaoHeaderFields dao={dao} />}
                  </dd>
                </div>
              ))}
              <div className="grid gap-1 sm:grid-cols-[160px_minmax(0,1fr)]">
                <dt className="text-text-dim">Cellbase message bytes</dt>
                <dd className="break-all">
                  {block.minerMessage === null ? 'Genesis' : block.minerMessage}
                </dd>
              </div>
            </dl>
          }
        >
          {activation && resources && (
            <section aria-label="Hardfork resources">
              <h2 className="text-text-dim mb-3 text-[15px]">
                HARDFORK RESOURCES · {activation.shortName.toUpperCase()}
              </h2>
              <div className="flex flex-wrap gap-4">
                {resources.map((resource) => (
                  <a
                    key={resource.url}
                    href={resource.url}
                    target="_blank"
                    rel="noreferrer"
                    data-testid="block-hardfork-resource-link"
                    className="text-interactive text-xs"
                  >
                    {resource.label}
                  </a>
                ))}
              </div>
            </section>
          )}
          <div className="grid min-w-0 gap-10 lg:grid-cols-[minmax(0,1fr)_240px]">
            <div className="min-w-0">
              <p
                data-testid="block-summary"
                className="mb-8 max-w-[80ch] text-[15px] leading-relaxed"
              >
                {formatCount(block.transactionsCount, 'transaction')}
                {block.minerAddress && (
                  <>
                    , mined by <Address address={block.minerAddress} />
                  </>
                )}{' '}
                on {formatUtc8Date(block.timestamp)} at {formatUtc8Time(block.timestamp)}.
                {block.miningReward !== null ? (
                  <>
                    {' '}
                    The miner earns {amount(block.miningReward, 'reward')} for this block
                    {paidIn !== null && <>, paid {formatCount(paidIn, 'block')} later</>}.
                  </>
                ) : (
                  paidIn !== null && <> Its reward is paid {formatCount(paidIn, 'block')} later.</>
                )}
              </p>
              <div className="border-base-border-subtle mb-4 flex flex-wrap items-center justify-between gap-x-6 gap-y-2 border-b">
                <div role="group" aria-label="Block contents" className="flex flex-wrap gap-6">
                  {(['activities', 'transactions'] as const).map((tab) => (
                    <button
                      type="button"
                      key={tab}
                      aria-pressed={view === tab}
                      onClick={() => setView(tab)}
                      className={`-mb-px flex min-h-11 items-center gap-2 border-b-2 text-[15px] font-medium ${tab === view ? 'border-jade text-text-bright' : 'text-text-dim border-transparent'}`}
                    >
                      <span>{tab === 'activities' ? 'Activities' : 'Transactions'}</span>
                      {/* One activity row per transaction of the block (the block feed's contract). */}
                      <span className="text-text-dim text-[12px] font-normal">
                        {block.transactionsCount.toLocaleString('en-US')}
                      </span>
                    </button>
                  ))}
                </div>
                {transactions.data && !transactions.data.hasMore && !pagination.hasPrevious && (
                  <CommittedFrom transactions={transactions.data.data} block={block} />
                )}
              </div>
              {view === 'activities' ? (
                <>
                  {activities.isLoading && <p role="status">Loading activities…</p>}
                  {activities.error && (
                    <p role="alert">Unable to read activities: {activities.error.message}</p>
                  )}
                  {activities.data && activityRows && (
                    <>
                      {activityRows.map((activity) => (
                        <ActivityRow
                          key={activity.txHash}
                          activity={activity}
                          headline={activity.isCellbase ? 'Cellbase' : undefined}
                          evidence={
                            <>
                              #{activity.txIndex} ·{' '}
                              <Link
                                href={`/tx/${activity.txHash}`}
                                className="hover:text-jade text-interactive-dim"
                              >
                                {truncateHash(activity.txHash, 10, 6)}
                              </Link>
                            </>
                          }
                        />
                      ))}
                      {activityRows.length === 0 && (
                        <p className="text-text-dim text-xs">No activities in this block.</p>
                      )}
                      <CursorPagination
                        pageSize={100}
                        page={activityPagination.page}
                        hasMore={activities.data.hasMore}
                        hasPrevious={activityPagination.hasPrevious}
                        onNext={() => activityPagination.goToNext(activities.data.nextCursor)}
                        onPrevious={activityPagination.goToPrevious}
                      />
                    </>
                  )}
                </>
              ) : (
                <>
                  {transactions.isLoading && <p role="status">Loading transactions…</p>}
                  {transactions.error && (
                    <p role="alert">Unable to read transactions: {transactions.error.message}</p>
                  )}
                  {transactions.data && (
                    <>
                      <BlockTransactionsTable
                        transactions={transactions.data.data}
                        blockNumber={block.number}
                      />
                      <CursorPagination
                        total={block.transactionsCount}
                        pageSize={100}
                        page={pagination.page}
                        hasMore={transactions.data.hasMore}
                        hasPrevious={pagination.hasPrevious}
                        onNext={() => pagination.goToNext(transactions.data.nextCursor)}
                        onPrevious={pagination.goToPrevious}
                      />
                    </>
                  )}
                </>
              )}
            </div>
            <aside aria-label="Block facts" className="min-w-0 text-xs">
              <p className="text-jade mb-5">
                Canonical · {block.confirmations.toLocaleString('en-US')} confirmations
              </p>
              <dl className="space-y-5">
                <div>
                  <dt className="text-text-dim mb-1">Time</dt>
                  <dd>{formatUtc8DateTime(block.timestamp)}</dd>
                  <dd className="text-text-dim mt-1">
                    {formatTimeAgo(block.timestamp)}
                    {parentElapsedMs !== null && (
                      <> · {parentElapsedMs / 1000} s after its parent</>
                    )}
                  </dd>
                </div>
                <div>
                  <dt className="text-text-dim mb-1">Miner</dt>
                  <dd>
                    {block.minerAddress ? <Address address={block.minerAddress} /> : 'Genesis'}
                  </dd>
                </div>
                <div>
                  <dt className="text-text-dim mb-1">Miner reward</dt>
                  <dd>
                    {block.miningReward !== null
                      ? amount(block.miningReward, 'reward')
                      : 'Not finalized yet'}
                  </dd>
                  {block.rewardPaidAtBlock !== null && (
                    <dd className="text-text-dim mt-1">
                      <Link href={`/blocks/${block.rewardPaidAtBlock}`}>
                        Paid in {formatBlockNumber(block.rewardPaidAtBlock)}
                      </Link>
                    </dd>
                  )}
                  {block.miningRewardTxHash && (
                    <dd className="text-text-dim mt-1">
                      <Link href={`/tx/${block.miningRewardTxHash}`}>Paying cellbase →</Link>
                    </dd>
                  )}
                </div>
                <div>
                  <dt className="text-text-dim mb-1">Fees</dt>
                  <dd>{amount(block.totalFees, 'fees')}</dd>
                </div>
              </dl>
            </aside>
          </div>
        </DetailLayout>
      </main>
    </div>
  );
}

// ---- F2 blocks: the Block board's small pieces ----

function ArrowLink({ href, label, d }: { href: string; label: string; d: string }) {
  return (
    <Link
      href={href}
      aria-label={label}
      className="border-base-border text-text-dim hover:text-jade flex h-9 w-9 shrink-0 items-center justify-center rounded-md border"
    >
      <svg
        aria-hidden="true"
        width="12"
        height="12"
        viewBox="0 0 12 12"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
      >
        <path d={d} />
      </svg>
    </Link>
  );
}

/** How far into its epoch the block sits: a 72 px bar beside the exact index / length. */
function EpochBar({
  index,
  length,
  children,
}: {
  index: number;
  length: number;
  children: ReactNode;
}) {
  if (!Number.isSafeInteger(length) || length <= 0 || index < 0 || index >= length)
    throw new Error(`Epoch position ${index} / ${length} is outside its epoch`);
  return (
    <span
      role="img"
      aria-label={String(Array.isArray(children) ? children.join('') : children)}
      className="bg-base-border flex h-1 w-[72px] shrink-0 overflow-hidden rounded-sm"
    >
      <span className="bg-text h-full" style={{ width: `${((index + 1) / length) * 100}%` }} />
    </span>
  );
}

function ScalePending({ error, value }: { error: Error | null; value: number }) {
  return (
    <span className="text-text-bright">
      {value.toLocaleString('en-US')}
      <span className="text-text-dim ml-2 text-[11px]">
        {error ? `bar scale unavailable: ${error.message}` : 'reading the list scale…'}
      </span>
    </span>
  );
}

/** "2 · both committed": the block's own proposals and how many a later block committed. */
function proposalsSummary(total: number, committed: number): string {
  if (total === 0) return '0';
  if (committed === 0) return `${total.toLocaleString('en-US')} · none committed yet`;
  if (committed === total)
    return `${total.toLocaleString('en-US')} · ${total === 1 ? 'committed' : total === 2 ? 'both committed' : 'all committed'}`;
  return `${total.toLocaleString('en-US')} · ${committed.toLocaleString('en-US')} committed`;
}

/** "3 committed from #20,827,930's proposals, 2 blocks later", from each tx's proposal origin. */
function CommittedFrom({
  transactions,
  block,
}: {
  transactions: readonly Transaction[];
  block: Pick<Block, 'number'>;
}) {
  const committed = transactions.filter((tx) => !tx.isCellbase);
  if (!committed.length || block.number === 0) return null;
  const origins = new Map<number, number>();
  for (const tx of committed) {
    if (!tx.proposalOrigin)
      throw new Error(`Block ${block.number}: missing proposal origin for ${tx.hash}`);
    origins.set(
      tx.proposalOrigin.blockNumber,
      (origins.get(tx.proposalOrigin.blockNumber) ?? 0) + 1
    );
  }
  const [only] = origins.keys();
  return (
    <span className="text-text-dim text-xs">
      {origins.size === 1
        ? `${committed.length.toLocaleString('en-US')} committed from ${formatBlockNumber(only)}'s proposals, ${formatCount(block.number - only, 'block')} later`
        : `${committed.length.toLocaleString('en-US')} committed from the proposals of ${origins.size.toLocaleString('en-US')} blocks`}
    </span>
  );
}

/** One included uncle: its sibling on the canonical chain, the shared parent, the delay (R10). */
function UncleRow({
  blockNumber,
  index,
  uncle,
  detail,
  error,
}: {
  blockNumber: number;
  index: number;
  uncle: Block['uncles'][number];
  detail: BlockUncle | undefined;
  error: Error | null;
}) {
  if (detail && (detail.sameParentAs === null) !== (detail.msAfterSibling === null))
    throw new Error(
      `Uncle ${index} of block ${formatBlockNumber(blockNumber)}: sibling facts disagree`
    );
  return (
    <article className="border-base-border-subtle grid grid-cols-[minmax(0,1fr)_auto] gap-x-3 gap-y-0.5 border-b py-2.5 text-xs">
      <span>
        <Link href={getUncleDetailHref(blockNumber, index)} className="text-text-bright">
          {formatBlockNumber(uncle.number)}
        </Link>{' '}
        <span className="text-text-dim">uncle</span>
      </span>
      <Link href={getUncleDetailHref(blockNumber, index)}>
        <Hash hash={uncle.hash} copyable={false} />
      </Link>
      <span className="text-text-dim">
        {error ? (
          `sibling facts unavailable: ${error.message}`
        ) : !detail ? (
          'reading its sibling…'
        ) : detail.sameParentAs === null ? (
          <>the canonical {formatBlockNumber(detail.canonicalAtHeight.number)} has another parent</>
        ) : (
          <>
            sibling of {formatBlockNumber(detail.sameParentAs)}{' '}
            <Link href={`/blocks/${detail.canonicalAtHeight.hash}`}>
              <Hash hash={detail.canonicalAtHeight.hash} copyable={false} />
            </Link>
            , same parent{' '}
            <Link href={`/blocks/${uncle.parentHash}`}>{formatBlockNumber(uncle.number - 1)}</Link>
          </>
        )}
      </span>
      <span className="text-text-dim">
        {formatUtc8Time(uncle.timestamp)}
        {detail?.msAfterSibling != null &&
          `, ${detail.msAfterSibling >= 0 ? `${detail.msAfterSibling.toLocaleString('en-US')} ms after` : `${(-detail.msAfterSibling).toLocaleString('en-US')} ms before`} its sibling`}{' '}
        · {formatCount(uncle.proposalIds.length, 'proposal')}
      </span>
    </article>
  );
}
