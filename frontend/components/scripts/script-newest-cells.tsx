'use client';
import type { ReactNode } from 'react';
import { useQueries } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { BlockLink } from '@/components/ui/block-link';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import { api, type Cell, type ScriptFamilyDetail } from '@/lib/api';
import { cellInventoryItem, type CellInventoryItem } from '@/lib/cell-inventory';
import { readErrorText } from '@/lib/read-error';
import { retryTransientOnly } from '@/lib/query-client';
import { scriptActivityLink } from '@/lib/script-activity-link';
import { shortHex } from '@/lib/script-lineage';
import type { NewestCellsKind } from '@/lib/script-newest-cells';
import { formatBlockNumber } from '@/lib/format-block';
import { formatUtc8Date } from '@/lib/utils';

export interface NewestCellsState {
  /** The index the list reads (ruling 2); null when the family has no lock or type role. */
  kind: NewestCellsKind | null;
  rows: readonly Cell[];
  loading: boolean;
  error: Error | null;
}

const IDENTITY_WORD = { dotbit: '.bit', 'bit-cell': '.bit Cell', did: 'did:ckb', dotcell: '.cell' };

const shortId = (id: string) => shortHex(id, 8, 6);

/**
 * The words lane U's inventory naming gives the item: its pending statement on the cell page (the
 * class and the short id; no further name is read per row), or the name the cell's data carries.
 */
function itemWords(item: CellInventoryItem): string {
  switch (item.kind) {
    case 'spore':
      return `Spore object ${shortId(item.id)}`;
    case 'spore-cluster':
      return `Spore cluster ${shortId(item.id)}`;
    case 'm-nft':
      return `m-NFT object ${shortId(item.id)}`;
    case 'm-nft-class':
      return `m-NFT class ${shortId(item.id)}`;
    case 'token':
      return `${item.amount} ${item.unit}`;
    case 'token-without-amount':
      return `${item.unit} · no readable amount`;
    case 'identity':
      return item.name ?? `${IDENTITY_WORD[item.standard]} identity ${shortId(item.id)}`;
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
 * own detail), else its type script by registry name (R6), else a plain cell.
 */
function whatOf(row: Cell, detail: Cell | undefined, error: Error | null): What {
  if (error)
    return { state: 'failed', message: `could not read the cell: ${readErrorText(error)}` };
  if (!detail) return { state: 'reading' };
  let item: CellInventoryItem | null;
  try {
    item = cellInventoryItem(detail);
  } catch (failure) {
    return { state: 'failed', message: (failure as Error).message };
  }
  if (item) return { state: 'item', text: itemWords(item), href: item.href, title: itemId(item) };
  if (!row.typeScriptHash) return { state: 'plain' };
  if (!row.typeCodeHash || !row.typeHashType)
    return { state: 'failed', message: 'type reference unavailable' };
  if (row.typeScriptName === undefined)
    throw new Error(`Cell ${row.txHash}:${row.outputIndex} row has no typeScriptName (R6)`);
  return {
    state: 'typed',
    text: row.typeScriptName ?? `type ${shortHex(row.typeCodeHash)}`,
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

function Owner({ cell }: { cell: Cell }) {
  return cell.address ? (
    <Link
      href={`/address/${cell.address}`}
      title={cell.address}
      className="text-text hover:text-jade"
    >
      {cell.address.slice(0, 10)}…{cell.address.slice(-6)}
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
  return <span className="tabular-nums">{formatUtc8Date(detail.createdTimestamp)}</span>;
}

/** "tx 0x1234…abcd · #20,911,313 · 2026-11-02": where and when the cell was made. */
function Created({ cell, detail }: { cell: Cell; detail: Cell | undefined }) {
  return (
    <span className="text-text-dim whitespace-nowrap">
      <Dotted
        parts={[
          <>
            tx{' '}
            <Link
              href={`/tx/${cell.txHash}`}
              title={cell.txHash}
              className="text-text-dim hover:text-jade"
            >
              {shortHex(cell.txHash, 6, 4)}
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
  what: What;
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
  const rows: Row[] = newest.rows.map((cell, i) => {
    const detail = details[i]?.data;
    return { cell, detail, what: whatOf(cell, detail, details[i]?.error ?? null) };
  });
  const activity = scriptActivityLink(family.familyId);
  const verb = newest.kind === 'type' ? 'types' : 'locks';
  const outpoint = (cell: Cell) => `${cell.txHash}:${cell.outputIndex}`;
  const rowTitle = ({ cell, detail, what }: Row) =>
    [
      outpoint(cell),
      whatText(what),
      cell.address ? `owner ${cell.address}` : 'owner address unavailable',
      `created in tx ${cell.txHash}`,
      `at ${formatBlockNumber(cell.createdAtBlock)}`,
      ...(detail?.createdTimestamp ? [formatUtc8Date(detail.createdTimestamp)] : []),
    ].join(' · ');
  const key = ({ cell }: Row) => (
    <Link
      href={`/cell/${cell.txHash}-${cell.outputIndex}`}
      title={outpoint(cell)}
      className="text-body text-text-bright hover:text-jade leading-[18px] hover:underline"
    >
      {shortHex(cell.txHash, 6, 4)}:{cell.outputIndex}
    </Link>
  );
  const columns: ListColumn<Row>[] = [
    {
      key: 'cell',
      label: 'Cell',
      header: <Head label="Cell" />,
      minWidth: 150,
      width: '15%',
      render: key,
    },
    {
      key: 'what',
      label: 'What it is',
      header: <Head label="What it is" />,
      minWidth: 190,
      width: 'auto',
      className: 'truncate',
      render: ({ what }) => <WhatItIs what={what} />,
    },
    {
      key: 'owner',
      label: 'Owner',
      header: <Head label="Owner" />,
      minWidth: 170,
      width: '17%',
      render: ({ cell }) => <Owner cell={cell} />,
    },
    {
      key: 'created',
      label: 'Created in',
      header: <Head label="Created in" />,
      minWidth: 300,
      width: '30%',
      numeric: true,
      render: ({ cell, detail }) => <Created cell={cell} detail={detail} />,
    },
  ];
  return (
    <section aria-label="Newest cells" className={`min-w-0 ${className}`}>
      <div className="mb-[18px] flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <h2 className="text-text-bright text-section font-medium">Newest cells</h2>
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
                { key: 'cell', tone: 'head', left: key(row), right: <Owner cell={row.cell} /> },
                { key: 'what', tone: 'body', left: <WhatItIs what={row.what} /> },
                {
                  key: 'created',
                  tone: 'quiet',
                  left: <Created cell={row.cell} detail={row.detail} />,
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
          All {family.liveCellsCount.toLocaleString('en-US')} cells ↓
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
