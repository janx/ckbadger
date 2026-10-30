import { describe, expect, it } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import { TransactionListTable } from '@/components/transaction/transaction-list-table';
import type { Transaction } from '@/lib/api';

const base: Omit<Transaction, 'hash' | 'activity' | 'isCellbase'> = {
  blockNumber: 20829775,
  blockHash: `0x${'cd'.repeat(32)}`,
  index: 1,
  inputsCount: 3,
  outputsCount: 3,
  fee: '30000',
  value: '0',
  outputCapacity: '100000000000',
  txSize: 903,
  cycles: 1704957,
  cyclesStatus: 'complete',
  timestamp: '2026-10-25T00:00:00Z',
};
const scriptCall: Transaction = {
  ...base,
  hash: `0x00de304b${'00'.repeat(26)}25cbbc`,
  isCellbase: false,
  activity: {
    txHash: `0x00de304b${'00'.repeat(26)}25cbbc`,
    blockNumber: 20829775,
    txIndex: 1,
    timestamp: '2026-10-25T00:00:00Z',
    isCellbase: false,
    protocolActions: [],
    lockCalls: [],
    typeCalls: [
      {
        typeCodeHash: '0xtime',
        typeHashType: 'type',
        typeArgs: '0x00',
        scriptHash: '0xs1',
        scriptName: '.bit Time Index State',
      },
      {
        typeCodeHash: '0xinfo',
        typeHashType: 'type',
        typeArgs: '0x00',
        scriptHash: '0xs2',
        scriptName: '.bit Time Info',
      },
    ],
    participants: [feedSamples.objectTransfer.participants[2]],
  },
};
const transfer: Transaction = {
  ...base,
  hash: `0xcfaebc55${'00'.repeat(26)}e6dd61`,
  isCellbase: false,
  value: '76729874184',
  activity: { ...feedSamples.eightParties, typeCalls: [], protocolActions: [] },
};
const cellbase: Transaction = {
  ...base,
  hash: `0xc89d2d25${'00'.repeat(26)}9a2cd7`,
  index: 0,
  isCellbase: true,
  inputsCount: 1,
  outputsCount: 1,
  fee: '0',
  value: '0',
  outputCapacity: '55961739927',
  txSize: 360,
  cycles: 0,
  activity: null,
};

describe('TransactionListTable (Transactions board, TX-25 / TX-26 / D5)', () => {
  it('uses the board columns, with the bar scales in the header', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer, cellbase]} />);
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    expect(
      within(table)
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual([
      'Transaction',
      'Block · age',
      'What it did',
      'Value, CKB',
      'Fee, CKB',
      'Cells',
      'Size, bytes01,000',
      'Cycles02,000,000',
    ]);
  });

  it('says what each transaction did: kind, mark and a quiet note', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer]} />);
    const rows = within(screen.getByRole('table')).getAllByRole('row').slice(1);
    expect(rows[0]).toHaveTextContent('Script call.bit Time Index State · .bit Time Info');
    expect(rows[1]).toHaveTextContent(
      `CKB Transfer${feedSamples.eightParties.participants.length} participants`
    );
  });

  it('sets cellbase rows back and shows the reward they pay, grey, outside every total (D5)', () => {
    render(<TransactionListTable transactions={[cellbase]} />);
    const row = within(screen.getByRole('table')).getAllByRole('row')[1];
    expect(row).toHaveClass('opacity-[.55]');
    expect(row).toHaveTextContent('Cellbase');
    const value = row.querySelector('[data-reward]')!;
    expect(value).toHaveTextContent('reward 559.61739927 CKB');
    expect(value).toHaveClass('text-text-dim');
    expect(row).toHaveTextContent('0 → 1');
    expect(row.querySelector('[data-segment="value"]')).toHaveStyle({
      backgroundColor: 'rgb(58, 66, 88)',
    });
  });
});
