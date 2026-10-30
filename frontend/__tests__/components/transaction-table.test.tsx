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
  it('renders a neutral transaction hash that navigates without a copy affordance', () => {
    render(<TransactionTable transactions={[tx]} />);
    for (const hash of screen.getAllByTitle(tx.hash)) {
      expect(hash).toHaveClass('text-text', 'text-xs');
      expect(hash).toHaveTextContent('0xababababab...ababab');
      expect(hash.children).toHaveLength(0);
      expect(hash.closest('a')).toHaveAttribute('href', `/mainnet/tx/${tx.hash}`);
    }
    expect(screen.queryByTitle(/Click to copy/)).not.toBeInTheDocument();
  });
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
  it('fits block evidence into a narrower column budget with stacked resource bars', () => {
    render(
      <TransactionTable
        transactions={[
          {
            ...tx,
            outputCapacity: '18446744073709551615',
            fee: '123456789012345',
            cycles: 3500000000,
          },
        ]}
        showBlock={false}
      />
    );
    const table = screen.getByRole('table');
    expect(Number.parseFloat(table.style.minWidth)).toBeLessThanOrEqual(928);
    expect(table).toHaveTextContent('184,467,440,737.09551615 CKB');
    expect(table).toHaveTextContent('1,234,567.89012345 CKB');
    expect(
      screen.getAllByRole('img', { name: 'Cycles: 3,500,000,000' })[0].parentElement
    ).toHaveClass('flex-col');
    expect(table.querySelector('[data-ckb-amount]')).toHaveClass('flex-wrap');
  });
  it('marks the class of what a transaction did beside its headline (IV-22 wiring)', () => {
    const dao = {
      ...tx,
      hash: `0x${'de'.repeat(32)}`,
      activity: {
        ...tx.activity!,
        txHash: `0x${'de'.repeat(32)}`,
        protocolActions: [
          {
            protocol: 'dao',
            protocolName: 'Nervos DAO',
            action: 'deposit',
            metadata: { capacity: '10000000000' },
          },
        ],
      },
    } as Transaction;
    const { container } = render(<TransactionTable transactions={[dao, tx]} showBlock />);
    const marks = Array.from(container.querySelectorAll('svg[data-class]'));
    // The DAO deposit row carries the vault ring (table + compact layout); a plain transfer
    // carries no class mark.
    expect(marks.map((m) => m.getAttribute('data-class'))).toEqual(['dao', 'dao']);
    expect(marks[0]).toHaveAttribute('width', '12');
  });

  it('shows cellbase as 0 → N cells, not 1 → N (BA-11)', () => {
    render(
      <TransactionTable
        transactions={[{ ...tx, isCellbase: true, inputsCount: 1, outputsCount: 1 }]}
      />
    );
    expect(screen.getByRole('table')).toHaveTextContent('0→1');
  });

  it('renders genesis non-cellbase transactions without inventing a proposal origin', () => {
    render(
      <TransactionTable
        transactions={[{ ...tx, blockNumber: 0, proposalOrigin: null, cycles: 0 }]}
        showBlock={false}
      />
    );
    expect(screen.getByRole('table')).toHaveTextContent('Genesis');
    expect(screen.queryByRole('link', { name: '#497' })).not.toBeInTheDocument();
    expect(screen.getAllByRole('img', { name: 'Cycles: 0' })).toHaveLength(2);
  });
  it('renders a pending replay explicitly and rejects missing byte evidence', () => {
    render(<TransactionTable transactions={[{ ...tx, cycles: null, cyclesStatus: 'pending' }]} />);
    expect(screen.getAllByText('Not yet run')).toHaveLength(2);
    expect(() => TransactionTable({ transactions: [{ ...tx, txSize: null }] })).toThrow(
      'exact serialized size'
    );
  });
});
