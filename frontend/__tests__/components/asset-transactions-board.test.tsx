// OB-13 / OB-C9 (Collection / CollectionTxs / ObjectTxs boards): the Size and Cycles headers carry
// their column's shared scale, and a collection's Transactions name the collection in a column of
// its own right after Cells.
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { AssetTransactions } from '@/components/asset/asset-feed';
import type { Transaction } from '@/lib/api';

const tx = (n: number, size: number, cycles: number | null): Transaction => ({
  hash: `0x${n.toString(16).padStart(2, '0').repeat(32)}`,
  blockNumber: 1000 + n,
  blockHash: `0x${'aa'.repeat(32)}`,
  index: 1,
  inputsCount: 1,
  outputsCount: 2,
  fee: '1000',
  value: '0',
  outputCapacity: '50000000000',
  txSize: size,
  cycles,
  cyclesStatus: cycles === null ? 'pending' : 'complete',
  isCellbase: false,
  timestamp: '1700000000000',
});

describe('AssetTransactions board columns', () => {
  it('heads Size and Cycles with the scale every bar in the column shares (OB-13)', () => {
    render(<AssetTransactions rows={[tx(1, 1930, 1_923_713), tx(2, 640, 0)]} />);
    const table = screen.getByRole('table', { name: 'Asset transactions' });
    const size = within(table).getByRole('columnheader', { name: /Size/ });
    expect(size.querySelector('[data-scale-ticks]')).toHaveTextContent('02,000');
    const cycles = within(table).getByRole('columnheader', { name: /Cycles/ });
    expect(cycles.querySelector('[data-scale-ticks]')).toHaveTextContent('02,000,000');
  });

  it('puts the collection column, titled by the collection, right after Cells (OB-C9)', () => {
    render(
      <AssetTransactions
        rows={[tx(1, 1930, 1_923_713)]}
        context={{ label: 'Nervapes', placement: 'after-cells', render: () => '3 minted' }}
      />
    );
    const headers = screen.getAllByRole('columnheader').map((h) => h.textContent);
    expect(headers.slice(0, 4)).toEqual(['Transaction', 'Block · age', 'Cells', 'Nervapes']);
  });

  it('keeps an item context at the end of the row (OB-13)', () => {
    render(
      <AssetTransactions
        rows={[tx(1, 1930, null)]}
        context={{ label: 'Object · cell after', render: () => 'to ckb1… (output 1)' }}
      />
    );
    const headers = screen.getAllByRole('columnheader').map((h) => h.textContent);
    expect(headers.at(-1)).toBe('Object · cell after');
  });
});
