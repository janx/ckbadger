import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '../utils/test-utils';
import { LatestTransactions } from '@/components/latest-transactions';
import { usePipelineHoverStore } from '@/hooks/usePipelineHoverStore';
import { api, type Transaction } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getTransactions: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

function mockTx(hash: string): Transaction {
  return {
    hash,
    blockNumber: 8_775_638,
    blockHash: '0xblock',
    index: 0,
    inputsCount: 2,
    outputsCount: 3,
    fee: '10000',
    isCellbase: false,
    timestamp: '2026-02-22T00:00:00Z',
  };
}

describe('LatestTransactions', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders latest transaction details with transaction and block links', async () => {
    const txs: Transaction[] = [mockTx('0xtx1')];

    vi.mocked(api.getTransactions).mockResolvedValue({
      data: txs,
      total: txs.length,
      limit: 10,
      hasMore: false,
      nextCursor: null,
    });

    render(<LatestTransactions initialTransactions={txs} />);

    await waitFor(() => {
      expect(screen.getByText('Block')).toBeInTheDocument();
    });

    expect(screen.getByRole('link', { name: '#8,775,638' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/8775638'
    );
    expect(screen.getByTitle('Click to copy: 0xtx1').closest('a')).toHaveAttribute(
      'href',
      '/mainnet/tx/0xtx1'
    );
    expect(screen.getByText('2')).toBeInTheDocument();
    expect(screen.getByText('3')).toBeInTheDocument();
  });

  it('highlights the row of the tx hovered or pinned in the pipeline and hovers it back', async () => {
    usePipelineHoverStore.getState().clear();
    const txs: Transaction[] = [mockTx('0xtx1'), mockTx('0xtx2')];
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: txs,
      total: txs.length,
      limit: 10,
      hasMore: false,
      nextCursor: null,
    });
    render(<LatestTransactions initialTransactions={txs} />);
    const row = (await screen.findByTitle('Click to copy: 0xtx2')).closest(
      '[data-tx-row]'
    ) as HTMLElement;
    expect(row).toHaveAttribute('data-highlighted', 'false');

    act(() =>
      usePipelineHoverStore.getState().setHover({ block: null, tx: '0xtx2', window: null })
    );
    expect(row).toHaveAttribute('data-highlighted', 'true');
    act(() => usePipelineHoverStore.getState().clearHover());
    act(() => usePipelineHoverStore.getState().togglePin('0xtx2'));
    expect(row).toHaveAttribute('data-highlighted', 'true');
    act(() => usePipelineHoverStore.getState().clearPin());
    expect(row).toHaveAttribute('data-highlighted', 'false');

    fireEvent.pointerEnter(row);
    // A committed tx anchors the proposal window on the block that committed it.
    expect(usePipelineHoverStore.getState()).toMatchObject({
      hoverTx: '0xtx2',
      hoverBlock: 8_775_638,
      windowBlock: 8_775_638,
    });
    fireEvent.pointerLeave(row);
    expect(usePipelineHoverStore.getState().hoverTx).toBeNull();
  });
});
