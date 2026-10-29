'use client';

import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { notFound, useParams } from '@/src/navigation';
import {
  api,
  isNotFoundApiError,
  type CanonicalBranchBlock,
  type OrphanedBlock,
  type OrphanedTransaction,
} from '@/lib/api';
import { Header } from '@/components/layout/header';
import { NotFoundPage } from '@/components/not-found-page';
import { formatObserverTimeUtc8 } from '@/lib/fork-time';
import { formatCkbAmount } from '@/lib/utils';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { ListTable } from '@/components/ui/list-table';

const fullHash = (hash: string) => <span className="break-all font-mono text-xs">{hash}</span>;
const blockAt = (number: number) => (
  <Link href={`/blocks/${number}`} className="text-emphasis hover:underline">
    #{number.toLocaleString()}
  </Link>
);
const historicalHeight = (number: number) => <span>#{number.toLocaleString()}</span>;
const blockRows = (label: string, rows: CanonicalBranchBlock[], current = false) => (
  <ListTable
    label={label}
    rows={rows}
    rowKey={(block) => block.hash}
    empty="No blocks on this branch."
    columns={[
      {
        key: 'height',
        label: 'Height',
        minWidth: 90,
        render: (block) => (current ? blockAt(block.number) : historicalHeight(block.number)),
      },
      { key: 'hash', label: 'Block hash', minWidth: 330, render: (block) => fullHash(block.hash) },
      {
        key: 'miner',
        label: 'Miner lock hash',
        minWidth: 330,
        render: (block) =>
          block.minerLockHash ? fullHash(block.minerLockHash) : 'Miner witness unavailable',
      },
      { key: 'time', label: 'Header time', minWidth: 180, render: (block) => block.timestamp },
    ]}
    mobileRow={(block) => (
      <div className="min-w-0 space-y-1 text-xs">
        {current ? blockAt(block.number) : historicalHeight(block.number)}
        <div>{fullHash(block.hash)}</div>
        <div>
          Miner: {block.minerLockHash ? fullHash(block.minerLockHash) : 'witness unavailable'}
        </div>
        <div>{block.timestamp}</div>
      </div>
    )}
  />
);

export default function ForkDetailPage() {
  const params = useParams();
  const id = Number(params.id);
  const query = useQuery({
    queryKey: ['fork', id],
    queryFn: () => api.getForkDetail(id),
    enabled: Number.isSafeInteger(id) && id >= 0,
  });
  if (!Number.isSafeInteger(id) || id < 0) notFound();
  if (!query.data && isNotFoundApiError(query.error)) {
    return (
      <NotFoundPage
        entity={{ kind: 'fork', identifier: String(id), checkedEndpoint: `/forks/${id}` }}
      />
    );
  }
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto min-w-0 px-4 py-8">
        {query.isLoading && <p role="status">Loading fork evidence…</p>}
        {query.error && (
          <p role="alert">Fork evidence unavailable: {(query.error as Error).message}</p>
        )}
        {query.data && <ForkEvidence data={query.data} />}
      </main>
    </div>
  );
}

function ForkEvidence({ data }: { data: Awaited<ReturnType<typeof api.getForkDetail>> }) {
  const { event, orphanedBlocks, orphanedTransactions, adoptedBranch, canonicalBranch } = data;
  return (
    <DetailLayout
      title={`Fork event #${event.id}`}
      identity={
        <>
          Detected {formatObserverTimeUtc8(event.detectedAt)} ·{' '}
          {event.eventType === 'deep' ? 'deep fork detection' : 'branch switch'}
        </>
      }
      facts={[
        { label: 'Fork point', value: blockAt(event.forkPointNumber) },
        {
          label: event.eventType === 'deep' ? 'Indexed → node tip' : 'Old → adopted tip',
          value: (
            <>
              #{event.oldTipNumber.toLocaleString()} → #{event.newTipNumber.toLocaleString()}
            </>
          ),
        },
        {
          label: event.eventType === 'deep' ? 'Detected depth' : 'Switch depth',
          value: `${event.depth.toLocaleString()} blocks`,
        },
        { label: 'Old branch transactions', value: event.orphanedTxsCount.toLocaleString() },
        {
          label: 'Later reversal',
          value: event.revertedByEvent ? (
            <Link href={`/forks/${event.revertedByEvent}`}>Event #{event.revertedByEvent}</Link>
          ) : (
            'None recorded'
          ),
        },
        {
          label: 'Events reversed here',
          value: event.reversesEvents?.length
            ? event.reversesEvents.map((reversed, index) => (
                <span key={reversed}>
                  {index ? ', ' : ''}
                  <Link href={`/forks/${reversed}`}>#{reversed}</Link>
                </span>
              ))
            : 'None recorded',
        },
        ...(event.resolvedAt
          ? [
              {
                label: 'Resolution',
                value: `${event.resolutionAction ?? 'Recorded'} · ${event.resolvedAt}`,
              },
            ]
          : []),
        {
          label: 'Current indexed tip',
          value: data.canonicalTipNumber == null ? 'Unavailable' : blockAt(data.canonicalTipNumber),
        },
      ]}
    >
      <DetailSection
        title={event.eventType === 'deep' ? 'Two tips at detection' : 'Two branches at this switch'}
      >
        <p className="text-text-dim mb-5 text-xs">
          {event.eventType === 'deep'
            ? 'This deep-fork detection paused indexing before rollback. No branch was orphaned by this event; the recorded tips show the disagreement.'
            : 'These branch paths are traced from the persisted old and adopted tip hashes back to the recorded fork point. Their current fate appears below.'}
        </p>
        {/* Stack full-width: side-by-side half-columns crush 930px tables (FF-D1). */}
        <div className="grid min-w-0 gap-8">
          <div className="min-w-0">
            <h3 className="text-text-bright mb-3 text-sm">
              {event.eventType === 'deep'
                ? 'Indexed tip · no rollback'
                : 'Old branch · dropped at detection'}
            </h3>
            <p className="mb-3 text-xs">Tip {fullHash(event.oldTipHash)}</p>
            {event.eventType === 'deep' ? (
              <p className="text-text-dim text-xs">No blocks orphaned by this detection.</p>
            ) : (
              blockRows(
                'Historical old branch',
                orphanedBlocks.map((block) => ({
                  number: block.number,
                  hash: block.hash,
                  timestamp: block.timestamp,
                  transactionsCount: block.transactionsCount,
                  minerLockHash: block.minerLockHash,
                }))
              )
            )}
          </div>
          <div className="min-w-0">
            <h3 className="text-text-bright mb-3 text-sm">
              {event.eventType === 'deep'
                ? 'Node tip · not adopted by indexer'
                : 'Branch adopted at detection'}
            </h3>
            <p className="mb-3 text-xs">Tip {fullHash(event.newTipHash)}</p>
            {event.reconcileInterrupted && (
              <p className="text-text-dim mb-3 text-xs">
                Reconciliation interrupted by a later rollback
              </p>
            )}
            {event.supersededByTipHash && (
              <p className="text-text-dim mb-3 text-xs">
                This observed tip was superseded before replay. Canonical block #
                {event.newTipNumber} was {fullHash(event.supersededByTipHash)}.
              </p>
            )}
            {event.eventType === 'deep' ? (
              <p className="text-text-dim text-xs">
                Indexer paused before adopting the node branch.
              </p>
            ) : (
              blockRows('Historically adopted branch', adoptedBranch)
            )}
          </div>
        </div>
      </DetailSection>
      {event.eventType !== 'deep' && (
        <>
          <DetailSection title="Old branch fate now">
            <ListTable
              label="Old block fate"
              rows={orphanedBlocks}
              rowKey={(block: OrphanedBlock) => block.hash}
              empty="No old branch blocks recorded for this event."
              columns={[
                {
                  key: 'block',
                  label: 'Old block',
                  minWidth: 340,
                  render: (block: OrphanedBlock) => (
                    <>
                      {historicalHeight(block.number)} {fullHash(block.hash)}
                    </>
                  ),
                },
                {
                  key: 'fate',
                  label: 'Fate now',
                  minWidth: 155,
                  render: (block: OrphanedBlock) => block.fate.replaceAll('_', ' '),
                },
                {
                  key: 'uncle',
                  label: 'Uncle inclusion',
                  minWidth: 155,
                  render: (block: OrphanedBlock) =>
                    block.uncleIn ? (
                      <>
                        {blockAt(block.uncleIn.blockNumber)} · distance {block.uncleIn.distance}
                      </>
                    ) : (
                      '—'
                    ),
                },
                {
                  key: 'competing',
                  label: 'Current block at height',
                  minWidth: 190,
                  render: (block: OrphanedBlock) =>
                    block.competingBlock
                      ? fullHash(block.competingBlock.hash)
                      : block.fate === 'canonical_now'
                        ? 'This old block re-entered'
                        : 'Unavailable',
                },
                {
                  key: 'competingMiner',
                  label: 'Current competing miner lock',
                  minWidth: 220,
                  render: (block: OrphanedBlock) =>
                    block.competingBlock
                      ? block.competingBlock.minerLockHash
                        ? fullHash(block.competingBlock.minerLockHash)
                        : 'Miner witness unavailable'
                      : block.fate === 'canonical_now'
                        ? 'Same block now'
                        : 'No current comparison',
                },
                {
                  key: 'gap',
                  label: 'Header time gap',
                  minWidth: 125,
                  render: (block: OrphanedBlock) =>
                    block.timestampGapMs == null
                      ? '—'
                      : `${block.timestampGapMs > 0 ? '+' : ''}${block.timestampGapMs.toLocaleString()} ms`,
                },
              ]}
              mobileRow={(block: OrphanedBlock) => (
                <div className="min-w-0 space-y-2 text-xs">
                  <div>
                    {historicalHeight(block.number)} {fullHash(block.hash)}
                  </div>
                  <div>Fate now: {block.fate.replaceAll('_', ' ')}</div>
                  <div>
                    Uncle:{' '}
                    {block.uncleIn ? (
                      <>
                        {blockAt(block.uncleIn.blockNumber)} · distance {block.uncleIn.distance}
                      </>
                    ) : (
                      'none observed'
                    )}
                  </div>
                  {block.competingBlock && (
                    <>
                      <div>Current competing block: {fullHash(block.competingBlock.hash)}</div>
                      <div>
                        Current competing miner lock:{' '}
                        {block.competingBlock.minerLockHash
                          ? fullHash(block.competingBlock.minerLockHash)
                          : 'witness unavailable'}
                      </div>
                    </>
                  )}
                  {block.timestampGapMs != null && (
                    <div>
                      Header time gap: {block.timestampGapMs > 0 ? '+' : ''}
                      {block.timestampGapMs.toLocaleString()} ms
                    </div>
                  )}
                </div>
              )}
            />
            <p className="text-text-dim mt-3 text-xs">
              A historical drop can later be reversed. “Canonical now” means the exact old block
              hash is currently indexed at its height.
            </p>
          </DetailSection>
          <DetailSection title="Transactions from the old branch">
            <ListTable
              label="Old branch transaction fate"
              rows={orphanedTransactions}
              rowKey={(tx: OrphanedTransaction) => `${tx.blockHash}-${tx.txIndex}`}
              empty="No old branch transactions retained."
              columns={[
                {
                  key: 'tx',
                  label: 'Transaction',
                  minWidth: 340,
                  render: (tx: OrphanedTransaction) =>
                    tx.committedAt ? (
                      <Link
                        href={`/tx/${tx.hash}`}
                        className="text-emphasis break-all hover:underline"
                      >
                        {tx.hash}
                      </Link>
                    ) : (
                      fullHash(tx.hash)
                    ),
                },
                {
                  key: 'origin',
                  label: 'Old block',
                  minWidth: 100,
                  render: (tx: OrphanedTransaction) => historicalHeight(tx.blockNumber),
                },
                {
                  key: 'fate',
                  label: 'Fate now',
                  minWidth: 180,
                  render: (tx: OrphanedTransaction) => tx.fate.replaceAll('_', ' '),
                },
                {
                  key: 'commit',
                  label: 'Canonical commit',
                  minWidth: 135,
                  render: (tx: OrphanedTransaction) =>
                    tx.committedAt ? (
                      <>
                        {blockAt(tx.committedAt.blockNumber)} · tx {tx.committedAt.txIndex}
                      </>
                    ) : (
                      '—'
                    ),
                },
                {
                  key: 'capacity',
                  label: 'Output capacity',
                  minWidth: 170,
                  render: (tx: OrphanedTransaction) =>
                    tx.totalCapacity == null
                      ? 'Unavailable'
                      : `${formatCkbAmount(tx.totalCapacity).full} CKB`,
                },
              ]}
              mobileRow={(tx: OrphanedTransaction) => (
                <div className="min-w-0 space-y-2 text-xs">
                  {tx.committedAt ? (
                    <Link href={`/tx/${tx.hash}`} className="text-emphasis break-all">
                      {tx.hash}
                    </Link>
                  ) : (
                    fullHash(tx.hash)
                  )}
                  <div>
                    Old block {historicalHeight(tx.blockNumber)} · {tx.fate.replaceAll('_', ' ')}
                  </div>
                  <div>
                    Canonical commit:{' '}
                    {tx.committedAt ? blockAt(tx.committedAt.blockNumber) : 'none observed'}
                  </div>
                  <div>
                    Output capacity:{' '}
                    {tx.totalCapacity == null
                      ? 'Unavailable'
                      : `${formatCkbAmount(tx.totalCapacity).full} CKB`}
                  </div>
                </div>
              )}
            />
          </DetailSection>
        </>
      )}
      {event.eventType !== 'deep' && (
        <DetailSection title="Current canonical view at the affected heights">
          <p className="text-text-dim mb-3 text-xs">
            Current indexed chain state; this can differ from the branch this event originally
            adopted.
          </p>
          {blockRows('Current canonical branch', canonicalBranch, true)}
        </DetailSection>
      )}
    </DetailLayout>
  );
}
