'use client';

import { TopicCharts } from '@/components/charts/topic-charts';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import Link from '@/components/ui/link';
import { api, type Block } from '@/lib/api';
import { DetailLayout } from '@/components/ui/detail-layout';
import { ListTable, type ListColumn, type ListTableForm } from '@/components/ui/list-table';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import { Hash } from '@/components/ui/hash';
import { ROW_SEAL, RowSeal } from '@/components/ui/row-seal';
import type { SealId } from '@/lib/seal/stones';
import { Address } from '@/components/ui/address';
import { MinerLink } from '@/components/forks/miner-link';
import { CkbAmount } from '@/components/ui/ckb-amount';
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
import { usePageSize } from '@/hooks/usePageSize';
import { parseShannons } from '@/lib/ckb-decimal';
import { Utc8Age } from '@/components/ui/utc8-date';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import type { ShortSize } from '@/lib/shorten';
import { useState } from 'react';
import { unitNode } from '@/components/ui/units';

export default function BlocksPage() {
  const { cursor, hasPrevious, page, goToNext, goToPrevious } = useCursorPagination();
  const limit = usePageSize(DEFAULT_PAGE_SIZE);
  // PA-02: the legend explains the wide table's Contents glyphs and size bar, which the 608 px
  // table and the phone rows write in figures; it is drawn with the form the list chose.
  const [form, setForm] = useState<ListTableForm | null>(null);
  const read = useQuery({
    queryKey: ['blocks', cursor, limit],
    queryFn: () => api.getBlocks({ cursor, limit }),
    staleTime: 30_000,
    placeholderData: keepPreviousData,
    refetchInterval: (query) =>
      query.state.data?.data.some((block) => block.cyclesStatus === 'pending') ? 3000 : false,
  });
  const { data, error } = read;
  const sizeScale = resourceScale(data ? data.data.map((block) => block.size) : []);
  const cyclesScale = resourceScale(
    data ? data.data.flatMap((block) => (block.cycles === null ? [] : [Number(block.cycles)])) : []
  );
  const columns: ListColumn<Block>[] = [
    {
      key: 'block',
      label: 'Block',
      // The block's stone and its gap (26 + 8, ROW_SEAL.block.footprint), "#20,879,198" (11
      // characters at 13 px, 85.8 px), a 12 px space (7.2 px) and "0xd161…8fef" (11 at 11 px,
      // 72.6 px, the tight size: the hash is the key cell's second token, beside the block
      // number, and the row size's 17 characters, 112.2 px, would need 256 here and a table of
      // 1,257 px, wider than 1,232 at 1280): 199.6 px and the 16 px gap, 215.6 (was 228.8 with
      // "0xd161...8fef" at 13 characters, 200 without a seal, 222 with the old hash seal's 27).
      minWidth: 195 + ROW_SEAL.block.footprint,
      render: (block) => <BlockIdentity block={block} />,
    },
    {
      key: 'age',
      label: 'Age',
      minWidth: 70,
      className: 'whitespace-nowrap',
      // the age reads the page's one clock (the review's item H), the date-time its hover
      render: (block) => <Utc8Age timestamp={block.timestamp} className="text-text-dim" />,
    },
    {
      key: 'contents',
      label: 'Contents',
      minWidth: 170,
      // T2: a flex box, so the glyph's 18 px inline box does not sit on the text baseline and
      // stretch the line to 23 px (a 48 px row); the row stays one 18 px line.
      render: (block) => (
        <div className="flex">
          <BlockContents
            transactionSizes={block.transactionSizes}
            proposalsCount={block.proposalsCount}
            unclesCount={block.unclesCount}
          />
        </div>
      ),
    },
    {
      key: 'miner',
      label: 'Miner',
      minWidth: 160,
      className: 'whitespace-nowrap',
      render: (block) => miner(block, 'row'),
    },
    {
      key: 'reward',
      label: 'Reward, CKB',
      // A reward of four integer digits, "1,917.80821917" (14 characters at 12 px, 100.8 px), and
      // the 16 px gap: the column gave the row seal's 22 px from its 140 so the table keeps its
      // 1,230 px minimum (the widest container is 1,232).
      minWidth: 118,
      numeric: true,
      render: (block) =>
        block.miningReward === null ? (
          <span className="text-text-dim text-meta">
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
          <CkbAmount
            form="column"
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
      // Fees under 1,000 CKB, "999.99999999" (12 characters at 12 px, 86.4 px), and the 16 px
      // gap: 102.4 px. Every amount has its eight decimals, so no fee is 13 characters and the
      // next is 14 ("1,000.00000000", 116.8 px), which 112 did not hold either: the column gave
      // the block stone's 7 px more than the old hash seal's from its 112 and keeps 2.6 to spare,
      // so the table keeps its 1,230 px minimum (the widest container is 1,232).
      minWidth: 105,
      numeric: true,
      render: (block) => (
        <CkbAmount
          form="column"
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
    <div className="min-h-screen">
      <main className="container mx-auto min-w-0 py-8">
        <DetailLayout
          title="Blocks"
          kind="index"
          actions={form === 'table' ? <BlocksLegend /> : undefined}
        >
          {isAwaited(read) && (
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
                onForm={setForm}
              />
              <BlocksLimitsFootnote blocks={data.data} />
              <CursorPagination
                summary={
                  <span className="text-small">{blocksRange(data.data, data.total, page)}</span>
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
        <TopicCharts topic="/blocks" />
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
      className="text-text-dim text-small flex flex-wrap items-center gap-x-[18px] gap-y-1.5"
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
        <span aria-hidden="true" className="flex h-2 w-[18px] overflow-hidden">
          <span className="w-2" style={{ backgroundColor: CLASS_COLORS.ckb }} />
          <span className="flex-1" style={{ backgroundColor: DATA_COLOR }} />
        </span>
        block overhead · user transactions
      </span>
    </div>
  );
}

/**
 * A block's seal (seals by kind, §0): the stone of its hash, carrying how many transactions it
 * holds — `transactionsCount` as the API serves it on the list and the block page. Nothing is
 * filled in: a count that is missing or not a whole number is refused by the seal, naming the
 * block's hash.
 */
function blockSeal(block: Block): SealId {
  return { kind: 'block', hash: block.hash, txs: block.transactionsCount };
}

/**
 * The row seal, then "#20,879,198 0xd161…8fef" on one line; a hardfork activation below it. The
 * number is the row's one bright thing at body (table-rows T5), on an 18 px line so the row keeps
 * its height (the 26 px stone overhangs that line by 4 px above and below, RowSeal).
 */
function BlockIdentity({ block }: { block: Block }) {
  return (
    <div className="flex items-start">
      <RowSeal id={blockSeal(block)} overhang />
      <div className="min-w-0 space-y-1">
        <div className="whitespace-nowrap">
          <Link href={`/blocks/${block.number}`} className={KEY}>
            {formatBlockNumber(block.number)}
          </Link>{' '}
          <Hash
            hash={block.hash}
            size="tight"
            copyable={false}
            className="text-text-dim text-meta"
          />
        </div>
        {block.hardforkActivation && (
          <Link href="/hardforks" className="text-text-dim text-meta block">
            HF · {block.hardforkActivation.shortName.toUpperCase()}
          </Link>
        )}
      </div>
    </div>
  );
}

/** The key cell of a block row (table-rows T5): 13 px, bright, on the row's 18 px line. */
const KEY = 'text-body text-text-bright leading-[18px] hover:text-jade';

/**
 * Who mined the block, linked to its address (the full address its title): the pool `pools.toml`
 * names, else the address at the form's size (U-09): `ckb1qzda0c…93hhsj` in the tables (row),
 * `ckb1…93hhsj` in the phone row (tight). Genesis is mined by no one.
 */
function miner(block: Block, size: ShortSize) {
  if (!block.minerAddress) return <span className="text-text-dim">Genesis</span>;
  return (
    <MinerLink
      address={block.minerAddress}
      pool={block.minerPool}
      // T5: the block number is the row's one bright thing; a named pool reads in body ink.
      size={size}
      className="text-text"
      unnamed={<Address address={block.minerAddress} size={size} className="text-small" />}
    />
  );
}

/** A block's fees: a column in the table, a summary on the phone row with its unit (R5). */
function fees(block: Block, unit: boolean) {
  return (
    <CkbAmount
      form={unit ? 'summary' : 'column'}
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
        <CkbAmount
          form="summary"
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
 * A block on a phone (board L1): its seal leading the row, the height with its transactions and
 * age, who mined it and the fees, then the quiet figures — hash, proposals and uncles, size,
 * cycles and when the reward is paid. What every reader comes for leads; the consensus detail
 * (the two-step commit's proposals, the orphan rate's uncles) stands with the developer figures,
 * after the hash that starts that line on every row, each count one unbroken unit and only when
 * non-zero.
 *
 * The head line never wraps: the 13 px height, "#20,987,284" (11 characters, 85.8 px), and the
 * 12 px gap leave 196.2 px at 360 and 226.2 at 390 for the right part, 27 and 31 characters at
 * 12 px; "999 txs · 59m ago" is 17 (122.4 px, 220.2 in all). Only an age that is not an age, "this
 * device's clock is behind" (29 characters), drops it under the height, whole. With the counts it
 * read "4 txs · 1 proposal · 13s ago" (28) and dropped at 360 while its neighbours did not.
 *
 * The miner line keeps one shape at 390 and 360 px (text columns of 324 and 294 px beside the
 * stone: 390 and 360 less the two 16 px gutters and the block stone's 34; 12 px, 7.2 px a
 * character, the unit 11 px, 6.6): "miner ckb1…93hhsj" (17 characters, 122.4 px, the tight address
 * at 4…6), the 12 px gap and the widest fee below 10 CKB, "fees 9.99999999" (15, 108) and " CKB"
 * (26.4), make 268.8 px, 25.2 to spare at 360; a fee of 100–999 CKB makes 283.2. At 8…6, "miner
 * ckb1qzda…93hhsj" (151.2 px), it was 297.6 and the fee dropped under the miner at 360. A fee from
 * 1,000 CKB at 360 (the separator: 297.6), from 1,000,000 CKB at 390, or a long pool name drops the
 * fees under the miner whole, right-aligned (PhoneRow's rule).
 */
function BlockPhoneRow({ block }: { block: Block }) {
  return (
    <PhoneRow
      testId="block-row-mobile"
      mark={<RowSeal id={blockSeal(block)} />}
      lines={[
        {
          key: 'identity',
          tone: 'head',
          left: (
            <Link href={`/blocks/${block.number}`} className="text-text-bright font-medium">
              {formatBlockNumber(block.number)}
            </Link>
          ),
          right: (
            <>
              {unitNode(formatCount(block.transactionSizes.length, 'tx', 'txs'))} ·{' '}
              <Utc8Age timestamp={block.timestamp} />
            </>
          ),
        },
        {
          key: 'miner',
          tone: 'body',
          left: (
            <>
              <span className="text-text-dim">miner </span>
              {miner(block, 'tight')}
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
                  size="tight"
                  copyable={false}
                  className="text-meta"
                />,
                block.proposalsCount > 0 && (
                  <span key="p" className="whitespace-nowrap">
                    {formatCount(block.proposalsCount, 'proposal')}
                  </span>
                ),
                block.unclesCount > 0 && (
                  <span key="u" className="whitespace-nowrap">
                    {formatCount(block.unclesCount, 'uncle')}
                  </span>
                ),
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
    // The block's stone and its gap (26 + 8) beside the two lines ("#20,879,198" at 13 px, 85.8
    // px, and the hash at 11 px at the tight size, 72.6 px) and the 16 px gap: 135.8 px, with 2.2
    // px to spare (the number
    // may break between characters, and with the old hash seal at 0.2 px to spare the cell's
    // sub-pixel width broke it); it was 112 without a seal, 131 with the old hash seal's 27.
    minWidth: 104 + ROW_SEAL.block.footprint,
    render: (block) => (
      <div className="flex items-start">
        <RowSeal id={blockSeal(block)} />
        <div className="min-w-0">
          <Link href={`/blocks/${block.number}`} className={KEY}>
            {formatBlockNumber(block.number)}
          </Link>
          <Hash
            hash={block.hash}
            size="tight"
            copyable={false}
            className="text-text-dim text-meta block whitespace-nowrap"
          />
          {block.hardforkActivation && (
            <Link href="/hardforks" className="text-text-dim text-meta block">
              HF · {block.hardforkActivation.shortName.toUpperCase()}
            </Link>
          )}
        </div>
      </div>
    ),
  },
  {
    key: 'age',
    label: 'Age',
    minWidth: 76,
    className: 'whitespace-nowrap',
    render: (block) => <Utc8Age timestamp={block.timestamp} className="text-text-dim" />,
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
          <span className="text-text-dim text-meta block">{block.proposalsCount}p</span>
        )}
        {block.unclesCount > 0 && (
          <span className="text-text-dim text-meta block">{block.unclesCount}u</span>
        )}
      </>
    ),
  },
  {
    key: 'miner',
    label: 'Miner',
    // An unnamed miner, "ckb1qzda0c…93hhsj" (17 characters at 12 px, 122.4 px; it was 19 with
    // three dots), and the 16 px gap: 138.4 px, 15.6 to spare. A pool's name may be as long here
    // as at 156 (19 characters: 138 px holds 19.2), so the column gave 2 of the block stone's 7 px
    // from its 156.
    minWidth: 154,
    className: 'whitespace-nowrap',
    render: (block) => miner(block, 'row'),
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
    // The most cycles a block may take, "3,500,000,000" (13 characters at 12 px, 93.6 px; "replay
    // failed" is as long), and no gap (the last column): 96 keeps 2.4 to spare. It gave the old
    // hash seal's 19 px from its 120, then 5 of the block stone's 7, so the table keeps 608 px.
    minWidth: 96,
    // The column that takes the free width (the board's 1fr).
    width: 'auto',
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
