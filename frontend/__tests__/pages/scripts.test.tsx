import { CHARTS } from '@/components/charts/chart-catalogue';
import { renderPaused } from '@/__tests__/utils/paused-read';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ScriptsPage from '@/app/scripts/page';
import { api, KnownScript } from '@/lib/api';
import { pagedRead } from '@/__tests__/utils/paged-read';

vi.mock('@/lib/api', () => ({
  api: {
    getScripts: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockScriptsResponse = pagedRead({
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
      firstDeployedAt: null,
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
      firstDeployedAt: null,
    } satisfies KnownScript,
  ],
  total: 2,
  limit: 50,
  hasMore: false,
  nextCursor: null,
});

describe('ScriptsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // Lane L (charts ↔ topics).
  it('links the chart drawn from scripts (lane L)', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScriptsResponse);
    render(<ScriptsPage />);
    const charts = await screen.findByRole('navigation', { name: 'Charts of these numbers' });
    const hrefs = within(charts)
      .getAllByRole('link')
      .map((a) => a.getAttribute('href'));
    expect(hrefs).toEqual(
      CHARTS.filter((c) => c.evidenceHref === '/scripts').map((c) => `/mainnet/charts/${c.slug}`)
    );
  });

  it('offers name or code-hash search (SC-L5, R2b-b)', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScriptsResponse);

    render(<ScriptsPage />);

    // lowercase, as the header's field (U-01, R2)
    expect(screen.getByPlaceholderText('search by name or code hash')).toBeInTheDocument();
  });

  it('uses server role totals and refetches the filtered family set', async () => {
    vi.mocked(api.getScripts).mockResolvedValue({
      ...mockScriptsResponse,
      classSummary: [
        {
          class: 'fixed',
          lockCellsCount: 2,
          lockOwnedCapacitySum: '3000000000',
          referenceCount: 1,
          familyCount: 1,
        },
      ],
      roleCounts: { all: 2, locks: 1, types: 1, both: 0, neither: 0 },
    });
    render(<ScriptsPage />);
    // Re-pinned on purpose (controls lane C2): the role row is the controls board's tab row, a
    // tablist whose names are lowercase (it was the aria-pressed button "Types 1").
    expect(await screen.findByRole('tab', { name: 'types 1' })).toBeInTheDocument();
    expect(
      screen.getByRole('img', { name: 'CKB under lock scripts by who can change their code' })
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole('tab', { name: 'types 1' }));
    await waitFor(() =>
      expect(api.getScripts).toHaveBeenLastCalledWith(
        expect.objectContaining({ role: 'type', cursor: undefined })
      )
    );
  });

  it('renders script list columns and sorts through one control', async () => {
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

    expect(
      (await screen.findAllByRole('link', { name: 'SECP256K1_BLAKE160' }))[0]
    ).toBeInTheDocument();
    expect(screen.getAllByText('lock')[0]).toBeInTheDocument();
    // Re-pinned on purpose (controls lane C2): the controls board's select, named by "sorted by"
    // (was the aria-label "Sort scripts"), its choices in the page's words, lowercase (they were
    // "CKB in their cells", "Occupied CKB", "Live cells", "Cells ever", "Name").
    const sort = screen.getByRole('combobox', { name: 'sorted by' });
    for (const option of ['ckb in their cells', 'occupied ckb', 'live cells', 'cells ever', 'name'])
      expect(within(sort).getByRole('option', { name: option })).toBeInTheDocument();
    expect(screen.queryByText('Deployed')).toBeNull();
    expect(screen.queryByRole('link', { name: '#0' })).toBeNull();
    expect(screen.queryByRole('link', { name: '#100' })).toBeNull();
    expect(screen.queryByText('Utilization Ratio')).toBeNull();
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

    expect(screen.getAllByText('deprecated').length).toBeGreaterThan(0);
  });

  it('supports sorting by occupied CKB', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScriptsResponse);

    render(<ScriptsPage />);

    // Re-pinned on purpose (controls lane C2): the controls board's select, named by the label
    // it stands after, "sorted by" (it was the aria-label "Sort scripts").
    fireEvent.change(await screen.findByRole('combobox', { name: 'sorted by' }), {
      target: { value: 'used' },
    });

    await waitFor(() => {
      expect(api.getScripts).toHaveBeenLastCalledWith(
        expect.objectContaining({
          limit: 50,
          sortKey: 'used',
          sortDirection: 'desc',
        })
      );
    });
    fireEvent.click(screen.getByRole('button', { name: 'Reverse sort direction' }));
    await waitFor(() =>
      expect(api.getScripts).toHaveBeenLastCalledWith(
        expect.objectContaining({ sortKey: 'used', sortDirection: 'asc' })
      )
    );
  });

  it('routes Unknown script entries to code-hash detail page', async () => {
    const unknownCodeHash = '0x010445a300000000000000000000000000000000000000000000000000000001';
    const unknownScriptRefLabel = 'Unlabeled';
    // U-09 (re-pinned): the table's reference at the row size
    const unknownScriptRefDisplay = 'type · 0x010445a3…000001';
    const unknownScriptRefFull = `type ref:${unknownCodeHash}`;
    vi.mocked(api.getScripts).mockResolvedValue(
      pagedRead({
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
            firstDeployedAt: null,
          } satisfies KnownScript,
        ],
        total: 1,
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );

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
      `/mainnet/script/${encodeURIComponent(unknownCodeHash)}?hashType=type&kind=type`
    );
    expect(screen.getAllByRole('link', { name: unknownScriptRefLabel })[0]).toHaveAttribute(
      'title',
      unknownScriptRefFull
    );
  });

  it('supports sorting by live cells', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScriptsResponse);
    render(<ScriptsPage />);
    // Re-pinned on purpose (controls lane C2): the controls board's select, named by the label
    // it stands after, "sorted by" (it was the aria-label "Sort scripts").
    fireEvent.change(await screen.findByRole('combobox', { name: 'sorted by' }), {
      target: { value: 'liveCells' },
    });
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

    // Since hierarchy v3 the hero's summary is its own request and states its own failure.
    const list = screen.getByRole('region', { name: 'Script list' });
    expect(within(list).getByText(/negative live capacity in list scripts/i)).toBeInTheDocument();
    expect(
      within(screen.getByRole('region', { name: 'Scripts' })).getByText(
        'Failed to load the script summary'
      )
    ).toBeInTheDocument();
    expect(screen.queryByText('No scripts.')).toBeNull();
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

    // The first request (the table's) meets the warmup; every later one, the hero's included, the data.
    vi.mocked(api.getScripts)
      .mockResolvedValue(mockScriptsResponse)
      .mockRejectedValueOnce(warmupError);

    render(<ScriptsPage />);

    await waitFor(() => {
      expect(screen.getByText(/data is being prepared/i)).toBeInTheDocument();
    });

    // Two requests per page since hierarchy v3: the table's page and the hero's summary (limit 1,
    // no role). The table's request is retried once after the warmup error.
    await waitFor(() => {
      expect(
        vi.mocked(api.getScripts).mock.calls.filter(([params]) => params?.limit === 50)
      ).toHaveLength(2);
    });

    await waitFor(() => {
      expect(screen.getAllByRole('link', { name: 'SECP256K1_BLAKE160' })[0]).toBeInTheDocument();
    });

    expect(screen.queryByText(/data is being prepared/i)).toBeNull();
  });
});

// A1 review fix, round 2: a read paused offline, or in a hidden tab between retries, is still
// awaited; the section says it is loading, never "no …" or "failed", and resumes with the data.
describe('ScriptsPage while its read is paused', () => {
  it('says it is loading, not "No scripts.", then lists the scripts once back online', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScriptsResponse);
    const { resume } = renderPaused(<ScriptsPage />);
    expect(await screen.findByText('Loading scripts…')).toBeInTheDocument();
    expect(screen.queryByText('No scripts.')).toBeNull();
    resume();
    expect((await screen.findAllByText('SECP256K1_BLAKE160')).length).toBeGreaterThan(0);
    expect(screen.queryByText('Loading scripts…')).toBeNull();
  });
});

// The controls lane (C2, c2-lists), written before the role tabs, the sort, the search and the
// pager became the controls board's: what each asks for, and the step to the last page while it
// loads (the list keeps the page it shows until the next one answers).
describe('the scripts list’s pages (what the controls ask for)', () => {
  const script = (n: number): KnownScript => ({
    ...mockScriptsResponse.data[0],
    codeHash: `0x${n.toString(16).padStart(64, '0')}`,
    name: `Script ${n}`,
  });
  const firstPage = () =>
    pagedRead({
      data: Array.from({ length: 50 }, (_, i) => script(i + 1)),
      total: 51,
      limit: 50,
      hasMore: true,
      nextCursor: 'n2',
    });
  const lastPage = () =>
    pagedRead({
      data: [script(51)],
      total: 51,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
  const lastList = () =>
    vi
      .mocked(api.getScripts)
      .mock.calls.filter(([p]) => p?.limit === 50)
      .at(-1)?.[0];

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getScripts).mockImplementation(async (params = {}) =>
      params.cursor === 'n2' ? lastPage() : firstPage()
    );
  });

  const toPageTwo = async () => {
    fireEvent.click(screen.getByRole('button', { name: /^next/i }));
    await waitFor(() => expect(lastList()).toEqual(expect.objectContaining({ cursor: 'n2' })));
    expect((await screen.findAllByText('Script 51')).length).toBeGreaterThan(0);
  };

  it('starts again from the first page when the sort, its direction or the search changes', async () => {
    render(<ScriptsPage />);
    expect((await screen.findAllByText('Script 1')).length).toBeGreaterThan(0);
    await toPageTwo();
    fireEvent.change(screen.getByRole('combobox', { name: /sort/i }), {
      target: { value: 'liveCells' },
    });
    await waitFor(() =>
      expect(lastList()).toEqual(
        expect.objectContaining({ sortKey: 'liveCells', cursor: undefined })
      )
    );
    expect((await screen.findAllByText('Script 1')).length).toBeGreaterThan(0);
    await toPageTwo();
    fireEvent.click(screen.getByRole('button', { name: 'Reverse sort direction' }));
    await waitFor(() =>
      expect(lastList()).toEqual(
        expect.objectContaining({ sortDirection: 'asc', cursor: undefined })
      )
    );
    expect((await screen.findAllByText('Script 1')).length).toBeGreaterThan(0);
    await toPageTwo();
    // re-pinned on purpose (lists lane P3): the search filters as typed (no Search button), and the
    // sort direction is named as the inventory's ("Reverse sort direction"; it was "Reverse order")
    fireEvent.change(screen.getByPlaceholderText('search by name or code hash'), {
      target: { value: 'script' },
    });
    await waitFor(() =>
      expect(lastList()).toEqual(expect.objectContaining({ search: 'script', cursor: undefined }))
    );
  });

  it('keeps showing its page while the last one loads, then shows the last one', async () => {
    let answer: (value: ReturnType<typeof lastPage>) => void = () => {};
    vi.mocked(api.getScripts).mockImplementation((params = {}) =>
      params.cursor === 'n2'
        ? new Promise((resolve) => {
            answer = resolve;
          })
        : Promise.resolve(firstPage())
    );
    render(<ScriptsPage />);
    expect((await screen.findAllByText('Script 50')).length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole('button', { name: /^next/i }));
    await waitFor(() => expect(lastList()).toEqual(expect.objectContaining({ cursor: 'n2' })));
    // the page shown until the next answers: its rows, and no error in their place
    expect(screen.getAllByText('Script 50').length).toBeGreaterThan(0);
    expect(screen.queryByRole('alert')).toBeNull();
    answer(lastPage());
    expect((await screen.findAllByText('Script 51')).length).toBeGreaterThan(0);
    expect(screen.queryByText('Script 50')).toBeNull();
  });
});
