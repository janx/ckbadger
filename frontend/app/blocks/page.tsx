'use client';

import { useQuery, keepPreviousData } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { api, type Block } from '@/lib/api';
import { Header } from '@/components/layout/header';
import { DetailLayout } from '@/components/ui/detail-layout';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import { Hash } from '@/components/ui/hash';
import { Address } from '@/components/ui/address';
import { MinerLink } from '@/components/forks/miner-link';
import { ExactCkb } from '@/components/activity-event-row';
import {
  BlockContents,
  BlocksLimitsFootnote,
  ResourceBar,
  ScaleHeader,
  resourceScale,
} from '@/components/blocks/block-visual';
import { CLASS_COLORS, DATA_COLOR } from '@/lib/design-tokens';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { parseShannons } from '@/lib/ckb-decimal';
import { formatTimeAgo } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';

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
      minWidth: 200,
      render: (block) => <BlockIdentity block={block} />,
    },
    {
      key: 'age',
      label: 'Age',
      minWidth: 70,
      className: 'whitespace-nowrap',
      render: (block) => <span className="text-text-dim">{formatTimeAgo(block.timestamp)}</span>,
    },
    {
      key: 'contents',
      label: 'Contents',
      minWidth: 170,
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
      minWidth: 160,
      className: 'whitespace-nowrap',
      render: miner,
    },
    {
      key: 'reward',
      label: 'Reward, CKB',
      minWidth: 140,
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
            unit={false}
          />
        ),
    },
    {
      key: 'fees',
      label: 'Fees, CKB',
      minWidth: 112,
      numeric: true,
      render: (block) => (
        <ExactCkb
          shannons={parseShannons(block.totalFees, `block ${block.number} fees`)}
          signed={false}
          tone="plain"
          unit={false}
        />
      ),
    },
    {
      key: 'size',
      label: 'Size, bytes',
      header: <ScaleHeader label="Size, bytes" scale={sizeScale} />,
      minWidth: 182,
      className: 'whitespace-nowrap',
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
      header: <ScaleHeader label="Cycles" scale={cyclesScale} />,
      minWidth: 196,
      className: 'whitespace-nowrap',
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
        <DetailLayout title="Blocks" kind="index" actions={<BlocksLegend />}>
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
                narrowColumns={NARROW_COLUMNS}
                mobileRow={(block) => <BlockPhoneRow block={block} />}
              />
              <BlocksLimitsFootnote blocks={data.data} />
              <CursorPagination
                summary={
                  <span className="text-xs">{blocksRange(data.data, data.total, page)}</span>
                }
                hasMore={data.hasMore}
                hasPrevious={hasPrevious}
                onNext={() => goToNext(data.nextCursor)}
                onPrevious={goToPrevious}
                previousLabel="Newer"
                nextLabel="Older →"
              />
            </>
          )}
        </DetailLayout>
      </main>
    </div>
  );
}

/** Newest first: page one is "50 newest of N"; later pages name their first and last heights. */
function blocksRange(blocks: readonly Block[], total: number | undefined, page: number): string {
  if (total === undefined) throw new Error('Blocks list response has no total');
  const of = total.toLocaleString('en-US');
  if (!blocks.length) return `0 of ${of}`;
  if (page === 1) return `${blocks.length.toLocaleString('en-US')} newest of ${of}`;
  return `${formatBlockNumber(blocks[0].number)} to ${formatBlockNumber(blocks[blocks.length - 1].number)} of ${of}`;
}

/** The Blocks v3 legend: the glyph grammar and the two-part size bar. */
function BlocksLegend() {
  return (
    <div
      aria-label="Legend"
      className="text-text-dim flex flex-wrap items-center gap-x-[18px] gap-y-1.5 text-xs"
    >
      <span className="flex items-center gap-1.5">
        <svg aria-hidden="true" width="10" height="10" viewBox="0 0 10 10">
          <path d="M5 .5 9.5 5 5 9.5.5 5z" fill={DATA_COLOR} />
        </svg>
        cellbase
      </span>
      <span className="flex items-center gap-1.5">
        <svg aria-hidden="true" width="10" height="10" viewBox="0 0 10 10">
          <rect x="1" y="1" width="8" height="8" fill={DATA_COLOR} fillOpacity="0.8" />
        </svg>
        transaction, area ∝ bytes
      </span>
      <span className="flex items-center gap-1.5">
        <svg aria-hidden="true" width="10" height="10" viewBox="0 0 10 10">
          <circle cx="5" cy="5" r="2.4" fill={CLASS_COLORS.dao} />
        </svg>
        proposal
      </span>
      <span className="flex items-center gap-1.5">
        <span aria-hidden="true" className="flex h-2 w-[18px] overflow-hidden rounded-sm">
          <span className="w-2" style={{ backgroundColor: CLASS_COLORS.ckb }} />
          <span className="flex-1" style={{ backgroundColor: DATA_COLOR }} />
        </span>
        block overhead · user transactions
      </span>
    </div>
  );
}

/** "#20,879,198 0xd161…8fef" on one line; a hardfork activation below it. */
function BlockIdentity({ block }: { block: Block }) {
  return (
    <div className="space-y-1">
      <div className="whitespace-nowrap">
        <Link href={`/blocks/${block.number}`} className="text-text-bright hover:text-jade">
          {formatBlockNumber(block.number)}
        </Link>{' '}
        <Hash
          hash={block.hash}
          startChars={6}
          endChars={4}
          copyable={false}
          className="text-text-dim text-[11.5px]"
        />
      </div>
      {block.hardforkActivation && (
        <Link href="/hardforks" className="text-text-dim block text-[11px]">
          HF · {block.hardforkActivation.shortName.toUpperCase()}
        </Link>
      )}
    </div>
  );
}

function miner(block: Block) {
  return block.minerAddress ? (
    <MinerLink
      address={block.minerAddress}
      pool={block.minerPool}
      unnamed={
        <Address address={block.minerAddress} startChars={10} endChars={6} className="text-xs" />
      }
    />
  ) : (
    <span className="text-text-dim">Genesis</span>
  );
}

/** "4 txs · 3 proposals · 1 uncle": the block's contents in words (the glyph's facts). */
function contents(block: Block): string {
  return [
    formatCount(block.transactionSizes.length, 'tx', 'txs'),
    ...(block.proposalsCount > 0 ? [formatCount(block.proposalsCount, 'proposal')] : []),
    ...(block.unclesCount > 0 ? [formatCount(block.unclesCount, 'uncle')] : []),
  ].join(' · ');
}

function fees(block: Block, unit: boolean) {
  return (
    <ExactCkb
      shannons={parseShannons(block.totalFees, `block ${block.number} fees`)}
      signed={false}
      tone="plain"
      unit={unit}
    />
  );
}

function cyclesText(block: Block): string {
  if (block.cycles === null)
    return block.cyclesStatus === 'failed' ? 'cycles replay failed' : 'cycles not yet run';
  return `${BigInt(block.cycles).toLocaleString('en-US')} cycles`;
}

/** "3,344 B, 640 B overhead": the size bar's two parts in figures. */
function sizeText(block: Block): string {
  return `${block.size.toLocaleString('en-US')} B, ${block.overheadSize.toLocaleString('en-US')} B overhead`;
}

function reward(block: Block) {
  if (block.miningReward !== null)
    return (
      <span className="whitespace-nowrap">
        reward{' '}
        <ExactCkb
          shannons={parseShannons(block.miningReward, `block ${block.number} reward`)}
          signed={false}
          tone="plain"
        />
      </span>
    );
  if (block.rewardFinalizesAt === null) return 'Genesis';
  return (
    <span className="whitespace-nowrap">
      reward pays at{' '}
      <Link href={`/blocks/${block.rewardFinalizesAt}`}>
        {formatBlockNumber(block.rewardFinalizesAt)}
      </Link>
    </span>
  );
}

/**
 * A block on a phone (board L1): the height with its contents and age, who mined it and the fees,
 * then the quiet figures — hash, size, cycles and when the reward is paid.
 */
function BlockPhoneRow({ block }: { block: Block }) {
  return (
    <PhoneRow
      testId="block-row-mobile"
      lines={[
        {
          key: 'identity',
          tone: 'head',
          left: (
            <Link href={`/blocks/${block.number}`} className="text-text-bright font-medium">
              {formatBlockNumber(block.number)}
            </Link>
          ),
          right: `${contents(block)} · ${formatTimeAgo(block.timestamp)}`,
        },
        {
          key: 'miner',
          tone: 'body',
          left: (
            <>
              <span className="text-text-dim">miner </span>
              {miner(block)}
            </>
          ),
          right: <span className="whitespace-nowrap">fees {fees(block, true)}</span>,
        },
        {
          key: 'figures',
          tone: 'quiet',
          left: (
            <Dotted
              parts={[
                <Hash
                  key="h"
                  hash={block.hash}
                  startChars={6}
                  endChars={4}
                  copyable={false}
                  className="text-[11px]"
                />,
                block.hardforkActivation && (
                  <Link key="hf" href="/hardforks">
                    HF · {block.hardforkActivation.shortName.toUpperCase()}
                  </Link>
                ),
                <span key="s" className="whitespace-nowrap">
                  {sizeText(block)}
                </span>,
                <span key="c" className="whitespace-nowrap">
                  {cyclesText(block)}
                </span>,
                reward(block),
              ]}
            />
          ),
        },
      ]}
    />
  );
}

/**
 * Board L1's 640 table: six columns in 608 px. The reward is not a column (it is paid eleven
 * blocks later); the hash and a hardfork activation are the height cell's second line.
 */
const NARROW_COLUMNS: ListColumn<Block>[] = [
  {
    key: 'height',
    label: 'Height',
    minWidth: 112,
    render: (block) => (
      <>
        <Link href={`/blocks/${block.number}`} className="text-text-bright hover:text-jade">
          {formatBlockNumber(block.number)}
        </Link>
        <Hash
          hash={block.hash}
          startChars={6}
          endChars={4}
          copyable={false}
          className="text-text-dim block whitespace-nowrap text-[11px]"
        />
        {block.hardforkActivation && (
          <Link href="/hardforks" className="text-text-dim block text-[11px]">
            HF · {block.hardforkActivation.shortName.toUpperCase()}
          </Link>
        )}
      </>
    ),
  },
  {
    key: 'age',
    label: 'Age',
    minWidth: 76,
    className: 'whitespace-nowrap',
    render: (block) => <span className="text-text-dim">{formatTimeAgo(block.timestamp)}</span>,
  },
  {
    key: 'txs',
    label: 'Txs',
    minWidth: 48,
    numeric: true,
    render: (block) => (
      <>
        {block.transactionSizes.length.toLocaleString('en-US')}
        {block.proposalsCount > 0 && (
          <span className="text-text-dim block text-[11px]">{block.proposalsCount}p</span>
        )}
        {block.unclesCount > 0 && (
          <span className="text-text-dim block text-[11px]">{block.unclesCount}u</span>
        )}
      </>
    ),
  },
  {
    key: 'miner',
    label: 'Miner',
    minWidth: 156,
    className: 'whitespace-nowrap',
    render: miner,
  },
  {
    key: 'fees',
    label: 'Fees, CKB',
    minWidth: 96,
    numeric: true,
    render: (block) => fees(block, false),
  },
  {
    key: 'resources',
    label: 'Size · cycles',
    minWidth: 120,
    numeric: true,
    className: '!whitespace-normal text-text-dim',
    render: (block) => (
      <>
        <span className="block whitespace-nowrap">{block.size.toLocaleString('en-US')} B</span>
        <span className="block whitespace-nowrap">
          {block.cycles === null
            ? block.cyclesStatus === 'failed'
              ? 'replay failed'
              : 'not yet run'
            : BigInt(block.cycles).toLocaleString('en-US')}
        </span>
      </>
    ),
  },
];
