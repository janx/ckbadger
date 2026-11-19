// The cell page's transactions (lane D3; UI review U-07, U-17, PA-24; the board's embed): the
// cell page copied the whole transaction that created it, 158 tiles and its script groups, 24,288
// px for one cell. It embeds its own tile and its continuity partner; the rest of each sheet is a
// count; one line says which of the transaction's cells it is and gives the one link to the
// transaction, at this cell's out point; the script groups stand in the developer tier, in the
// transaction page's rows and words.
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { ReactElement } from 'react';
import {
  CellScriptGroups,
  CellTransactions,
  TransactionEvidence,
} from '@/components/cell/cell-transactions';
import type { Cell } from '@/lib/api';
import liveFixture from '@/__tests__/fixtures/d5/cell-live.json';
import { LONG_TX_HASH, LONG_TX_PREV, LONG_TX_SENDER, longDeposit } from '@/__tests__/utils/long-tx';

const getTransactionDetail = vi.fn();
const getTransactionCellDeps = vi.fn();
vi.mock('@/lib/api', () => ({
  api: {
    getTransactionDetail: (...args: unknown[]) => getTransactionDetail(...args),
    getTransactionCellDeps: (...args: unknown[]) => getTransactionCellDeps(...args),
    lookupScripts: async () => ({}),
  },
}));

/** The deposit, output 0 of the long transaction: live, created by it. */
const deposit = {
  ...liveFixture,
  txHash: LONG_TX_HASH,
  outputIndex: 0,
  currentSpendRequirements: [],
} as unknown as Cell;
const SHORT_SENDER = `ckb1…${LONG_TX_SENDER.slice(-8)}`;
const SHORT_TX = `${LONG_TX_HASH.slice(0, 10)}…${LONG_TX_HASH.slice(-6)}`;

function view(ui: ReactElement) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={client}>{ui}</QueryClientProvider>);
}

/** A sheet of the embed: its drawn tiles and its fold row's words. */
function sheet(root: ParentNode, side: 'consumed' | 'created') {
  const el = root.querySelector<HTMLElement>(`[data-sheet="${side}"]`)!;
  return {
    tiles: Array.from(el.querySelectorAll<HTMLElement>('[data-cell-id]')).map(
      (t) => t.dataset.cellId
    ),
    words: el.querySelector('[data-fold-words]')?.textContent ?? null,
    controls: el.querySelectorAll('[data-fold] button').length,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  getTransactionDetail.mockResolvedValue(longDeposit());
  getTransactionCellDeps.mockResolvedValue([]);
});

describe('the cell page embeds its own tile, not the transaction (lane D3)', () => {
  it('draws this cell’s tile, each sheet’s rest a count, and one link to the transaction', async () => {
    view(<CellTransactions cell={deposit} />);
    const structure = await screen.findByLabelText(`Transaction ${LONG_TX_HASH} structure`);
    const consumed = sheet(structure, 'consumed');
    expect(consumed.tiles).toEqual([]);
    // re-pinned on purpose (lane P1, lane D3's note: ", none of them this cell" says nothing on the
    // sheet a cell page draws, where this cell cannot stand): the count and its owner alone
    expect(consumed.words).toMatch(new RegExp(`^158\\scells from ${SHORT_SENDER}$`));
    expect(consumed.controls).toBe(0);
    const created = sheet(structure, 'created');
    expect(created.tiles).toEqual(['out:0']);
    expect(created.words).toMatch(/^1\smore cell created$/);
    expect(structure.querySelector('[data-cell-id="out:0"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );
    // the line: which of the transaction's cells this is, by whom and when, and the one link to
    // the transaction, at this cell's out point (the transaction page keeps that cell drawn)
    const line = screen.getByTestId('cell-transaction-line');
    expect(line).toHaveTextContent(
      new RegExp(
        `^one of 2\\scells created by ${SHORT_TX.replace('.', '\\.')} · .+ · open the transaction`
      )
    );
    const links = within(line).getAllByRole('link');
    expect(links).toHaveLength(1);
    expect(links[0]).toHaveAttribute('href', `/mainnet/tx/${LONG_TX_HASH}#${LONG_TX_HASH}-0`);
    expect(links[0]).toHaveClass('text-jade');
    expect(screen.getAllByRole('link', { name: /open the transaction/ })).toHaveLength(1);
    // the script groups are the developer tier's, not the embed's
    expect(document.querySelector('[data-group-id], [data-group-row]')).toBeNull();
  });

  it('draws a spent cell as its spending transaction’s input, the rest of that sheet counted', async () => {
    view(
      <TransactionEvidence
        hash={LONG_TX_HASH}
        label="Spent"
        focus="in:57"
        anchor={`${LONG_TX_PREV}-57`}
      />
    );
    const structure = await screen.findByLabelText(`Transaction ${LONG_TX_HASH} structure`);
    expect(sheet(structure, 'consumed').tiles).toEqual(['in:57']);
    expect(sheet(structure, 'consumed').words).toMatch(
      new RegExp(`^157\\smore cells from ${SHORT_SENDER}$`)
    );
    expect(sheet(structure, 'created').tiles).toEqual([]);
    // re-pinned on purpose (lane P1, lane D3's note): "2 cells created" is enough there
    expect(sheet(structure, 'created').words).toMatch(/^2\scells created$/);
    const line = screen.getByTestId('cell-transaction-line');
    expect(line).toHaveTextContent(/^one of 158\scells consumed by /);
    expect(within(line).getByRole('link')).toHaveAttribute(
      'href',
      `/mainnet/tx/${LONG_TX_HASH}#${LONG_TX_PREV}-57`
    );
  });

  it('writes the script groups in the developer tier in the transaction page’s rows', async () => {
    view(<CellScriptGroups cell={deposit} />);
    const heading = await screen.findByRole('heading', {
      name: 'Script groups of the transaction that created it',
    });
    const section = heading.closest('section')!;
    // the transaction page's rows: one per group, nothing to press, the page's words
    await waitFor(() => expect(section.querySelectorAll('[data-group-row]')).toHaveLength(4));
    expect(section.querySelectorAll('button')).toHaveLength(0);
    expect(section).toHaveTextContent('attached, runs when spent');
    expect(section).not.toHaveTextContent(/not executed|executed/);
  });
});
