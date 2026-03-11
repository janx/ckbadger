import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ScriptsPage from '@/app/scripts/page';
import { api, KnownScript } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getScripts: vi.fn(),
  },
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
      liveCapacitySum: '2000000000',
      liveUsedCapacitySum: '1000000000',
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
      liveCapacitySum: '10000000000',
      liveUsedCapacitySum: '5000000000',
      liveCellsCount: 1500,
      cellsCount: 3200,
      deployedAt: 100,
    } satisfies KnownScript,
  ],
  total: 2,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

describe('ScriptsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders script list', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScriptsResponse);

    render(<ScriptsPage />);

    await waitFor(() => {
      expect(api.getScripts).toHaveBeenCalledWith(
        expect.objectContaining({
          limit: 20,
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
    expect(screen.getByText('Capacity (CKB)')).toBeInTheDocument();
    expect(screen.queryByText('Utilization Ratio')).toBeNull();
    expect(screen.getByRole('button', { name: 'Sort by Used (CKB)' })).toBeInTheDocument();
  });

  it('supports sorting by occupied capacity', async () => {
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
          limit: 20,
          sortKey: 'used',
          sortDirection: 'desc',
        })
      );
    });

    await waitFor(() => {
      const scriptLinks = screen
        .getAllByRole('link')
        .filter((link) => link.getAttribute('href')?.startsWith('/scripts/'));
      // Dual-render (table + card) means each script appears twice; check first pair order
      const names = scriptLinks.map((el) => el.textContent);
      expect(names.indexOf('ALWAYS_SUCCESS')).toBeLessThan(names.indexOf('SECP256K1_BLAKE160'));
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
          liveCapacitySum: '2000000000',
          liveUsedCapacitySum: '1000000000',
        } satisfies KnownScript,
      ],
      total: 1,
      limit: 20,
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

  it('renders Live Cells and Total Cells sort headers at xl width', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScriptsResponse);
    render(<ScriptsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Sort by Live Cells' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Sort by Total Cells' })).toBeInTheDocument();
    });
  });

  it('renders Deployed column header at xl width', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScriptsResponse);
    render(<ScriptsPage />);
    await waitFor(() => {
      expect(screen.getByText('Deployed')).toBeInTheDocument();
    });
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
});
