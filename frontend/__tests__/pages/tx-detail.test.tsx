import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor, fireEvent } from '@testing-library/react';
import { render } from '../utils/test-utils';
import TransactionDetailPage from '@/app/tx/[hash]/page';
import { api } from '@/lib/api';

const TX_HASH = '0x57a54eb7922190d5b0e0d7f5ad91dbbd91714a9bd85200994f99250ddc08e0f';
const LOCK_CODE_HASH = '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8';
const TYPE_CODE_HASH = '0x7366a61534fa7c7e6225ecc0d828ea3b5366adec2b58206f2ee84995fe030075';

vi.mock('@/lib/api', () => ({
  api: {
    getTransactionDetail: vi.fn(),
    getTransactionGraph: vi.fn(),
    getTransactionCellDeps: vi.fn(),
    getTransactionLifecycle: vi.fn(),
    lookupScripts: vi.fn(),
  },
}));

vi.mock('@/hooks/useCyclesCalculation', () => ({
  useCyclesCalculation: () => ({
    cycles: 4446651,
    hasCycles: true,
    isCalculating: false,
    hasFailed: false,
  }),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('next/navigation', () => ({
  useParams: () => ({ hash: TX_HASH }),
  useRouter: () => ({ push: vi.fn() }),
}));

describe('TransactionDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();

    vi.mocked(api.getTransactionDetail).mockResolvedValue({
      hash: TX_HASH,
      blockNumber: 18661531,
      blockHash: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      index: 0,
      inputsCount: 1,
      outputsCount: 1,
      fee: '58803',
      feeRate: '52130',
      txSize: 1128,
      isCellbase: false,
      timestamp: '2026-02-20T16:46:12Z',
      confirmations: 4,
      inputsCapacity: '55500000000',
      outputsCapacity: '55499941197',
      inputsOccupiedCapacity: '6100000000',
      outputsOccupiedCapacity: '6100000650',
      inputs: [
        {
          previousOutput: {
            txHash: '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
            index: 0,
          },
          capacity: '55500000000',
          lock: {
            codeHash: LOCK_CODE_HASH,
            hashType: 'type',
            args: '0x1111111111111111111111111111111111111111',
          },
          address: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq',
        },
      ],
      outputs: [
        {
          capacity: '55499941197',
          occupiedCapacity: 61,
          lock: {
            codeHash: LOCK_CODE_HASH,
            hashType: 'type',
            args: '0x2222222222222222222222222222222222222222',
          },
          type: {
            codeHash: TYPE_CODE_HASH,
            hashType: 'type',
            args: '0x',
          },
          address: 'ckb1qypk9w2g8j6v4e5xj0k9n3l2m1p8q7r6s5t4u3v2w1x0y9z8a7b6c5d4e3',
        },
      ],
    });

    vi.mocked(api.lookupScripts).mockResolvedValue({
      [LOCK_CODE_HASH]: {
        codeHash: LOCK_CODE_HASH,
        name: 'Default Multisig',
        scriptKind: 'lock',
        decoderType: null,
        hashType: 'type',
        codeCellTxHash: null,
        codeCellOutputIndex: null,
        liveCellsCount: 10,
        liveCapacitySum: '100000000000',
        liveOccupiedCapacitySum: '60000000000',
      },
      [TYPE_CODE_HASH]: {
        codeHash: TYPE_CODE_HASH,
        name: 'Unknown',
        scriptKind: 'type',
        decoderType: null,
        hashType: 'type',
        codeCellTxHash: null,
        codeCellOutputIndex: null,
        liveCellsCount: 1,
        liveCapacitySum: '10000000000',
        liveOccupiedCapacitySum: '6000000000',
      },
    });

    vi.mocked(api.getTransactionGraph).mockResolvedValue({ nodes: [], links: [] });
    vi.mocked(api.getTransactionCellDeps).mockResolvedValue([]);
    vi.mocked(api.getTransactionLifecycle).mockResolvedValue({
      hash: TX_HASH,
      phase: 'committed',
      proposalId: '0x01',
      proposedIn: null,
      committedIn: null,
      commitmentDistance: null,
      commitmentWindow: { close: 2, far: 10 },
      isCellbase: false,
      confirmations: 4,
    });
  });

  it('links unknown type script to code-hash detail page', async () => {
    render(<TransactionDetailPage />);

    await waitFor(() => {
      expect(api.getTransactionDetail).toHaveBeenCalled();
    });

    fireEvent.click(await screen.findByRole('button', { name: 'Scripts' }));

    const unknownLink = await screen.findByRole('link', { name: 'Unknown' });
    expect(unknownLink).toHaveAttribute(
      'href',
      `/script/${TYPE_CODE_HASH}?hashType=type&kind=type`
    );
    expect(document.querySelector('a[href="/scripts/Unknown"]')).toBeNull();
  });
});
