'use client';

import { Fragment, useEffect, useState, type ReactNode } from 'react';
import { useQueries, useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import Link from '@/components/ui/link';
import { useParams, useRouter } from '@/src/navigation';
import { NotFoundPage } from '@/components/not-found-page';
import { Hash } from '@/components/ui/hash';
import { Address } from '@/components/ui/address';
import { MinerLink } from '@/components/forks/miner-link';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { PromptHeading } from '@/components/ui/prompt-heading';
import { DetailSkeleton } from '@/components/ui/detail-skeleton';
import { ActivityRow } from '@/components/activity/activity-row';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { ResourceBar, resourceScale } from '@/components/blocks/block-visual';
import { BlockPosition } from '@/components/blocks/block-position';
import { BlockTransactionsTable } from '@/components/blocks/block-transactions-table';
import { RewardBreakdown, RewardPaidBy } from '@/components/blocks/reward-breakdown';
import { decodeDaoHeader } from '@/lib/dao-header';
import { DaoHeaderFields } from '@/components/blocks/dao-header-fields';
import { getUncleDetailHref } from '@/lib/detail-routes';
import { Pager } from '@/components/ui/pager';
import { Tabs } from '@/components/ui/tabs';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { usePageSize } from '@/hooks/usePageSize';
import { rankedPageAt } from '@/lib/pagination';
import {
  api,
  isNotFoundApiError,
  type Block,
  type BlockUncle,
  type GlobalActivity,
  type Transaction,
} from '@/lib/api';
import { ParticipantRefView } from '@/components/participant-ref';
import { isBadRequestApiError } from '@/lib/api-errors';
import { ReadFailure } from '@/components/read-failure';
import { isEditableElement } from '@/lib/search-focus';
import { parseShannons } from '@/lib/ckb-decimal';
import { formatCompactInteger } from '@/lib/exact-integer';
import { formatUtc8Time } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import { checkHeaderEpoch, formatEpochPosition, isGenesisEpoch } from '@/lib/epoch';
import { ShortId } from '@/components/ui/short-id';
import { Utc8Age, Utc8DateTime } from '@/components/ui/utc8-date';
import { HeadId } from '@/components/ui/head-id';
import { RowSeal } from '@/components/ui/row-seal';
import { percentOf } from '@/lib/format-unit';
import { unitNode } from '@/components/ui/units';

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
  const pageSize = usePageSize(100);
  // a change of page size at md starts each walk again (lane c2-finish, 1d)
  const pagination = useCursorPagination(pageSize);
  const activityPagination = useCursorPagination(pageSize);
  const read = useQuery({
    queryKey: ['block', id],
    queryFn: () => api.getBlock(id as string),
    refetchInterval: (query) => (query.state.data?.cyclesStatus === 'pending' ? 3000 : false),
  });
  const { data: block, error } = read;
  const transactions = useQuery({
    queryKey: ['block-transactions', id, block?.number, pagination.cursor, pageSize],
    queryFn: () =>
      api.getTransactions({
        blockNumber: block!.number,
        limit: pageSize,
        ...(pagination.cursor ? { cursor: pagination.cursor } : {}),
      }),
    enabled: !!block,
  });
  const activities = useQuery({
    queryKey: ['block-activities', block?.number, activityPagination.cursor, pageSize],
    queryFn: () =>
      api.getGlobalActivities({
        blockNumber: block!.number,
        limit: pageSize,
        cursor: activityPagination.cursor,
        // R2b-a 3 (BA-14): the block in tx order, cellbase first, paged forward by the API.
        order: 'asc',
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
  const proposalFees = useQuery({
    queryKey: ['block-proposal-fees', block?.number],
    queryFn: () => api.getBlockProposalFees(block!.number),
    enabled: !!block && block.rewardBreakdown !== null,
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
        // a key a control took (the tab row's ← and →) is not also a step through the chain
        event.defaultPrevented ||
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

  // Awaited, fetching or paused (offline, or a hidden tab between retries): isLoading is pending
  // AND fetching, so a paused read would fall through as if it had completed.
  if (isAwaited(read))
    return (
      <div>
        <main className="container mx-auto py-8">
          <DetailSkeleton />
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
  if (!block && isBadRequestApiError(error))
    return <NotFoundPage notAnId={{ kind: 'block', reason: error.apiMessage }} />;
  if (error && !block)
    return (
      <div>
        <main className="container mx-auto py-8">
          <ReadFailure what="block" error={error} />
        </main>
      </div>
    );
  // past pending, no error means the read succeeded: with no block, an API invariant broke
  if (!block) throw new Error(`Block ${id}: request completed without block evidence`);
  const activation = block.hardforkActivation;
  const resources = activation?.resources?.length
    ? activation.resources
    : hardforkTimeline?.events.find((event) => event.id === activation?.id)?.resources;
  // The epoch the header states, by the one header-epoch rule: genesis's is the zero epoch, which
  // states no length, so it has no bar (a fraction the header does not state).
  const epoch = checkHeaderEpoch(
    { number: block.epochNumber, index: block.epochIndex, length: block.epochLength },
    block.number
  );
  const genesisEpoch = isGenesisEpoch(epoch);
  const client = minerClient(block);
  const epochStart = block.number - epoch.index;
  const amount = (value: string, field: string) => (
    <CkbAmount
      form="column"
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
  const activityRows = activities.data?.data;
  // Both lists are read in tx order: the pager's pages mode, null for one page, counted only where
  // a page's own response counts its rows (lane c2-finish, 1c: the block's transaction count is
  // another read, at its own tip; the tab heading states it).
  const activitiesAt =
    activities.data && rankedPageAt('activities', activityPagination.page, activities.data);
  const transactionsAt =
    transactions.data && rankedPageAt('transactions', pagination.page, transactions.data);
  const parentElapsedMs =
    block.parentTimestamp === null
      ? null
      : new Date(block.timestamp).getTime() - new Date(block.parentTimestamp).getTime();
  if (parentElapsedMs !== null && !Number.isSafeInteger(parentElapsedMs))
    throw new Error(`Block ${block.number}: invalid parent timestamp interval`);
  const cycles = block.cycles === null ? null : Number(block.cycles);
  return (
    <div className="min-h-screen">
      <main className="container mx-auto min-w-0 py-8">
        {error && (
          <p className="max-w-measure" role="alert">
            Unable to refresh block: {error.message}. Showing previously loaded data.
          </p>
        )}
        <DetailLayout
          documentTitle={[formatBlockNumber(block.number), 'block']}
          // The name plate (seals by kind, C10): the height first, on the plate's left edge where
          // the kind word and the hash start, then the step links as one pair, ‹ ›, in reading
          // order (genesis has no previous: its next link stands alone in the pair). Under the
          // stone below md the title has the full width (328 px at 360) and the row (the 26 px
          // height, 166 px, + 10 + 36 + 6 + 36) needs 254 px, 279 at 30 px from md; should it ever
          // be too narrow, the pair wraps as a whole.
          title={
            <span className="flex flex-wrap items-center gap-x-2.5 gap-y-2">
              <span>{formatBlockNumber(block.number)}</span>
              <span className="flex shrink-0 items-center gap-1.5">
                {block.number > 0 && (
                  <ArrowLink
                    href={`/blocks/${block.number - 1}`}
                    label={`Previous block, ${(block.number - 1).toLocaleString('en-US')}`}
                    d="M7.5 2 3.5 6l4 4"
                  />
                )}
                <ArrowLink
                  href={`/blocks/${block.number + 1}`}
                  label={`Next block, ${(block.number + 1).toLocaleString('en-US')}`}
                  d="M4.5 2 8.5 6l-4 4"
                />
              </span>
            </span>
          }
          // The id line: the full hash alone (the kind line says "Block"), in the plate's voice
          // (C10 round 2), the copy button after it (copy has one form: lane c2-finish, item 4);
          // one line or two halves, never a scroller (the lead's ruling a).
          identity={<HeadId value={block.hash} kind="hash" copy="copy block hash" />}
          // The block's stone (seals by kind, §0: 104 px, 80 below md), beside its name plate: its
          // hash, and the transactions it holds in rods. A count the API did not serve is refused
          // (the summary below reads it first), never filled in.
          seal={{
            id: { kind: 'block', hash: block.hash, txs: block.transactionsCount },
            kind: 'Block',
          }}
          // The status strip under the id line (lane D2, PA-13: these facts stood in an aside beside
          // the block's contents): its state, its time, who mined it and its reward. Its fees stand
          // in the evidence tier's facts, the paying cellbase in the reward breakdown's head.
          strip={{
            label: 'Block status',
            facts: [
              {
                label: 'status',
                value: 'canonical',
                dot: 'jade',
                note: unitNode(formatCount(block.confirmations, 'confirmation')),
              },
              {
                // its age alone: the interval to its parent is an evidence fact (lane D2b), so
                // the strip stands in one row at 1280
                label: 'time',
                value: <Utc8DateTime timestamp={block.timestamp} />,
                note: <Utc8Age timestamp={block.timestamp} />,
              },
              {
                label: 'miner',
                value: block.minerAddress ? (
                  <MinerLink
                    address={block.minerAddress}
                    pool={block.minerPool}
                    size="row"
                    unnamed={<Address look="inline" address={block.minerAddress} size="row" />}
                  />
                ) : (
                  'Genesis'
                ),
              },
              {
                label: 'reward',
                // genesis is mined by no one: its reward is not pending, there is none
                value:
                  block.miningReward !== null ? (
                    <CkbAmount
                      form="exact"
                      shannons={parseShannons(block.miningReward, `block ${block.number} reward`)}
                      signed={false}
                      tone="inherit"
                    />
                  ) : block.number === 0 ? (
                    'None: genesis has no miner'
                  ) : (
                    'Not finalized yet'
                  ),
                note: block.rewardPaidAtBlock !== null && (
                  <>
                    paid in{' '}
                    <Link look="inline" href={`/blocks/${block.rewardPaidAtBlock}`}>
                      {formatBlockNumber(block.rewardPaidAtBlock)}
                    </Link>
                  </>
                ),
              },
            ],
          }}
          // The summary is the head's last line (phase 5, Gate 3 note), under the stone and the
          // plate from the page's left edge, where the tab row and the sections start (C10).
          // Lane P1 (the plan's P1 paragraph; said once): the strip states the time, the miner and
          // the reward, so the lede says what only it says — what the block holds, the uncles it
          // includes and which block's reward its cellbase pays (that cellbase's row names its
          // amount and payee); genesis, what it issued: its header's total issuance, C.
          lede={
            <p data-testid="block-summary" className="text-body leading-relaxed">
              {unitNode(formatCount(block.transactionsCount, 'transaction'))}
              {block.number === 0 ? (
                <>
                  , which issued the chain’s first{' '}
                  <CkbAmount form="summary" shannons={dao.C} signed={false} tone="plain" />
                </>
              ) : (
                <>
                  {block.uncles.length > 0 && (
                    <>
                      {' and '}
                      {unitNode(formatCount(block.uncles.length, 'uncle'))},{' '}
                      {block.uncles.map((uncle, index) => (
                        <Fragment key={uncle.hash}>
                          {index > 0 && (index === block.uncles.length - 1 ? ' and ' : ', ')}
                          <Link look="inline" href={getUncleDetailHref(block.number, index)}>
                            {formatBlockNumber(uncle.number)}
                          </Link>
                        </Fragment>
                      ))}
                    </>
                  )}
                  {block.cellbasePays && (
                    <>
                      ; its cellbase pays the reward for{' '}
                      <Link look="inline" href={`/blocks/${block.cellbasePays.hash}`}>
                        {formatBlockNumber(block.cellbasePays.number)}
                      </Link>
                    </>
                  )}
                </>
              )}
              .
            </p>
          }
          // on the kind line, after "Block"
          status={
            activation && (
              <Link look="inline" href="/hardforks" className="text-small">
                HARDFORK ACTIVATION · {activation.shortName.toUpperCase()}
              </Link>
            )
          }
          rawOpen
          tier2={
            <>
              <dl
                data-testid="block-tier2-facts"
                className="text-small grid gap-x-10 gap-y-5 [grid-template-columns:repeat(auto-fit,minmax(min(100%,260px),1fr))]"
              >
                <div>
                  <dt className="text-text-dim mb-1">Epoch</dt>
                  <dd className="flex items-center gap-2">
                    <Link look="key" href={`/blocks/${epochStart}`}>
                      {epoch.number.toLocaleString('en-US')}
                    </Link>
                    {!genesisEpoch && (
                      <EpochBar index={epoch.index} length={epoch.length}>
                        Epoch {epoch.number.toLocaleString('en-US')}:{' '}
                        {epoch.index.toLocaleString('en-US')} of{' '}
                        {epoch.length.toLocaleString('en-US')}
                      </EpochBar>
                    )}
                    <span>
                      {epoch.index.toLocaleString('en-US')} / {epoch.length.toLocaleString('en-US')}
                    </span>
                  </dd>
                  {genesisEpoch && (
                    // What the header says, and why it reads 0 / 0: no length is invented for it.
                    <dd
                      data-testid="block-epoch-genesis"
                      className="text-text-dim max-w-measure mt-1"
                    >
                      the genesis header’s epoch field is {block.rawHeader.epoch}, so it states no
                      epoch length;{' '}
                      <Link look="inline" href="/blocks/1">
                        {formatBlockNumber(1)}
                      </Link>{' '}
                      is the first header to state epoch 0’s
                    </dd>
                  )}
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
                    {percentOf(BigInt(block.size), BigInt(block.maxBlockBytes))} of{' '}
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
                      {percentOf(BigInt(block.cycles), BigInt(block.maxBlockCycles))} of{' '}
                      {BigInt(block.maxBlockCycles).toLocaleString('en-US')}
                    </dd>
                  )}
                </div>
                <div className="min-w-0">
                  <dt className="text-text-dim mb-1">Miner client</dt>
                  <dd className="text-text-bright [overflow-wrap:anywhere]">{client.line}</dd>
                  {client.decoded && (
                    <dd className="text-text-dim">decoded from the cellbase message</dd>
                  )}
                </div>
                {/* The fees its transactions paid (lane D2): a fact of the facts aside, which the
                    status strip replaced; the strip takes the four a reader checks first, and the
                    fees stand here, beside the block's size and cycles. */}
                <div>
                  <dt className="text-text-dim mb-1">Fees</dt>
                  <dd className="text-text-bright">{amount(block.totalFees, 'fees')}</dd>
                </div>
                {/* The time from its parent (lane D2b): the strip's time note before, here so the
                    strip stands in one row at 1280; genesis has no parent and no interval. */}
                {parentElapsedMs !== null && (
                  <div>
                    <dt className="text-text-dim mb-1">Interval</dt>
                    <dd className="text-text-bright">
                      {parentElapsedMs / 1000} s after its parent
                    </dd>
                  </div>
                )}
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
                <RewardBreakdown
                  block={block}
                  proposalFees={proposalFees.data}
                  proposalFeesError={proposalFees.error}
                />
              </DetailSection>
              <div className="grid gap-8 md:grid-cols-2">
                <DetailSection
                  title="Uncles"
                  actions={
                    <span className="text-text-dim text-small">
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
                    <p className="text-text-dim text-small">No uncles in this block.</p>
                  )}
                </DetailSection>
                <DetailSection
                  title="Proposals"
                  actions={
                    ownProposals && (
                      <span className="text-text-dim text-small">
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
                      className="text-text-dim text-small max-w-measure"
                    >
                      Failed to load proposals: {proposalsError.message}
                    </p>
                  ) : (
                    proposals?.map((proposal) => (
                      <article
                        key={`${proposal.uncle?.hash}:${proposal.proposalIndex}`}
                        data-testid="block-proposal-row"
                        className="border-row-rule text-small flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1 border-b py-2.5"
                      >
                        <p className="flex flex-wrap items-baseline gap-2">
                          <span className="text-gold">●</span>
                          {proposal.proposalId}
                          {proposal.uncle &&
                            (uncleIndex(proposal.uncle.hash) >= 0 ? (
                              <Link
                                look="inline"
                                href={getUncleDetailHref(
                                  block.number,
                                  uncleIndex(proposal.uncle.hash)
                                )}
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
                          <p className="text-text-dim max-w-measure">
                            →{' '}
                            <Link look="inline" href={`/tx/${proposal.committedTxHash}`}>
                              <Hash
                                hash={proposal.committedTxHash}
                                size="row"
                                className="text-small"
                              />
                            </Link>{' '}
                            in{' '}
                            <Link look="inline" href={`/blocks/${proposal.committedBlockNumber}`}>
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
                            No indexed commitment in its proposal window.
                          </p>
                        )}
                      </article>
                    ))
                  )}
                  {!proposalsError &&
                    (proposals?.length === 0 ||
                      (block.proposalsCount === 0 && block.unclesCount === 0)) && (
                      <p className="text-text-dim text-small">No proposals in this block.</p>
                    )}
                  {!proposalsError &&
                    proposals === undefined &&
                    (block.proposalsCount > 0 || block.unclesCount > 0) && (
                      <p className="text-text-dim text-small">Loading proposals…</p>
                    )}
                </DetailSection>
              </div>
            </>
          }
          rawData={
            <dl className="text-small grid min-w-0 gap-3">
              <div className="flex flex-wrap items-baseline justify-between gap-3">
                <span className="text-text-dim">Header · version {block.version} · 208 bytes</span>
                <span>
                  <Link
                    look="inline"
                    href={`/blocks/${block.number}.raw`}
                    target="_blank"
                    rel="noreferrer"
                  >
                    JSON
                  </Link>
                  {' · '}
                  <Link
                    look="inline"
                    href={`/blocks/${block.number}.md`}
                    target="_blank"
                    rel="noreferrer"
                  >
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
                        <Link look="inline" href={`/blocks/${value}`}>
                          {value}
                        </Link>
                      ) : (
                        value
                      )}
                    </span>
                    {key === 'epoch' && (
                      <span className="text-text-dim">
                        {' '}
                        = {formatEpochPosition(epoch, block.number)}
                      </span>
                    )}
                    {key === 'dao' && <DaoHeaderFields dao={dao} />}
                  </dd>
                </div>
              ))}
              <div className="grid gap-1 sm:grid-cols-[160px_minmax(0,1fr)]">
                <dt className="text-text-dim">Block bytes</dt>
                {/* R2b-a 5: `size` is the molecule-serialized block. */}
                <dd>{unitNode(formatCount(block.size, 'byte'))}, molecule-serialized</dd>
              </div>
              <div className="grid gap-1 sm:grid-cols-[160px_minmax(0,1fr)]">
                <dt className="text-text-dim">Cellbase message bytes</dt>
                <dd className="[overflow-wrap:anywhere]">
                  {block.minerMessage ?? 'None: genesis has no miner message'}
                </dd>
              </div>
            </dl>
          }
        >
          {activation && resources && (
            <section aria-label="Hardfork resources">
              <PromptHeading
                command="hardfork resources"
                args={activation.shortName}
                className="mb-3"
              />
              <div className="flex flex-wrap gap-4">
                {resources.map((resource) => (
                  <a
                    key={resource.url}
                    href={resource.url}
                    target="_blank"
                    rel="noreferrer"
                    data-testid="block-hardfork-resource-link"
                    className="text-interactive text-small"
                  >
                    {resource.label}
                  </a>
                ))}
              </div>
            </section>
          )}
          {/* The block's contents across the whole measure (lane D2): the facts that stood in an
                aside beside them are the head's status strip. */}
          <div className="min-w-0">
            {/* The block's contents open on their command line, the tab row (the controls
                  board); the section keeps its name for the outline. */}
            <h2 className="sr-only">Block contents</h2>
            <Tabs
              label="block contents"
              command
              // One activity row per transaction of the block (the block feed's contract).
              tabs={[
                { key: 'activities', label: 'activities', count: block.transactionsCount },
                { key: 'transactions', label: 'transactions', count: block.transactionsCount },
              ]}
              value={view}
              onChange={setView}
            >
              <div className="pt-4">
                {transactions.data && !transactions.data.hasMore && !pagination.hasPrevious && (
                  <CommittedFrom transactions={transactions.data.data} block={block} />
                )}
                {view === 'activities' ? (
                  <>
                    {isAwaited(activities) && <p role="status">Loading activities…</p>}
                    {activities.error && (
                      <p className="max-w-measure" role="alert">
                        Unable to read activities: {activities.error.message}
                      </p>
                    )}
                    {activities.data && activityRows && (
                      <>
                        {activityRows.map((activity) => (
                          <ActivityRow
                            key={activity.txHash}
                            activity={activity}
                            headline={
                              activity.isCellbase ? (
                                <CellbaseHeadline block={block} activity={activity} />
                              ) : undefined
                            }
                            evidence={
                              <>
                                #{activity.txIndex} ·{' '}
                                <Link
                                  look="key"
                                  href={`/tx/${activity.txHash}`}
                                  className="transition-colors"
                                >
                                  <ShortId value={activity.txHash} kind="hash" size="row" />
                                </Link>
                              </>
                            }
                          />
                        ))}
                        {activityRows.length === 0 && (
                          <p className="text-text-dim text-small">No activities in this block.</p>
                        )}
                        {activitiesAt && (
                          <Pager
                            mode="pages"
                            label="activities"
                            {...activitiesAt}
                            onPrevious={activityPagination.goToPrevious}
                            onNext={() => activityPagination.goToNext(activities.data.nextCursor)}
                          />
                        )}
                      </>
                    )}
                  </>
                ) : (
                  <>
                    {isAwaited(transactions) && <p role="status">Loading transactions…</p>}
                    {transactions.error && (
                      <p className="max-w-measure" role="alert">
                        Unable to read transactions: {transactions.error.message}
                      </p>
                    )}
                    {transactions.data && (
                      <>
                        <BlockTransactionsTable
                          transactions={transactions.data.data}
                          blockNumber={block.number}
                        />
                        {transactionsAt && (
                          <Pager
                            mode="pages"
                            label="transactions"
                            {...transactionsAt}
                            onPrevious={pagination.goToPrevious}
                            onNext={() => pagination.goToNext(transactions.data.nextCursor)}
                          />
                        )}
                      </>
                    )}
                  </>
                )}
              </div>
            </Tabs>
          </div>
        </DetailLayout>
      </main>
    </div>
  );
}

// ---- F2 blocks: the Block board's small pieces ----

function ArrowLink({ href, label, d }: { href: string; label: string; d: string }) {
  return (
    // a block: a box whose mark draws its own ink, quiet, celadon under the pointer (the link's
    // hover reaches it through the group)
    <Link
      look="block"
      href={href}
      aria-label={label}
      className="border-base-border group flex h-9 w-9 shrink-0 items-center justify-center border"
    >
      <svg
        aria-hidden="true"
        width="12"
        height="12"
        viewBox="0 0 12 12"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        className="text-text-dim group-hover:text-jade group-focus-visible:text-jade"
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
      className="bg-base-border flex h-1 w-[72px] shrink-0 overflow-hidden"
    >
      <span className="bg-text h-full" style={{ width: `${((index + 1) / length) * 100}%` }} />
    </span>
  );
}

function ScalePending({ error, value }: { error: Error | null; value: number }) {
  return (
    <span className="text-text-bright">
      {value.toLocaleString('en-US')}
      <span className="text-text-dim text-meta ml-2">
        {error ? `bar scale unavailable: ${error.message}` : 'reading the list scale…'}
      </span>
    </span>
  );
}

/**
 * The miner client a block's cellbase message names, as the API decoded it. The API serves no
 * message for block 0 alone: it reads no miner witness for genesis (CKB builds genesis's cellbase
 * witness with the bootstrap lock and an empty message, and nobody mined it); a later block with
 * none is a contract violation. Every later block's message is its bytes, `0x` when empty, and its
 * text is null when the bytes hold no readable text (padding only, not UTF-8, or control
 * characters). Only a decoded text is `decoded`.
 */
function minerClient(block: Pick<Block, 'number' | 'minerMessage' | 'minerMessageText'>): {
  line: string;
  decoded: boolean;
} {
  if (block.minerMessage === null) {
    if (block.number !== 0)
      throw new Error(
        `Block ${block.number}: the API served no cellbase message; only genesis has none`
      );
    return { line: 'None: genesis has no miner', decoded: false };
  }
  if (block.minerMessageText !== null) return { line: block.minerMessageText, decoded: true };
  if (block.minerMessage === '0x')
    return { line: 'None: the cellbase message is empty', decoded: false };
  return { line: 'No readable text; see the raw bytes below', decoded: false };
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
  // the same quiet note, now its own line under the tab row (it stood at the row's right end,
  // which is the tab list now)
  return (
    <span className="text-text-dim text-small mb-3 block">
      {unitNode(
        origins.size === 1
          ? `${committed.length.toLocaleString('en-US')} committed from ${formatBlockNumber(only)}'s proposals, ${formatCount(block.number - only, 'block')} later`
          : `${committed.length.toLocaleString('en-US')} committed from the proposals of ${formatCount(origins.size, 'block')}`
      )}
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
    // The row leads with the uncle's row stone (lane D2b; U-03, R8): the block stone without rods,
    // the chain keeping no transactions for an uncle, in the dead ink, as the fork page's dropped
    // block's; its three lines beside it
    <article className="border-row-rule text-small flex items-start border-b py-2.5">
      <RowSeal id={{ kind: 'block', hash: uncle.hash, txs: 'not kept' }} state="ink" />
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="flex flex-wrap items-baseline gap-x-2">
          <Link look="key" href={getUncleDetailHref(blockNumber, index)}>
            {formatBlockNumber(uncle.number)}
          </Link>
          <span className="text-text-dim">uncle</span>
          <Link look="inline" href={getUncleDetailHref(blockNumber, index)}>
            <Hash hash={uncle.hash} size="row" className="text-small" />
          </Link>
        </span>
        <span className="text-text-dim">
          {error ? (
            `sibling facts unavailable: ${error.message}`
          ) : !detail ? (
            'reading its sibling…'
          ) : detail.sameParentAs === null ? (
            <>
              the canonical{' '}
              <Link look="inline" href={`/blocks/${detail.canonicalAtHeight.hash}`}>
                {formatBlockNumber(detail.canonicalAtHeight.number)}
              </Link>{' '}
              has another parent
            </>
          ) : (
            <>
              sibling of{' '}
              <Link look="inline" href={`/blocks/${detail.canonicalAtHeight.hash}`}>
                {formatBlockNumber(detail.sameParentAs)}
              </Link>{' '}
              <Link look="inline" href={`/blocks/${detail.canonicalAtHeight.hash}`}>
                <Hash hash={detail.canonicalAtHeight.hash} size="row" className="text-small" />
              </Link>
              , same parent{' '}
              <Link look="inline" href={`/blocks/${uncle.parentHash}`}>
                {formatBlockNumber(uncle.number - 1)}
              </Link>
            </>
          )}
        </span>
        <span className="text-text-dim">
          {unitNode(formatUtc8Time(uncle.timestamp))}
          {detail?.msAfterSibling != null &&
            `, ${detail.msAfterSibling >= 0 ? `${detail.msAfterSibling.toLocaleString('en-US')} ms after` : `${(-detail.msAfterSibling).toLocaleString('en-US')} ms before`} its sibling`}{' '}
          · {unitNode(formatCount(uncle.proposalIds.length, 'proposal'))}
        </span>
      </div>
    </article>
  );
}

/**
 * "Cellbase  reward for #N, to ckb1…xxxxxxxx" (R2b-a 1, BA-12; lane P1, the plan's P1 paragraph):
 * the block whose reward this cellbase pays, and to whom — its parties, the cells it creates, in
 * the row form — so its amount is not read as this block's own reward, which the status strip
 * states. The words are the row's statement, the reading ink; its links are inside the line.
 */
function CellbaseHeadline({
  block,
  activity,
}: {
  block: Pick<Block, 'cellbasePays'>;
  activity: Pick<GlobalActivity, 'participants'>;
}) {
  return (
    <>
      Cellbase
      {block.cellbasePays && (
        <span className="text-text font-normal">
          {' '}
          reward for{' '}
          <Link look="inline" href={`/blocks/${block.cellbasePays.hash}`}>
            {formatBlockNumber(block.cellbasePays.number)}
          </Link>
          {activity.participants.length > 0 && (
            <>
              , to{' '}
              {activity.participants.map((party, i) => (
                <Fragment key={party.lockHash ?? party.lockHashPrefix ?? i}>
                  {i > 0 && (i === activity.participants.length - 1 ? ' and ' : ', ')}
                  <ParticipantRefView participant={party} size="row" compact quiet />
                </Fragment>
              ))}
            </>
          )}
        </span>
      )}
    </>
  );
}
