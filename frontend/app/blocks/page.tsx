'use client';

import { TopicCharts } from '@/components/charts/topic-charts';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import Link from '@/components/ui/link';
import { api, type Block } from '@/lib/api';
import { DetailLayout } from '@/components/ui/detail-layout';
import {
  CELL_GAP_PX,
  ListTable,
  type ListColumn,
  type ListTableForm,
} from '@/components/ui/list-table';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import { Hash } from '@/components/ui/hash';
import { ROW_SEAL, RowSeal } from '@/components/ui/row-seal';
import type { SealId } from '@/lib/seal/stones';
import { Address } from '@/components/ui/address';
import { MinerLink } from '@/components/forks/miner-link';
import { CkbAmount } from '@/components/ui/ckb-amount';
import {
  BlockContents,
  BlocksLimitsNote,
  CONTENTS_GLYPH,
  ResourceBar,
  ScaleHeader,
  resourceScale,
} from '@/components/blocks/block-visual';
import { CLASS_COLORS, DATA_COLOR } from '@/lib/design-tokens';
import { Pager } from '@/components/ui/pager';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { DEFAULT_PAGE_SIZE, timePageAt } from '@/lib/pagination';
import { usePageSize } from '@/hooks/usePageSize';
import { parseShannons } from '@/lib/ckb-decimal';
import { Utc8Age } from '@/components/ui/utc8-date';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import { useState } from 'react';
import { unitNode } from '@/components/ui/units';
import { charPx, columnPx, fontPx, textPx } from '@/lib/type-metrics';
import { BAR_FIGURE_GAP, COUNT_TRACK_PX } from '@/components/ui/aligned-bar';
import { SHORT_FORMS, type ShortSize } from '@/lib/shorten';
import {
  GENESIS_BLOCK_BYTES,
  MAX_BLOCK_CYCLES,
  WIDEST_AGE,
  WIDEST_BLOCK_FEES,
  WIDEST_BLOCK_NUMBER,
  WIDEST_BLOCK_REWARD,
  WIDEST_COUNT,
  WIDEST_PROPOSALS,
  shortChars,
} from '@/lib/list-bounds';

/**
 * The widest scale each bar column's figure writes (readability F3-e): genesis's 1,236,271 bytes,
 * the one block over the 597,000-byte limit, on a 1,500,000 scale; the 3,500,000,000-cycle limit
 * on a 4,000,000,000 one. Their heads write the scale in the tick form ("0 … 1.5M", "0 … 4B"), so
 * each bar's track is COUNT_TRACK_PX, 48 px (the coordinator's ruling: the rule at `trackPx`).
 */
const SIZE_SCALE = resourceScale([GENESIS_BLOCK_BYTES]).toLocaleString('en-US');
const CYCLES_SCALE = resourceScale([MAX_BLOCK_CYCLES]).toLocaleString('en-US');
const SIZE_TRACK = COUNT_TRACK_PX;
const CYCLES_TRACK = COUNT_TRACK_PX;

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
  // where the list stands in time (lib/pagination.ts): its page and the pages its served total
  // makes at the served page size; nothing for a list on one page, an empty one too
  const blocksAt = data ? timePageAt('blocks', page, data) : null;
  const sizeScale = resourceScale(data ? data.data.map((block) => block.size) : []);
  const cyclesScale = resourceScale(
    data ? data.data.flatMap((block) => (block.cycles === null ? [] : [Number(block.cycles)])) : []
  );
  const columns: ListColumn<Block>[] = [
    {
      key: 'block',
      label: 'Block',
      // The block's stone and its gap (ROW_SEAL.block.footprint, 24), the widest height,
      // "#99,999,999" at the key's 14 px (92.4), a space at 13 px (7.8) and the hash at its tight
      // size, "0xd161…8fef" at 12 px (79.2: the hash is the key cell's second token, beside the
      // number), and the gap: 219.4. Its 13 px spare went (readability F3-e): it stood for no
      // content, and a glyph advances exactly 0.6 em.
      minWidth: columnPx(
        ROW_SEAL.block.footprint +
          textPx(WIDEST_BLOCK_NUMBER.length, fontPx('body')) +
          charPx('small') +
          textPx(shortChars(SHORT_FORMS.tight.hash), fontPx('meta')) +
          CELL_GAP_PX
      ),
      render: (block) => <BlockIdentity block={block} />,
    },
    {
      key: 'age',
      label: 'Age',
      // The widest age, "10y 364d ago" (12 characters at 13 px, 93.6), and the gap: it was 70,
      // which held "59m ago" and not an older page's "6y 363d ago".
      minWidth: columnPx(textPx(WIDEST_AGE.length, fontPx('small')) + CELL_GAP_PX),
      // the age reads the page's one clock (the review's item H), the date-time its hover; an age
      // is one unit (Utc8Age), "this device's clock is behind" words that break in the column
      render: (block) => <Utc8Age timestamp={block.timestamp} className="text-text-dim" />,
    },
    {
      key: 'contents',
      label: 'Contents',
      // The glyph (112 px), its 8 px gap and the widest count, "9,999 tx" at 12 px (57.6), and the
      // gap: 193.6. It was 170, which held a count of two digits. A block with more than seven
      // user transactions or more proposals than dots says so on a second line.
      minWidth: columnPx(
        CONTENTS_GLYPH.width +
          CONTENTS_GLYPH.gap +
          textPx(`${WIDEST_COUNT} tx`.length, fontPx('meta')) +
          CELL_GAP_PX
      ),
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
      // The table's flexible column: an unnamed miner whole at the row size, "ckb1…4s93hhsj" (13
      // characters at 13 px, 101.4), and the gap; it takes what the container gives beyond the sum.
      // A pool name, as long as pools.toml writes it, ends in the one ellipsis there; the link's
      // title is the miner's address. It was 160 and named no content.
      minWidth: columnPx(
        textPx(shortChars(SHORT_FORMS.row.address), fontPx('small')) + CELL_GAP_PX
      ),
      width: 'auto',
      className: 'whitespace-nowrap truncate',
      render: (block) => miner(block, 'row'),
    },
    {
      key: 'reward',
      label: 'Reward, CKB',
      // A reward of four whole digits, "9,999.99999999" (14 characters at 13 px, 109.2: the first
      // epochs paid 1,917 CKB), and the gap. Its 1 px spare went (F3-e).
      minWidth: columnPx(textPx(WIDEST_BLOCK_REWARD.length, fontPx('small')) + CELL_GAP_PX),
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
      // Fees under 1,000 CKB, "999.99999999" (12 characters at 13 px, 93.6), and the gap. Every
      // amount has its eight decimals, so no fee is 13 characters. Its 2 px spare went (F3-e).
      minWidth: columnPx(textPx(WIDEST_BLOCK_FEES.length, fontPx('small')) + CELL_GAP_PX),
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
      header: <ScaleHeader label="Size, bytes" scale={sizeScale} trackWidth={SIZE_TRACK} />,
      // The 48 px track (its head's ticks in the tick form, "0 … 1.5M"), the bar's 10 px gap, the
      // scale's figure ("1,500,000": genesis's 1,236,271 bytes; 9 characters at 13 px, 70.2) and
      // the gap: 144.2. It was 182 on a 120 px track and held a scale of four characters.
      minWidth: columnPx(
        SIZE_TRACK + BAR_FIGURE_GAP + textPx(SIZE_SCALE.length, fontPx('small')) + CELL_GAP_PX
      ),
      className: 'whitespace-nowrap',
      render: (block) => (
        <ResourceBar
          label="Size, bytes"
          value={block.size}
          overhead={block.overheadSize}
          scale={sizeScale}
          trackWidth={SIZE_TRACK}
        />
      ),
    },
    {
      key: 'cycles',
      label: 'Cycles',
      header: <ScaleHeader label="Cycles" scale={cyclesScale} trackWidth={CYCLES_TRACK} />,
      // The 48 px track (its head's ticks in the tick form, "0 … 4B": the 3,500,000,000-cycle
      // limit), the 10 px gap and the scale's figure ("4,000,000,000", 13 characters at 13 px,
      // 101.4); the last column, no gap after it: 159.4. It was 196 on a 120 px track and held a
      // scale of six characters.
      minWidth: columnPx(
        CYCLES_TRACK + BAR_FIGURE_GAP + textPx(CYCLES_SCALE.length, fontPx('small'))
      ),
      className: 'whitespace-nowrap',
      render: (block) =>
        block.cycles === null ? (
          <span className="text-text-dim">
            {block.cyclesStatus === 'failed' ? 'Replay failed' : 'Not yet run'}
          </span>
        ) : (
          <ResourceBar
            label="Cycles"
            value={Number(block.cycles)}
            scale={cyclesScale}
            trackWidth={CYCLES_TRACK}
          />
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
            <p role="alert" className="text-text-dim max-w-measure">
              Unable to read blocks: {error.message}
            </p>
          )}
          {data && (
            <>
              {/* B5: the limits that give the Size and Cycles bars their meaning, above them. */}
              <BlocksLimitsNote blocks={data.data} />
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
              {/* The controls board's cursor in time: newest first, the page of the pages the
                  served total makes at the served page size. A list with no row has no page to
                  be on, and draws none. */}
              {blocksAt && (
                <Pager
                  mode="time"
                  label="blocks"
                  order="newest first"
                  {...blocksAt}
                  onNewer={hasPrevious ? goToPrevious : undefined}
                  onOlder={data.hasMore ? () => goToNext(data.nextCursor) : undefined}
                />
              )}
            </>
          )}
        </DetailLayout>
        <TopicCharts topic="/blocks" />
      </main>
    </div>
  );
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
 * number is the row's one bright thing at body (table-rows T5), on the 20 px row line, and the
 * 16 px stone stands centred on that line, 2 px clear above and below (RowSeal), so the row keeps
 * its height.
 */
function BlockIdentity({ block }: { block: Block }) {
  return (
    <div className="flex items-start">
      <RowSeal id={blockSeal(block)} />
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
const KEY = 'text-body text-text-bright leading-row hover:text-jade';

/**
 * Who mined the block, linked to its address (the full address its title): the pool `pools.toml`
 * names, else the address at the form's size (U-09): `ckb1…4s93hhsj` in the tables (row),
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
 * 12 px gap leave 206.2 px at 360 and 236.2 at 390 for the right part, 28 and 32 characters at
 * 12 px; "999 txs · 59m ago" is 17 (122.4 px, 220.2 in all). Only an age that is not an age, "this
 * device's clock is behind" (29 characters), drops it under the height, whole. With the counts it
 * read "4 txs · 1 proposal · 13s ago" (28) and dropped at 360 while its neighbours did not.
 *
 * The miner line keeps one shape at 390 and 360 px (text columns of 334 and 304 px beside the
 * stone: 390 and 360 less the two 16 px gutters and the block stone's 24; 12 px, 7.2 px a
 * character, the unit 11 px, 6.6): "miner ckb1…93hhsj" (17 characters, 122.4 px, the tight address
 * at 4…6), the 12 px gap and the widest fee below 10 CKB, "fees 9.99999999" (15, 108) and " CKB"
 * (26.4), make 268.8 px, 35.2 to spare at 360; a fee of 100–999 CKB makes 283.2. Beside the 26 px
 * stone (294 px at 360, 324 at 390) the 8…6 form, "miner ckb1qzda…93hhsj" (151.2 px), made it 297.6
 * and the fee dropped under the miner at 360, as a fee from 1,000 CKB did there (the separator:
 * 297.6) and one from 1,000,000 CKB at 390. A long pool name drops the fees under the miner whole,
 * right-aligned (PhoneRow's rule).
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
 * Board L1's 640 table, five columns in 608 px (readability F3-e, the coordinator's ruling): every
 * column at its honest budget, and the transaction count is the age's second line, as the hash is
 * the height's and the cycles the size's, so each row stays two lines. The reward is not a column
 * (it is paid eleven blocks later). 585 px: a table from a 585 px container, rows below.
 */
export const NARROW_COLUMNS: ListColumn<Block>[] = [
  {
    key: 'height',
    label: 'Height',
    // The block's stone and its gap (ROW_SEAL.block.footprint, 24) beside its two lines, the
    // widest height, "#99,999,999" at the key's 14 px (92.4), over the tight hash at 12 px (79.2),
    // and the gap: 132.4. Its 2 px spare went: a glyph advances exactly 0.6 em. A hardfork
    // activation is a third line, on its one block. The stone stands centred on the height's line
    // (RowSeal).
    minWidth: columnPx(
      ROW_SEAL.block.footprint +
        Math.max(
          textPx(WIDEST_BLOCK_NUMBER.length, fontPx('body')),
          textPx(shortChars(SHORT_FORMS.tight.hash), fontPx('meta'))
        ) +
        CELL_GAP_PX
    ),
    render: (block) => (
      <div className="flex items-start">
        <RowSeal id={blockSeal(block)} />
        <div className="min-w-0">
          {/* a block number is one figure: no table cell breaks inside it (ListTable) */}
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
    label: 'Age · txs',
    // Two data of one cell on two lines: the age, "10y 364d ago" (12 characters at 13 px, 93.6),
    // over the transaction count and the proposals, "9,999 · 1,500p" (the count and the separator
    // at 13 px, 62.4; the proposals at 12, up to the 1,500 a block may carry, 43.2): 105.6, the
    // wider, and the gap. A block's uncles ("· 1u") join that line, and stand under it only where
    // the count and the proposals already fill it. Age was 76 (it held "59m ago") and Txs 48.
    minWidth: columnPx(
      Math.max(
        textPx(WIDEST_AGE.length, fontPx('small')),
        textPx(`${WIDEST_COUNT} · `.length, fontPx('small')) +
          textPx(`${WIDEST_PROPOSALS}p`.length, fontPx('meta'))
      ) + CELL_GAP_PX
    ),
    render: (block) => (
      <span className="block">
        <span className="block">
          <Utc8Age timestamp={block.timestamp} className="text-text-dim" />
        </span>
        <span className="block">
          <Dotted
            parts={[
              block.transactionSizes.length.toLocaleString('en-US'),
              block.proposalsCount > 0 && (
                <span key="p" className="text-text-dim text-meta">
                  {block.proposalsCount.toLocaleString('en-US')}p
                </span>
              ),
              block.unclesCount > 0 && (
                <span key="u" className="text-text-dim text-meta">
                  {block.unclesCount}u
                </span>
              ),
            ]}
          />
        </span>
      </span>
    ),
  },
  {
    key: 'miner',
    label: 'Miner',
    // The table's flexible column: an unnamed miner whole at the row size, "ckb1…4s93hhsj" (13
    // characters at 13 px, 101.4), and the gap; it takes what the container gives beyond the sum
    // (Size · cycles did), and a pool name longer than that ends in the one ellipsis.
    minWidth: columnPx(textPx(shortChars(SHORT_FORMS.row.address), fontPx('small')) + CELL_GAP_PX),
    width: 'auto',
    className: 'whitespace-nowrap truncate',
    render: (block) => miner(block, 'row'),
  },
  {
    key: 'fees',
    label: 'Fees, CKB',
    // Fees under 1,000 CKB, "999.99999999" (12 characters at 13 px, 93.6), and the gap, as the
    // wide table's. It was 96, which held fees under 10 CKB.
    minWidth: columnPx(textPx(WIDEST_BLOCK_FEES.length, fontPx('small')) + CELL_GAP_PX),
    numeric: true,
    render: (block) => fees(block, false),
  },
  {
    key: 'resources',
    label: 'Size · cycles',
    // The size over the cycles: the most cycles a block may take, "3,500,000,000" (13 characters
    // at 13 px, 101.4; "replay failed" is as long, genesis's "1,236,271 B" narrower), and no gap
    // (the last column). Its 2 px spare went.
    minWidth: columnPx(
      textPx(
        Math.max(
          MAX_BLOCK_CYCLES.toLocaleString('en-US').length,
          `${GENESIS_BLOCK_BYTES.toLocaleString('en-US')} B`.length
        ),
        fontPx('small')
      )
    ),
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
