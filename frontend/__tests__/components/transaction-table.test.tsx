import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { TransactionTable } from '@/components/transaction/transaction-table';
import type { Transaction } from '@/lib/api';

const tx: Transaction = {
  hash: `0x${'ab'.repeat(32)}`,
  blockNumber: 500,
  blockHash: `0x${'cd'.repeat(32)}`,
  index: 1,
  inputsCount: 2,
  outputsCount: 3,
  fee: '12345',
  value: '9007199254740993',
  txSize: 226,
  cycles: 1709221,
  cyclesStatus: 'complete',
  outputCapacity: '19007199254740993',
  proposalOrigin: { blockNumber: 497, distance: 3, uncle: null },
  activity: {
    txHash: `0x${'ab'.repeat(32)}`,
    blockNumber: 500,
    txIndex: 1,
    timestamp: '2026-10-25T00:00:00Z',
    isCellbase: false,
    participants: [],
    protocolActions: [],
    typeCalls: [],
    lockCalls: [],
  },
  isCellbase: false,
  timestamp: '2026-10-25T00:00:00Z',
};
describe('TransactionTable', () => {
  it('renders the exact API Value and byte/cycle evidence with network-aware links', () => {
    render(<TransactionTable transactions={[tx]} />);
    expect(screen.getByRole('table', { name: 'Committed transactions' })).toHaveTextContent(
      '90,071,992.54740993'
    );
    expect(screen.getAllByRole('link', { name: '#500' })[0]).toHaveAttribute(
      'href',
      '/mainnet/blocks/500'
    );
    expect(screen.getAllByRole('img', { name: 'Size, bytes: 226' })).toHaveLength(2);
  });
  it('shows exact output capacity and proposal origin in a block table', () => {
    render(<TransactionTable transactions={[tx]} showBlock={false} />);
    expect(screen.getByRole('table')).toHaveTextContent('Output capacity');
    expect(screen.getByRole('table')).toHaveTextContent('190,071,992.54740993');
    expect(screen.getAllByRole('link', { name: '#497' })[0]).toHaveAttribute(
      'href',
      '/mainnet/blocks/497'
    );
  });
  it('renders a pending replay explicitly and rejects missing byte evidence', () => {
    render(<TransactionTable transactions={[{ ...tx, cycles: null, cyclesStatus: 'pending' }]} />);
    expect(screen.getAllByText('Not yet run')).toHaveLength(2);
    expect(() => TransactionTable({ transactions: [{ ...tx, txSize: undefined }] })).toThrow(
      'exact serialized size'
    );
  });
});
