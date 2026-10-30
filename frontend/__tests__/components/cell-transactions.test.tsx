import { render, screen } from '@/__tests__/utils/test-utils';
import { TransactionEvidence } from '@/components/cell/cell-transactions';
import { api, type TransactionDetail } from '@/lib/api';

const hash = `0x${'ab'.repeat(32)}`;
const tx = {
  hash,
  status: 'committed',
  blockNumber: 12,
  blockHash: `0x${'cd'.repeat(32)}`,
  index: 1,
  inputsCount: 0,
  outputsCount: 0,
  fee: '0',
  feeRate: null,
  txSize: null,
  cycles: null,
  cyclesStatus: 'pending',
  isCellbase: false,
  timestamp: '2026-10-29T10:12:50.452+00:00',
  confirmations: 3,
  inputsCapacity: '0',
  outputsCapacity: '0',
  inputsCommonKnowledgeSize: '0',
  outputsCommonKnowledgeSize: '0',
  inputs: [],
  outputs: [],
  scriptGroups: [],
  witnessDetails: [],
  witnesses: [],
  witnessesAvailable: true,
} as unknown as TransactionDetail;

it('prints the committing block time as the UTC clock, not raw ISO (#16)', async () => {
  vi.spyOn(api, 'getTransactionDetail').mockResolvedValue(tx);
  vi.spyOn(api, 'getTransactionCellDeps').mockResolvedValue([]);
  render(<TransactionEvidence hash={hash} label="Created" />);
  expect(await screen.findByText('2026-10-29 10:12:50 UTC')).toBeInTheDocument();
  expect(screen.queryByText(tx.timestamp!)).not.toBeInTheDocument();
});
