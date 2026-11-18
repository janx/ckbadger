'use client';
import type { ReactNode } from 'react';
import { useQueries } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { BlockLink } from '@/components/ui/block-link';
import { CELL_GAP_PX, ListTable, type ListColumn } from '@/components/ui/list-table';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import { PromptHeading } from '@/components/ui/prompt-heading';
import { api, type Cell, type ScriptFamilyDetail } from '@/lib/api';
import { cellInventoryItem, type CellInventoryItem } from '@/lib/cell-inventory';
import { readErrorText } from '@/lib/read-error';
import { retryTransientOnly } from '@/lib/query-client';
import { scriptActivityLink } from '@/lib/script-activity-link';
import { NEWEST_CELLS_COUNT, type NewestCellsKind } from '@/lib/script-newest-cells';
import { formatBlockNumber } from '@/lib/format-block';
import { formatUtc8Date } from '@/lib/utils';
import { formatCount } from '@/lib/format-count';
import { ShortId } from '@/components/ui/short-id';
import { OutpointIndex } from '@/components/ui/outpoint-index';
import { SHORT_FORMS, shortenHash, type ShortSize } from '@/lib/shorten';
import { columnPx, fontPx, textPx } from '@/lib/type-metrics';
import { WIDEST_BLOCK_NUMBER, WIDEST_COUNT_DIGITS, shortChars } from '@/lib/list-bounds';
import { Utc8Date } from '@/components/ui/utc8-date';
import { unitNode } from '@/components/ui/units';

export interface NewestCellsState {
  /** The index the list reads (ruling 2); null when the family has no lock or type role. */
  kind: NewestCellsKind | null;
  rows: readonly Cell[];
  loading: boolean;
  error: Error | null;
}

const IDENTITY_WORD = { dotbit: '.bit', 'bit-cell': '.bit Cell', did: 'did:ckb', dotcell: '.cell' };

/**
 * The words lane U's inventory naming gives the item: its pending statement on the cell page (the
 * class and the short id at the row's size; no further name is read per row), or the name the
 * cell's data carries.
 */
function itemWords(item: CellInventoryItem, size: ShortSize): string {
  const id = (value: string) => shortenHash(value, size);
  switch (item.kind) {
    case 'spore':
      return `Spore object ${id(item.id)}`;
    case 'spore-cluster':
      return `Spore cluster ${id(item.id)}`;
    case 'm-nft':
      return `m-NFT object ${id(item.id)}`;
    case 'm-nft-class':
      return `m-NFT class ${id(item.id)}`;
    case 'token':
      return `${item.amount} ${item.unit}`;
    case 'token-without-amount':
      return `${item.unit} · no readable amount`;
    case 'identity':
      return item.name ?? `${IDENTITY_WORD[item.standard]} identity ${id(item.id)}`;
    case 'dao':
      return item.stage === 'deposit' ? 'Nervos DAO deposit' : 'Nervos DAO withdrawal request';
  }
}

/** The full id an item's words shorten; none for a DAO cell, whose words shorten nothing. */
function itemId(item: CellInventoryItem): string | undefined {
  switch (item.kind) {
    case 'token':
    case 'token-without-amount':
      return item.typeHash;
    case 'dao':
      return undefined;
    default:
      return item.id;
  }
}

type What =
  | { state: 'reading' }
  | { state: 'failed'; message: string }
  /** `title`: the full id the shortened words stand for (lane L: hover titles keep the full id). */
  | { state: 'item'; text: string; href: string; title?: string }
  | { state: 'typed'; text: string; href: string; title: string }
  | { state: 'plain' };

/**
 * What the cell is: the inventory item it is (lane U's `cellInventoryItem`, read from the cell's
 * own detail), else its type script by registry name (R6), else a plain cell. Its ids at `size`,
 * the size of the form the row is drawn in (U-09).
 */
function whatOf({ cell: row, detail, error }: Row, size: ShortSize): What {
  if (error)
    return { state: 'failed', message: `could not read the cell: ${readErrorText(error)}` };
  if (!detail) return { state: 'reading' };
  let item: CellInventoryItem | null;
  try {
    item = cellInventoryItem(detail, size);
  } catch (failure) {
    return { state: 'failed', message: (failure as Error).message };
  }
  if (item)
    return { state: 'item', text: itemWords(item, size), href: item.href, title: itemId(item) };
  if (!row.typeScriptHash) return { state: 'plain' };
  if (!row.typeCodeHash || !row.typeHashType)
    return { state: 'failed', message: 'type reference unavailable' };
  if (row.typeScriptName === undefined)
    throw new Error(`Cell ${row.txHash}:${row.outputIndex} row has no typeScriptName (R6)`);
  return {
    state: 'typed',
    text: row.typeScriptName ?? `type ${shortenHash(row.typeCodeHash, size)}`,
    href: `/script/${row.typeCodeHash}?hashType=${row.typeHashType}`,
    title: `type script ${row.typeCodeHash} · ${row.typeHashType}`,
  };
}

function WhatItIs({ what }: { what: What }) {
  switch (what.state) {
    case 'reading':
      return <span className="text-text-dim">…</span>;
    case 'failed':
      return (
        <span role="alert" title={what.message} className="text-rouge">
          {what.message}
        </span>
      );
    case 'item':
    case 'typed':
      return (
        <Link href={what.href} title={what.title} className="text-text hover:text-jade">
          {what.text}
        </Link>
      );
    case 'plain':
      return <span className="text-text-dim">plain cell</span>;
  }
}

function whatText(what: What): string {
  return what.state === 'reading'
    ? 'reading the cell'
    : what.state === 'failed'
      ? what.message
      : what.state === 'plain'
        ? 'plain cell'
        : what.text;
}

function Owner({ cell, size }: { cell: Cell; size: ShortSize }) {
  return cell.address ? (
    <Link
      href={`/address/${cell.address}`}
      title={cell.address}
      className="text-text hover:text-jade"
    >
      <ShortId value={cell.address} kind="address" size={size} />
    </Link>
  ) : (
    <span role="alert" className="text-negative">
      Owner address unavailable
    </span>
  );
}

/** The day the cell was created, from its detail (list rows carry no timestamp). */
function createdDate(detail: Cell | undefined): ReactNode {
  if (!detail) return null;
  if (!detail.createdTimestamp)
    return (
      <span role="alert" className="text-negative">
        date unavailable
      </span>
    );
  return (
    <span className="tabular-nums">
      <Utc8Date timestamp={detail.createdTimestamp} />
    </span>
  );
}

/**
 * "tx 0x1234…abcd · #20,911,313 · 2026-11-02": where and when the cell was made. Its facts break
 * between them, never inside one: on a phone its widest, "tx 0x1234…abcd · #99,999,999 · date
 * unavailable" (47 characters at 12 px, 338.4 px), is past a 328 px row (the coordinator's sweep,
 * F3-e); the table's numeric cell keeps it on one line.
 */
function Created({
  cell,
  detail,
  size,
}: {
  cell: Cell;
  detail: Cell | undefined;
  size: ShortSize;
}) {
  return (
    <span className="text-text-dim">
      <Dotted
        parts={[
          <>
            tx{' '}
            <Link
              href={`/tx/${cell.txHash}`}
              title={cell.txHash}
              className="text-text-dim hover:text-jade"
            >
              <ShortId value={cell.txHash} kind="hash" size={size} />
            </Link>
          </>,
          <BlockLink key="block" number={cell.createdAtBlock} />,
          createdDate(detail),
        ]}
      />
    </span>
  );
}

function Head({ label }: { label: string }) {
  return <span className="text-meta">{label}</span>;
}

interface Row {
  cell: Cell;
  detail: Cell | undefined;
  error: Error | null;
}

/**
 * Lane S (S1, S2): the script's newest live cells, five, newest block first — one index (lock or
 * type, ruling 2), read by `GET /cells/by-script?order=desc`. Live cells only: the index holds no
 * spent cell, so a row has no status and the section is not a list of transactions (ruling 3).
 * Each row: the out point (the row's one bright link), what the cell is, its owner, and the tx,
 * block and day it was created. The footer jumps to "Its cells" and, for a family the registry
 * maps to an activity class, opens that class's feed.
 */
export function ScriptNewestCells({
  family,
  newest,
  className,
}: {
  family: ScriptFamilyDetail;
  newest: NewestCellsState;
  className: string;
}) {
  const details = useQueries({
    queries: newest.rows.map((cell) => ({
      queryKey: ['cell', cell.txHash, cell.outputIndex],
      queryFn: () => api.getCell(cell.txHash, cell.outputIndex),
      retry: retryTransientOnly,
    })),
  });
  const rows: Row[] = newest.rows.map((cell, i) => ({
    cell,
    detail: details[i]?.data,
    error: details[i]?.error ?? null,
  }));
  const activity = scriptActivityLink(family.familyId);
  const verb = newest.kind === 'type' ? 'types' : 'locks';
  const outpoint = (cell: Cell) => `${cell.txHash}:${cell.outputIndex}`;
  const rowTitle = (row: Row) => {
    const { cell, detail } = row;
    return [
      outpoint(cell),
      whatText(whatOf(row, 'row')),
      cell.address ? `owner ${cell.address}` : 'owner address unavailable',
      `created in tx ${cell.txHash}`,
      `at ${formatBlockNumber(cell.createdAtBlock)}`,
      ...(detail?.createdTimestamp ? [formatUtc8Date(detail.createdTimestamp)] : []),
    ].join(' · ');
  };
  // The out point at the form's size (U-09): `row` in the table, `tight` in the phone row.
  const key = ({ cell }: Row, size: ShortSize) => (
    <Link
      href={`/cell/${cell.txHash}-${cell.outputIndex}`}
      title={outpoint(cell)}
      className="text-body text-text-bright hover:text-jade leading-row hover:underline"
    >
      <ShortId value={cell.txHash} kind="hash" size={size} />
      <OutpointIndex index={cell.outputIndex} />
    </Link>
  );
  const columns: ListColumn<Row>[] = [
    {
      key: 'cell',
      label: 'Cell',
      header: <Head label="Cell" />,
      // The out point at the row size and an output index of four digits, ":9999" (22
      // characters at the key's 14 px, 184.8), and the gap (readability F3-e: it was 176, the
      // out point at 13 px with a one-digit index, and 15 % of the table).
      minWidth: columnPx(
        textPx(
          shortChars(SHORT_FORMS.row.hash) + `:${WIDEST_COUNT_DIGITS}`.length,
          fontPx('body')
        ) + CELL_GAP_PX
      ),
      render: (row) => key(row, 'row'),
    },
    {
      key: 'what',
      label: 'What it is',
      header: <Head label="What it is" />,
      // The table's flexible column (F3-e): 20 characters of what the cell is (a token amount and
      // its symbol, a name) at its minimum, the rest of the container beyond the sum; a longer one
      // ends in the one ellipsis, the row's title holding it. It was 190.
      minWidth: columnPx(textPx(20, fontPx('small')) + CELL_GAP_PX),
      width: 'auto',
      className: 'truncate',
      render: (row) => <WhatItIs what={whatOf(row, 'row')} />,
    },
    {
      key: 'owner',
      label: 'Owner',
      header: <Head label="Owner" />,
      // The owner's address at the row size (13 characters at 13 px, 101.4) and the gap; "Owner
      // address unavailable" breaks between its words. It was 170 and 17 % of the table.
      minWidth: columnPx(
        textPx(shortChars(SHORT_FORMS.row.address), fontPx('small')) + CELL_GAP_PX
      ),
      render: ({ cell }) => <Owner cell={cell} size="row" />,
    },
    {
      key: 'created',
      label: 'Created in',
      header: <Head label="Created in" />,
      // "tx 0x12345678…abcdef · #99,999,999 · 2026-11-02": 47 characters at 13 px (366.6), the last
      // column, no gap after it (F3-e: it was 356 at 12 px with the gap, and 30 % of the table).
      minWidth: columnPx(
        textPx(
          `tx ${'x'.repeat(shortChars(SHORT_FORMS.row.hash))} · ${WIDEST_BLOCK_NUMBER} · ${formatUtc8Date(0)}`
            .length,
          fontPx('small')
        )
      ),
      numeric: true,
      render: ({ cell, detail }) => <Created cell={cell} detail={detail} size="row" />,
    },
  ];
  return (
    <section aria-label="Newest cells" className={`min-w-0 ${className}`}>
      <div className="mb-[18px] flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        {/* Rubbing × Song print, phase 5 (A20): the same command as the family's cells, run on
            the newest few the list reads. */}
        <PromptHeading command="Cells" args={`--newest ${NEWEST_CELLS_COUNT}`} />
      </div>
      {newest.kind === null ? (
        <p role="alert" className="text-negative text-small">
          {family.name} has no registered lock or type role, so its newest cells cannot be read
        </p>
      ) : newest.error ? (
        <p role="alert" className="text-negative text-small [overflow-wrap:anywhere]">
          Newest cells unavailable: {readErrorText(newest.error)}
        </p>
      ) : newest.loading ? (
        <p className="text-text-dim text-small py-3">Loading the newest cells…</p>
      ) : (
        <ListTable
          label="Newest cells"
          rows={rows}
          rowKey={({ cell }) => outpoint(cell)}
          empty={`No live cell is ${newest.kind === 'type' ? 'typed' : 'locked'} by ${family.name}`}
          density="row"
          rowTitle={rowTitle}
          // T5: body ink for the row, the out point alone bright.
          rowClassName={() => 'text-text'}
          columns={columns}
          mobileRow={(row) => (
            <PhoneRow
              testId="newest-cell-row-mobile"
              lines={[
                // The owner is short enough to share the out point's line; what the cell is can
                // be long (a token amount, a .bit name) and takes its own line, wrapping.
                {
                  key: 'cell',
                  tone: 'head',
                  left: key(row, 'tight'),
                  right: <Owner cell={row.cell} size="tight" />,
                },
                { key: 'what', tone: 'body', left: <WhatItIs what={whatOf(row, 'tight')} /> },
                {
                  key: 'created',
                  tone: 'quiet',
                  left: <Created cell={row.cell} detail={row.detail} size="tight" />,
                },
              ]}
            />
          )}
        />
      )}
      {newest.kind && (
        // The section's one caption, under its evidence (as the custody timeline's is).
        <p data-role="caption" className="text-text-dim text-small mt-2">
          {/* The words' own box (R16 measures a text node's parent: a bare <p> is full width). */}
          <span>
            The newest live cells this script {verb}, newest block first · it is not a list of
            transactions
          </span>
        </p>
      )}
      <p data-role="newest-footer" className="text-small mt-1 flex flex-wrap gap-x-4 gap-y-1">
        <a href="#its-cells" className="text-text hover:text-jade">
          All {unitNode(formatCount(family.liveCellsCount, 'cell'))} ↓
        </a>
        {activity && (
          <Link href={activity.href} className="text-text hover:text-jade">
            {activity.label}
          </Link>
        )}
      </p>
    </section>
  );
}
