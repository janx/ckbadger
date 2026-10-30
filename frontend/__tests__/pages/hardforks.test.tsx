import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { stubChartWidth } from '../utils/chart-width';
import producerMainnet from '@/__tests__/fixtures/network-api-producer/hardforks-mainnet.json';
import producerDeployments from '@/__tests__/fixtures/network-api-producer/hardforks-deployments.json';

const getHardforksMock = vi.hoisted(() => vi.fn());
// F2 charts: the time axis runs from the genesis block's header time to the indexed tip's.
const getBlockMock = vi.hoisted(() => vi.fn());

vi.mock('@/lib/api', () => ({
  api: {
    getHardforks: getHardforksMock,
    getBlock: getBlockMock,
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

// R11 (CH-41): the board's live references per hash type. The captured producer payloads predate
// R11, so the tests add the lanes the API now always sends.
const lanes = [
  { hashType: 'type', knownVersions: 59, liveReferences: '1830780' },
  { hashType: 'data1', knownVersions: 19, liveReferences: '98551' },
  { hashType: 'data2', knownVersions: 0, liveReferences: '0' },
  { hashType: 'data', knownVersions: 1, liveReferences: '472' },
];
const features = [
  { rfc: '0028', activationEpoch: 5414 },
  { rfc: '0029', activationEpoch: 0 },
  { rfc: '0030', activationEpoch: 0 },
  { rfc: '0031', activationEpoch: 0 },
  { rfc: '0032', activationEpoch: 5414 },
  { rfc: '0036', activationEpoch: 0 },
  { rfc: '0038', activationEpoch: 0 },
  { rfc: '0048', activationEpoch: 12293 },
  { rfc: '0049', activationEpoch: 12293 },
];
const mockTimeline = {
  network: 'mainnet',
  hashTypeLanes: lanes,
  consensusFeatures: features,
  scriptDeployments: [],
  tipEpoch: 13000,
  tipBlock: 19000000,
  events: [
    {
      id: 'meepo-2024',
      name: 'CKB Edition Meepo',
      shortName: 'Meepo',
      editionYear: 2024,
      activationEpoch: 12293,
      activationDate: '2025-07-01',
      activationBlock: 18430000,
      status: 'activated' as const,
      summary: 'VM v2 activation with Spawn syscall.',
      resources: [{ label: 'CKB2023', url: 'https://example.com/ckb2023' }],
    },
    {
      id: 'mirana-2021',
      name: 'CKB Edition Mirana',
      shortName: 'Mirana',
      editionYear: 2021,
      activationEpoch: 5414,
      activationDate: '2022-05-10',
      activationBlock: 8775638,
      status: 'activated' as const,
      summary: 'VM v1 activation and consensus patches.',
      resources: [{ label: 'CKB2021', url: 'https://example.com/ckb2021' }],
    },
  ],
};

describe('HardforksPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    stubChartWidth(1200);
    getBlockMock.mockImplementation(async (id: string | number) => ({
      timestamp: String(id) === '0' ? '2019-11-15T21:49:50.812Z' : '2026-10-30T00:00:00.000Z',
    }));
  });

  it('renders the mainnet consensus producer payload at its unsynced tip', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue({ ...producerMainnet, hashTypeLanes: lanes });
    render(<HardforksPage />);
    expect(await screen.findByText('CKB Edition Mirana')).toBeInTheDocument();
    expect(screen.getAllByText('RFC 0049').length).toBeGreaterThan(0);
    expect(screen.getByText(/mainnet · tip epoch 0/)).toBeInTheDocument();
    // No activation block indexed yet: no block link is invented.
    expect(screen.queryByRole('link', { name: /^block #/ })).not.toBeInTheDocument();
  });

  it('renders timeline rows from API data', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue(mockTimeline);

    render(<HardforksPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Hardforks' })).toBeInTheDocument();

    await waitFor(() => {
      expect(getHardforksMock).toHaveBeenCalled();
      expect(screen.getByText('CKB Edition Meepo')).toBeInTheDocument();
    });

    expect(screen.getByText(/mainnet · tip epoch 13,000/)).toBeInTheDocument();
    expect(screen.getAllByText('RFC 0032').length).toBeGreaterThan(0);
    expect(screen.getByText('CKB Edition Mirana')).toBeInTheDocument();
    expect(screen.getAllByText('activated').length).toBeGreaterThanOrEqual(2);
    // The board's card line: "epoch 5,414 · block #8,775,638 · 2022-05-10" (CH-42).
    expect(screen.getByRole('link', { name: 'block #18,430,000' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/18430000'
    );
    expect(screen.getByRole('link', { name: 'block #8,775,638' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/8775638'
    );
    expect(screen.getByRole('link', { name: 'CKB2023 overview ↗' })).toHaveAttribute(
      'href',
      'https://example.com/ckb2023'
    );
  });

  it('shows the producer-backed script deployment mark and exact evidence links', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue({ ...producerDeployments, hashTypeLanes: lanes });
    render(<HardforksPage />);

    const mark = await screen.findByRole('button', { name: /Known Lock v2.*block #10/i });
    fireEvent.focus(mark);
    expect(
      within(
        screen.getByRole('complementary', { name: 'Selected script deployment evidence' })
      ).getByText(/earliest indexed code-cell creation/i)
    ).toBeInTheDocument();
    // Header time in the site's one UTC+8 clock form, not a raw ISO string (#16).
    expect(screen.getByText('2023-11-15 06:13:20 UTC+8')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Known Lock family' })).toHaveAttribute(
      'href',
      '/mainnet/scripts/Known%20Lock'
    );
    expect(screen.getByRole('link', { name: 'Code cell 0x55555555…:0' })).toHaveAttribute(
      'href',
      '/mainnet/cell/0x5555555555555555555555555555555555555555555555555555555555555555-0'
    );
    expect(screen.getByRole('link', { name: 'Block #10' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/10'
    );
  });

  it('shows error state on fetch failure', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockRejectedValue(new Error('network down'));

    render(<HardforksPage />);

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Failed to load hardfork timeline');
    });
  });

  // --- F2 charts group: the Hardforks board ---
  it('opens with the plain lead and draws the VM lanes as a timeline, not a table (CH-43, CH-40)', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue(mockTimeline);
    render(<HardforksPage />);
    const timeline = await screen.findByRole('img', { name: /which VM each hash type runs/i });
    expect(
      screen.getByText(
        /CKB changes its rules in editions\. Each one switches on at an agreed epoch/
      )
    ).toBeInTheDocument();
    expect(screen.queryByRole('table')).toBeNull();
    const text = [...timeline.querySelectorAll('text')].map((node) => node.textContent);
    for (const label of [
      'hash_type type',
      'the newest VM',
      'hash_type data1',
      'pinned to VM1',
      'hash_type data2',
      'pinned to VM2',
      'hash_type data',
      'pinned to VM0',
      'genesis',
      'Mirana',
      'Meepo',
      'epoch 5,414 · 2022-05-10',
      'epoch 12,293 · 2025-07-01',
      'not valid before Mirana',
      'not valid before Meepo',
      'no known script yet',
      'live references',
      'now · epoch 13,000',
      '2020',
      '2026',
    ])
      expect(text).toContain(label);
    expect(text.filter((t) => t === 'VM2')).toHaveLength(2);
  });

  it('puts live references and known versions per hash type on the right (CH-41)', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue(mockTimeline);
    render(<HardforksPage />);
    const timeline = await screen.findByRole('img', { name: /which VM each hash type runs/i });
    const text = [...timeline.querySelectorAll('text')].map((node) => node.textContent);
    for (const label of [
      '1,830,780',
      '59 known versions',
      '98,551',
      '19 known versions',
      '0 known versions',
      '472',
      '1 known version',
    ])
      expect(text).toContain(label);
    expect(
      screen.getByText(
        /Live cells reference registered scripts 1,830,780 times by type hash, 98,551 by data1, 0 by data2 and 472 by data\./
      )
    ).toBeInTheDocument();
  });

  it('tells each edition in plain words with its RFC rows and overview link (CH-42)', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue(mockTimeline);
    render(<HardforksPage />);
    const mirana = await screen.findByRole('article', { name: 'CKB Edition Mirana' });
    expect(mirana).toHaveTextContent(/Added CKB-VM version 1 and the hash type data1 to pin it/);
    expect(mirana).toHaveTextContent(
      'RFC 0028relative since by timestamp uses the median time of past blocks'
    );
    expect(mirana).toHaveTextContent('RFC 0032scripts choose their VM version through hash_type');
    expect(mirana).not.toHaveTextContent('RFC 0049');
    const meepo = screen.getByRole('article', { name: 'CKB Edition Meepo' });
    expect(meepo).toHaveTextContent(/with the Spawn syscalls/);
    expect(meepo).toHaveTextContent('No script CKBadger knows uses data2 yet.');
    expect(meepo).toHaveTextContent('RFC 0049CKB-VM version 2');
    expect(
      screen.getByText(
        "Today's node applies RFC 0029, 0030, 0031, 0036 and 0038 from epoch 0, so they do not show as switching on with an edition."
      )
    ).toBeInTheDocument();
  });

  it('places deployments on the time axis without a scroll box, same names told apart (CH-40, CH-44)', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue({ ...producerDeployments, hashTypeLanes: lanes });
    render(<HardforksPage />);
    const marks = await screen.findByRole('list', {
      name: 'Known script-version deployment timeline',
    });
    expect(marks.className).not.toMatch(/overflow-y-auto|max-h-/);
    const buttons = within(marks).getAllByRole('button');
    expect(buttons.map((b) => b.getAttribute('aria-label'))).toEqual([
      expect.stringMatching(/^Known Lock v2 · Known Lock · version 0x[0-9a-f]{8}…/),
      expect.stringMatching(/^Known Lock v3 · Known Lock · version 0x[0-9a-f]{8}…/),
    ]);
  });

  it('refuses a timeline without the hash-type lanes instead of drawing empty ones', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    const { hashTypeLanes: _omitted, ...withoutLanes } = mockTimeline;
    getHardforksMock.mockResolvedValue(withoutLanes);
    render(<HardforksPage />);
    expect(await screen.findByRole('alert')).toHaveTextContent(/no hash-type lanes/);
  });
});
