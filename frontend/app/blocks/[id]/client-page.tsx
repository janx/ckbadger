'use client';

import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { useParams, useRouter } from '@/src/navigation';
import { Header } from '@/components/layout/header';
import { Hash } from '@/components/ui/hash';
import { Address } from '@/components/ui/address';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { ActivityRow } from '@/components/activity/activity-row';
import { ExactCkb } from '@/components/activity-event-row';
import { TransactionTable } from '@/components/transaction/transaction-table';
import { ResourceBar, resourceScale } from '@/components/blocks/block-visual';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, isNotFoundApiError } from '@/lib/api';
import { isEditableElement } from '@/lib/search-focus';
import { parseShannons, formatPercent } from '@/lib/ckb-decimal';
import { formatTimeAgo } from '@/lib/utils';

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
  if (error)
    return (
      <div>
        <Header />
        <main className="container mx-auto px-4 py-8">
          <p role="alert">
            {isNotFoundApiError(error)
              ? 'Block not found'
              : `Unable to read block: ${error.message}`}
          </p>
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
  const sizeScale = resourceScale([block.size]);
  const parentElapsedMs =
    block.parentTimestamp === null
      ? null
      : new Date(block.timestamp).getTime() - new Date(block.parentTimestamp).getTime();
  if (parentElapsedMs !== null && !Number.isSafeInteger(parentElapsedMs))
    throw new Error(`Block ${block.number}: invalid parent timestamp interval`);
  const cycles = block.cycles === null ? null : Number(block.cycles);
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto min-w-0 px-4 py-8">
        <DetailLayout
          title={`Block #${block.number.toLocaleString('en-US')}`}
          identity={<Hash hash={block.hash} truncate={false} />}
          status={
            activation && (
              <Link href="/hardforks" className="text-text-dim text-xs">
                HARDFORK ACTIVATION · {activation.shortName.toUpperCase()}
              </Link>
            )
          }
          actions={
            <nav aria-label="Adjacent blocks" className="flex gap-5 text-xs">
              {block.number > 0 && (
                <Link href={`/blocks/${block.number - 1}`}>← Previous Block</Link>
              )}
              <Link href={`/blocks/${block.number + 1}`}>Next Block →</Link>
            </nav>
          }
          tier2={
            <>
              <DetailSection title="Block evidence">
                <dl className="grid gap-x-10 gap-y-5 text-xs sm:grid-cols-2 lg:grid-cols-3">
                  <div>
                    <dt className="text-text-dim mb-2">Epoch</dt>
                    <dd>
                      <Link href={`/blocks/${epochStart}`}>
                        {block.epochNumber.toLocaleString('en-US')}
                      </Link>{' '}
                      · {block.epochIndex.toLocaleString('en-US')} /{' '}
                      {block.epochLength.toLocaleString('en-US')}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-text-dim mb-2">Difficulty</dt>
                    <dd className="break-all">
                      {BigInt(block.difficulty).toLocaleString('en-US')}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-text-dim mb-2">Size, bytes</dt>
                    <dd>
                      <ResourceBar
                        label="Size, bytes"
                        value={block.size}
                        overhead={block.overheadSize}
                        scale={sizeScale}
                      />
                    </dd>
                    <dd className="text-text-dim mt-2">
                      {formatPercent(BigInt(block.size), BigInt(block.maxBlockBytes))}% of{' '}
                      {block.maxBlockBytes.toLocaleString('en-US')} ·{' '}
                      {block.overheadSize.toLocaleString('en-US')} overhead ·{' '}
                      {block.userTransactionsSize.toLocaleString('en-US')} user transactions
                    </dd>
                  </div>
                  <div>
                    <dt className="text-text-dim mb-2">Cycles</dt>
                    <dd>
                      {cycles === null ? (
                        block.cyclesStatus === 'failed' ? (
                          'Replay failed'
                        ) : (
                          'Not yet run'
                        )
                      ) : (
                        <ResourceBar
                          label="Cycles"
                          value={cycles}
                          scale={resourceScale([cycles])}
                        />
                      )}
                    </dd>
                    {block.cycles !== null && (
                      <dd className="text-text-dim mt-2">
                        {formatPercent(BigInt(block.cycles), BigInt(block.maxBlockCycles))}% of{' '}
                        {BigInt(block.maxBlockCycles).toLocaleString('en-US')}
                      </dd>
                    )}
                  </div>
                  <div className="min-w-0 sm:col-span-2">
                    <dt className="text-text-dim mb-2">Cellbase message</dt>
                    <dd className="break-all">
                      {block.minerMessage === null
                        ? 'Genesis'
                        : block.minerMessageText === null
                          ? 'Non-UTF-8 message; see raw bytes below'
                          : block.minerMessageText}
                    </dd>
                  </div>
                </dl>
              </DetailSection>
              <DetailSection title="Position in the chain">
                <nav
                  aria-label="Chain position"
                  className="flex flex-wrap items-center gap-4 text-xs"
                >
                  {block.number > 0 && (
                    <Link href={`/blocks/${block.parentHash}`}>
                      #{(block.number - 1).toLocaleString('en-US')}
                    </Link>
                  )}
                  <span>→</span>
                  <span className="text-jade">#{block.number.toLocaleString('en-US')}</span>
                  {block.confirmations > 1 && (
                    <>
                      <span>→</span>
                      <Link href={`/blocks/${block.number + 1}`}>
                        #{(block.number + 1).toLocaleString('en-US')}
                      </Link>
                    </>
                  )}
                </nav>
              </DetailSection>
              <DetailSection
                title="Reward breakdown"
                actions={
                  block.rewardPaidAtBlock !== null && (
                    <Link
                      href={`/blocks/${block.rewardPaidAtBlock}`}
                      className="text-text-dim text-xs"
                    >
                      Paid in #{block.rewardPaidAtBlock.toLocaleString('en-US')}
                    </Link>
                  )
                }
              >
                {block.rewardBreakdown ? (
                  <dl className="space-y-3 text-xs">
                    {(
                      [
                        ['primary', 'Primary issuance', 'The primary issuance paid to this miner'],
                        [
                          'secondary',
                          'Secondary issuance',
                          'The miner’s share of secondary issuance',
                        ],
                        ['committed', 'Committed fees', 'Fees earned for committing transactions'],
                        ['proposal', 'Proposal fees', 'Fees earned for proposing transactions'],
                      ] as const
                    ).map(([key, label, note]) => (
                      <div
                        key={key}
                        className="grid grid-cols-1 gap-1 sm:grid-cols-[160px_200px_1fr]"
                      >
                        <dt>{label}</dt>
                        <dd className="sm:text-right">
                          {amount(block.rewardBreakdown![key], key)}
                        </dd>
                        <dd className="text-text-dim sm:pl-4">{note}</dd>
                      </div>
                    ))}
                  </dl>
                ) : (
                  <p className="text-text-dim text-xs">
                    {block.number === 0
                      ? 'Genesis has no mining reward.'
                      : 'Reward is not finalized and indexed yet.'}
                  </p>
                )}
              </DetailSection>
              <div className="grid gap-8 md:grid-cols-2">
                <DetailSection title={`Uncles (${block.unclesCount})`}>
                  {block.uncles.map((uncle) => (
                    <article
                      key={uncle.hash}
                      className="border-base-border-subtle space-y-2 border-b py-3 text-xs"
                    >
                      <Link href={`/blocks/${uncle.hash}`}>
                        #{uncle.number.toLocaleString('en-US')} ·{' '}
                        <Hash hash={uncle.hash} copyable={false} />
                      </Link>
                      <p className="text-text-dim">
                        {new Date(uncle.timestamp).toUTCString()} · {uncle.proposalIds.length}{' '}
                        proposals
                      </p>
                      <Link href={`/blocks/${uncle.parentHash}`} className="text-text-dim">
                        Parent <Hash hash={uncle.parentHash} copyable={false} />
                      </Link>
                    </article>
                  ))}
                  {block.unclesCount === 0 && (
                    <p className="text-text-dim text-xs">No uncles in this block.</p>
                  )}
                </DetailSection>
                <DetailSection title={`Proposals (${block.proposalsCount})`}>
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
                          <span className="text-dao">●</span>
                          {proposal.proposalId}
                          {proposal.uncle && (
                            <span className="text-text-dim">
                              uncle #{proposal.uncle.number.toLocaleString('en-US')}
                            </span>
                          )}
                        </p>
                        {proposal.committedTxHash ? (
                          <p>
                            <Link href={`/tx/${proposal.committedTxHash}`}>
                              <Hash hash={proposal.committedTxHash} copyable={false} />
                            </Link>{' '}
                            in{' '}
                            <Link href={`/blocks/${proposal.committedBlockNumber}`}>
                              #{proposal.committedBlockNumber?.toLocaleString('en-US')}
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
              <div className="text-text-dim">Header version {block.version} · 208 bytes</div>
              {Object.entries(rawHeader).map(([key, value]) => (
                <div key={key} className="grid gap-1 sm:grid-cols-[160px_minmax(0,1fr)]">
                  <dt className="text-text-dim">{key}</dt>
                  <dd className="break-all">
                    {key === 'parentHash' ? <Link href={`/blocks/${value}`}>{value}</Link> : value}
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
              <p className="mb-8 text-[13px] leading-relaxed">
                {block.transactionsCount.toLocaleString('en-US')} transactions
                {block.minerAddress && (
                  <>
                    {' '}
                    mined by <Address address={block.minerAddress} />
                  </>
                )}{' '}
                on {new Date(block.timestamp).toUTCString()}.
                {block.miningReward !== null && (
                  <> The miner earns {amount(block.miningReward, 'reward')} for this block.</>
                )}
              </p>
              <div
                role="group"
                aria-label="Block contents"
                className="border-base-border-subtle mb-4 flex flex-wrap gap-6 border-b"
              >
                {(['activities', 'transactions'] as const).map((tab) => (
                  <button
                    type="button"
                    key={tab}
                    aria-pressed={view === tab}
                    onClick={() => setView(tab)}
                    className={`min-h-11 border-b-2 text-[13px] ${tab === view ? 'border-jade text-text-bright' : 'text-text-dim border-transparent'}`}
                  >
                    {tab === 'activities'
                      ? 'Activities'
                      : `Transactions (${block.transactionsCount})`}
                  </button>
                ))}
              </div>
              {view === 'activities' ? (
                <>
                  {activities.isLoading && <p role="status">Loading activities…</p>}
                  {activities.error && (
                    <p role="alert">Unable to read activities: {activities.error.message}</p>
                  )}
                  {activities.data && (
                    <>
                      {activities.data.data.map((activity) => (
                        <ActivityRow key={activity.txHash} activity={activity} />
                      ))}
                      {activities.data.data.length === 0 && (
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
                      <TransactionTable
                        transactions={transactions.data.data}
                        showBlock={false}
                        label="Transactions in this block"
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
                  <dd>{new Date(block.timestamp).toUTCString()}</dd>
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
                        Paid in #{block.rewardPaidAtBlock.toLocaleString('en-US')}
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
