import { render, screen } from '@/__tests__/utils/test-utils';
import { TransactionEvidence } from '@/components/cell/cell-transactions';
import { api, type TransactionDetail } from '@/lib/api';
import { LONG_TX_HASH, longDeposit } from '@/__tests__/utils/long-tx';

// Re-pinned on purpose (lane D3): a transaction's evidence on the cell page is about one cell of
// it, the cell's tile its focus, so the transaction holds that cell (it was a transaction of no
// cells, rendered with no focus): the long deposit, at the time this test always read.
const tx = {
  ...longDeposit(),
  timestamp: '2026-10-29T10:12:50.452+00:00',
} as unknown as TransactionDetail;

it('prints the committing block time as the site UTC+8 clock, not raw ISO (#16, D2)', async () => {
  vi.spyOn(api, 'getTransactionDetail').mockResolvedValue(tx);
  vi.spyOn(api, 'getTransactionCellDeps').mockResolvedValue([]);
  vi.spyOn(api, 'lookupScripts').mockResolvedValue({});
  render(
    <TransactionEvidence
      hash={LONG_TX_HASH}
      label="Created"
      focus="out:0"
      cell={{ txHash: LONG_TX_HASH, index: 0 }}
    />
  );
  expect(await screen.findByText('2026-10-29 18:12:50 UTC+8')).toBeInTheDocument();
  expect(screen.queryByText(tx.timestamp!)).not.toBeInTheDocument();
});
