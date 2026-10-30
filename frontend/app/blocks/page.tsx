'use client';

import { useQuery, keepPreviousData } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { api, type Block } from '@/lib/api';
import { Header } from '@/components/layout/header';
import { DetailLayout } from '@/components/ui/detail-layout';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { Hash } from '@/components/ui/hash';
import { Address } from '@/components/ui/address';
import { ExactCkb } from '@/components/activity-event-row';
import { BlockContents, ResourceBar, resourceScale } from '@/components/blocks/block-visual';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { parseShannons } from '@/lib/ckb-decimal';
import { formatTimeAgo } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';

export default function BlocksPage() {
  const { cursor, hasPrevious, page, goToNext, goToPrevious } = useCursorPagination();
  const limit = DEFAULT_PAGE_SIZE;
  const { data, isLoading, error } = useQuery({
    queryKey: ['blocks', cursor, limit],
    queryFn: () => api.getBlocks({ cursor, limit }),
    staleTime: 30_000,
    placeholderData: keepPreviousData,
    refetchInterval: (query) =>
      query.state.data?.data.some((block) => block.cyclesStatus === 'pending') ? 3000 : false,
  });
  const sizeScale = resourceScale(data ? data.data.map((block) => block.size) : []);
  const cyclesScale = resourceScale(
    data ? data.data.flatMap((block) => (block.cycles === null ? [] : [Number(block.cycles)])) : []
  );
  const columns: ListColumn<Block>[] = [
    {
      key: 'block',
      label: 'Block',
      minWidth: 175,
      render: (block) => (
        <div className="space-y-1">
          <Link href={`/blocks/${block.number}`} className="text-text-bright hover:text-jade">
            {formatBlockNumber(block.number)}
          </Link>
          <span className="text-text-dim block text-[11px]">
            <Hash hash={block.hash} copyable={false} className="text-xs" />
          </span>
          {block.hardforkActivation && (
            <Link href="/hardforks" className="text-text-dim block text-[11px]">
              HF · {block.hardforkActivation.shortName.toUpperCase()}
            </Link>
          )}
        </div>
      ),
    },
    {
      key: 'age',
      label: 'Age',
      minWidth: 78,
      render: (block) => <span className="text-text-dim">{formatTimeAgo(block.timestamp)}</span>,
    },
    {
      key: 'contents',
      label: 'Contents',
      minWidth: 160,
      render: (block) => (
        <BlockContents
          transactionSizes={block.transactionSizes}
          proposalsCount={block.proposalsCount}
          unclesCount={block.unclesCount}
        />
      ),
    },
    {
      key: 'miner',
      label: 'Miner',
      minWidth: 136,
      render: (block) =>
        block.minerAddress ? (
          <Address address={block.minerAddress} />
        ) : (
          <span className="text-text-dim">Genesis</span>
        ),
    },
    {
      key: 'reward',
      label: 'Reward',
      minWidth: 165,
      numeric: true,
      render: (block) =>
        block.miningReward === null ? (
          <span className="text-text-dim text-[11px]">
            {block.rewardFinalizesAt === null ? (
              'Genesis'
            ) : (
              <>
                in{' '}
                <Link href={`/blocks/${block.rewardFinalizesAt}`}>
                  {formatBlockNumber(block.rewardFinalizesAt)}
                </Link>
              </>
            )}
          </span>
        ) : (
          <ExactCkb
            shannons={parseShannons(block.miningReward, `block ${block.number} reward`)}
            signed={false}
            tone="plain"
          />
        ),
    },
    {
      key: 'fees',
      label: 'Fees',
      minWidth: 150,
      numeric: true,
      render: (block) => (
        <ExactCkb
          shannons={parseShannons(block.totalFees, `block ${block.number} fees`)}
          signed={false}
          tone="plain"
        />
      ),
    },
    {
      key: 'size',
      label: 'Size, bytes',
      minWidth: 155,
      numeric: true,
      render: (block) => (
        <ResourceBar
          label="Size, bytes"
          value={block.size}
          overhead={block.overheadSize}
          scale={sizeScale}
        />
      ),
    },
    {
      key: 'cycles',
      label: 'Cycles',
      minWidth: 180,
      numeric: true,
      render: (block) =>
        block.cycles === null ? (
          <span className="text-text-dim">
            {block.cyclesStatus === 'failed' ? 'Replay failed' : 'Not yet run'}
          </span>
        ) : (
          <ResourceBar label="Cycles" value={Number(block.cycles)} scale={cyclesScale} />
        ),
    },
  ];
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto min-w-0 px-4 py-8">
        <DetailLayout
          title="Blocks"
          kind="index"
          actions={
            <p className="text-text-dim flex flex-wrap gap-4 text-[11px]">
              <span>
                <span className="text-jade">◆</span> cellbase
              </span>
              <span>
                <span className="text-jade">■</span> transaction · area ∝ bytes
              </span>
              <span>
                <span className="text-gold">●</span> proposal
              </span>
            </p>
          }
        >
          {isLoading && (
            <p role="status" className="text-text-dim">
              Loading blocks…
            </p>
          )}
          {error && (
            <p role="alert" className="text-text-dim">
              Unable to read blocks: {error.message}
            </p>
          )}
          {data && (
            <>
              <ListTable
                label="Latest blocks"
                columns={columns}
                rows={data.data}
                rowKey={(block) => block.hash}
                empty="No blocks indexed."
              />
              <CursorPagination
                total={data.total}
                totalLabel="blocks"
                pageSize={limit}
                page={page}
                hasMore={data.hasMore}
                hasPrevious={hasPrevious}
                onNext={() => goToNext(data.nextCursor)}
                onPrevious={goToPrevious}
              />
            </>
          )}
        </DetailLayout>
      </main>
    </div>
  );
}
