import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ScriptsPage from '@/app/scripts/page';
import { api, KnownScript } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getScripts: vi.fn(),
  },
  isWarmupPendingError: (error: unknown) =>
    Boolean(
      error &&
      typeof error === 'object' &&
      (error as { code?: string; status?: number }).code === 'warmup_pending' &&
      (error as { code?: string; status?: number }).status === 503
    ),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockScriptsResponse = {
  data: [
    {
      codeHash: '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
      name: 'SECP256K1_BLAKE160',
      description: 'Default lock script',
      scriptKind: 'lock',
      rfc: null,
      website: null,
      sourceUrl: null,
      decoderType: null,
      network: 'mainnet',
      hashType: 'type',
      dataHash: null,
      typeHash: null,
      tag: null,
      deprecated: false,
      isSystem: true,
      codeCellTxHash: null,
      codeCellOutputIndex: null,
      ownedCapacitySum: '2000000000',
      ownedKnowledgeSum: '1000000000',
      liveCellsCount: 4200,
      cellsCount: 8500,
      deployedAt: 0,
    } satisfies KnownScript,
    {
      codeHash: '0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',
      name: 'ALWAYS_SUCCESS',
      description: 'Simple testing script',
      scriptKind: 'type',
      rfc: null,
      website: null,
      sourceUrl: null,
      decoderType: null,
      network: 'mainnet',
      hashType: 'data',
      dataHash: null,
      typeHash: null,
      tag: null,
      deprecated: false,
      isSystem: true,
      codeCellTxHash: null,
      codeCellOutputIndex: null,
      ownedCapacitySum: '10000000000',
      ownedKnowledgeSum: '5000000000',
      liveCellsCount: 1500,
      cellsCount: 3200,
      deployedAt: 100,
    } satisfies KnownScript,
  ],
  total: 2,
  limit: 50,
  hasMore: false,
  nextCursor: null,
};

describe('ScriptsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('uses a family-name search placeholder', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScriptsResponse);

    render(<ScriptsPage />);

    expect(screen.getByPlaceholderText('Search by script family name...')).toBeInTheDocument();
  });

  it('renders sortable script list columns and entries', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScriptsResponse);

    render(<ScriptsPage />);

    await waitFor(() => {
      expect(api.getScripts).toHaveBeenCalledWith(
        expect.objectContaining({
          limit: 50,
          sortKey: 'capacity',
          sortDirection: 'desc',
        })
      );
    });

    await waitFor(() => {
      expect(screen.getAllByText('Kind').length).toBeGreaterThan(0);
    });

    expect(screen.getAllByRole('link', { name: 'SECP256K1_BLAKE160' })[0]).toBeInTheDocument();
    expect(screen.getAllByText('lock')[0]).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sort by Live Cells' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sort by Total Cells' })).toBeInTheDocument();
    expect(screen.queryByText('Deployed')).toBeNull();
    expect(screen.queryByRole('link', { name: '#0' })).toBeNull();
    expect(screen.queryByRole('link', { name: '#100' })).toBeNull();
    expect(screen.getByText('Capacity (CKB)')).toBeInTheDocument();
    expect(screen.queryByText('Utilization Ratio')).toBeNull();
    expect(screen.getByRole('button', { name: 'Sort by Used (CKB)' })).toBeInTheDocument();
  });

  it('shows deprecated badge for deprecated known scripts', async () => {
    vi.mocked(api.getScripts).mockResolvedValue({
      ...mockScriptsResponse,
      data: [
        {
          ...mockScriptsResponse.data[0],
          name: 'PW Lock',
          description: 'Ethereum wallet compatible lock',
          deprecated: true,
        },
      ],
      total: 1,
    });

    render(<ScriptsPage />);

    await waitFor(() => {
      expect(screen.getAllByRole('link', { name: 'PW Lock' }).length).toBeGreaterThan(0);
    });

    expect(screen.getAllByText('Deprecated').length).toBeGreaterThan(0);
  });

  it('supports sorting by common knowledge size', async () => {
    vi.mocked(api.getScripts)
      .mockResolvedValueOnce(mockScriptsResponse)
      .mockResolvedValueOnce({
        ...mockScriptsResponse,
        data: [mockScriptsResponse.data[1], mockScriptsResponse.data[0]],
      });

    render(<ScriptsPage />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Sort by Used (CKB)' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Sort by Used (CKB)' }));

    await waitFor(() => {
      expect(api.getScripts).toHaveBeenLastCalledWith(
        expect.objectContaining({
          limit: 50,
          sortKey: 'used',
          sortDirection: 'desc',
        })
      );
    });
  });

  it('routes Unknown script entries to code-hash detail page', async () => {
    const unknownCodeHash = '0x010445a300000000000000000000000000000000000000000000000000000001';
    const unknownScriptRefLabel = 'Unlabeled';
    const unknownScriptRefDisplay = 'type · 0x010445a3...00000001';
    const unknownScriptRefFull = `type ref:${unknownCodeHash}`;
    vi.mocked(api.getScripts).mockResolvedValue({
      data: [
        {
          codeHash: unknownCodeHash,
          name: 'Unknown',
          description: null,
          scriptKind: 'type',
          rfc: null,
          website: null,
          sourceUrl: null,
          decoderType: null,
          network: 'mainnet',
          hashType: 'type',
          dataHash: null,
          typeHash: null,
          tag: null,
          deprecated: false,
          isSystem: false,
          codeCellTxHash: null,
          codeCellOutputIndex: null,
          ownedCapacitySum: '2000000000',
          ownedKnowledgeSum: '1000000000',
        } satisfies KnownScript,
      ],
      total: 1,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });

    render(<ScriptsPage />);

    await waitFor(() => {
      expect(api.getScripts).toHaveBeenCalled();
    });

    await waitFor(() => {
      expect(screen.getAllByRole('link', { name: unknownScriptRefLabel }).length).toBeGreaterThan(
        0
      );
    });

    expect(screen.getAllByText(unknownScriptRefDisplay)[0]).toBeInTheDocument();
    expect(screen.getAllByRole('link', { name: unknownScriptRefLabel })[0]).toHaveAttribute(
      'href',
      `/script/${encodeURIComponent(unknownCodeHash)}?hashType=type&kind=type`
    );
    expect(screen.getAllByRole('link', { name: unknownScriptRefLabel })[0]).toHaveAttribute(
      'title',
      unknownScriptRefFull
    );
  });

  it('supports sorting by live cells', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScriptsResponse);
    render(<ScriptsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Sort by Live Cells' })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Sort by Live Cells' }));
    await waitFor(() => {
      expect(api.getScripts).toHaveBeenLastCalledWith(
        expect.objectContaining({ sortKey: 'liveCells', sortDirection: 'desc' })
      );
    });
  });

  it('shows API errors instead of empty-state message', async () => {
    vi.mocked(api.getScripts).mockRejectedValue(
      new Error('API error: 500 - negative live capacity in list scripts')
    );

    render(<ScriptsPage />);

    await waitFor(() => {
      expect(screen.getByText('Failed to load scripts')).toBeInTheDocument();
    });

    expect(screen.getByText(/negative live capacity in list scripts/i)).toBeInTheDocument();
    expect(screen.queryByText('No scripts found')).toBeNull();
  });

  it('shows warmup message and retries until scripts become available', async () => {
    const warmupError = Object.assign(
      new Error('API error: 503 - script cache unavailable; warmup in progress'),
      {
        code: 'warmup_pending',
        status: 503,
        apiMessage: 'script cache unavailable; warmup in progress',
      }
    );

    vi.mocked(api.getScripts)
      .mockRejectedValueOnce(warmupError)
      .mockResolvedValueOnce(mockScriptsResponse);

    render(<ScriptsPage />);

    await waitFor(() => {
      expect(screen.getByText(/data is being prepared/i)).toBeInTheDocument();
    });

    await waitFor(() => {
      expect(api.getScripts).toHaveBeenCalledTimes(2);
    });

    await waitFor(() => {
      expect(screen.getAllByRole('link', { name: 'SECP256K1_BLAKE160' })[0]).toBeInTheDocument();
    });

    expect(screen.queryByText(/data is being prepared/i)).toBeNull();
  });
});
