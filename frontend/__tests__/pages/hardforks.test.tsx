import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { stubChartWidth } from '../utils/chart-width';
import { expectTier1Budget } from '../utils/hierarchy';
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
// R-10: the timeline's ends ride on /hardforks (the captured producer payloads predate them).
const ends = {
  genesisTimestampMs: Date.parse('2019-11-15T21:49:50.812Z'),
  tipTimestampMs: Date.parse('2026-10-30T00:00:00.000Z'),
};
const mockTimeline = {
  ...ends,
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
    // R-10: the page must not read /blocks/0 or /blocks/{tip} for the timeline's ends any more.
    getBlockMock.mockRejectedValue(new Error('the hardforks page read a block'));
  });

  it('renders the mainnet consensus producer payload at its unsynced tip', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue({ ...producerMainnet, ...ends, hashTypeLanes: lanes });
    render(<HardforksPage />);
    expect(await screen.findByText('CKB Edition Mirana')).toBeInTheDocument();
    expect(screen.getAllByText('RFC 0049').length).toBeGreaterThan(0);
    expect(screen.getByText(/mainnet · tip epoch 0/)).toBeInTheDocument();
    // No activation block indexed yet: no block link is invented.
    expect(screen.queryByRole('link', { name: /^block #/ })).not.toBeInTheDocument();
  });

  it('draws the VM timeline from the response’s genesis and tip times, reading no block (R-10)', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue(mockTimeline);
    render(<HardforksPage />);
    expect(
      await screen.findByRole('img', { name: /Which VM each hash type runs/ }, { timeout: 5000 })
    ).toBeInTheDocument();
    expect(getBlockMock).not.toHaveBeenCalled();
  });

  it('says the index has no header yet when both ends are null (R-10)', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue({
      ...mockTimeline,
      genesisTimestampMs: null,
      tipTimestampMs: null,
    });
    render(<HardforksPage />);
    expect(
      await screen.findByText(
        'The index holds no block header yet; the timeline draws once it does.'
      )
    ).toBeInTheDocument();
    expect(getBlockMock).not.toHaveBeenCalled();
  });

  it('renders timeline rows from API data', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue(mockTimeline);

    render(<HardforksPage />);

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
    getHardforksMock.mockResolvedValue({
      ...producerDeployments,
      ...ends,
      hashTypeLanes: lanes,
      scriptDeployments: producerDeployments.scriptDeployments.map((d) => ({
        ...d,
        hashTypes: [],
      })),
    });
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
    // Hierarchy v3 (H1): the plain lead is tier 2's opening paragraph now.
    expect(
      within(document.querySelector('[data-tier="2"]') as HTMLElement).getByText(
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
    // Hierarchy v3 (H2): the sentence with every lane's count is tier 2's.
    expect(
      within(document.querySelector('[data-tier="2"]') as HTMLElement).getByText(
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
    // R-10: the tip time arrives with the timeline, so the story counts the days since Meepo (CH-42).
    expect(meepo).toHaveTextContent('No script CKBadger knows uses data2 yet, 486 days on.');
    expect(meepo).toHaveTextContent('RFC 0049CKB-VM version 2');
    expect(
      screen.getByText(
        "Today's node applies RFC 0029, 0030, 0031, 0036 and 0038 from epoch 0, so they do not show as switching on with an edition."
      )
    ).toBeInTheDocument();
  });

  it('places deployments on the time axis without a scroll box, same names told apart (CH-40, CH-44)', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue({
      ...producerDeployments,
      ...ends,
      hashTypeLanes: lanes,
      scriptDeployments: producerDeployments.scriptDeployments.map((d) => ({
        ...d,
        hashTypes: [],
      })),
    });
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

  it('dates the first data1 script and the wait for data2 from the deployments (CH-42, R2b-b)', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    const base = producerDeployments.scriptDeployments[0];
    getHardforksMock.mockResolvedValue({
      ...mockTimeline,
      scriptDeployments: [
        {
          ...base,
          versionName: 'Omni Lock',
          versionHash: `0x${'c'.repeat(64)}`,
          timestampMs: Date.parse('2024-02-01T00:00:00+08:00'),
          hashTypes: ['data1'],
        },
        {
          ...base,
          versionName: 'xUDT',
          versionHash: `0x${'b'.repeat(64)}`,
          timestampMs: Date.parse('2024-01-08T12:00:00+08:00'),
          hashTypes: ['data1'],
        },
      ],
    });
    render(<HardforksPage />);
    const mirana = await screen.findByRole('article', { name: 'CKB Edition Mirana' });
    expect(mirana).toHaveTextContent(
      'The first known script pinned with data1 came 608 days later: xUDT.'
    );
    const meepo = screen.getByRole('article', { name: 'CKB Edition Meepo' });
    // The days count waits for the tip block's header time.
    await waitFor(() =>
      expect(meepo).toHaveTextContent('No script CKBadger knows uses data2 yet, 486 days on.')
    );
  });

  it('refuses deployments without their registered hash types', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue({ ...producerDeployments, ...ends, hashTypeLanes: lanes });
    render(<HardforksPage />);
    expect(await screen.findByRole('alert')).toHaveTextContent(/has no registered hash types/);
  });

  // --- Hierarchy v3 (lane H, plan §1b R11–R16) ---
  it('opens with the title row and one short lede, then the VM timeline as the display element (H1, R13)', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue(mockTimeline);
    const { container } = render(<HardforksPage />);
    const timeline = await screen.findByRole('img', { name: /which VM each hash type runs/i });
    const tier1 = container.querySelector('[data-tier="1"]') as HTMLElement;
    const hero = screen.getByRole('region', { name: 'Hardforks' });
    // The hero is tier 1's first section: title row, lede and the picture, nothing above it.
    expect(tier1.querySelector('section')).toBe(hero);
    expect(tier1.firstElementChild).toBe(hero);
    const h1 = within(hero).getByRole('heading', { level: 1, name: 'Hardforks' });
    expect(h1).toHaveClass('text-index', 'text-text-bright');
    // "mainnet · tip epoch N" sits on the h1's row.
    expect(h1.parentElement).toContainElement(within(hero).getByText('mainnet · tip epoch 13,000'));
    const lede = within(hero).getByText(
      /^Editions change CKB's rules; a script's hash type picks its VM\.$/
    );
    expect(lede.textContent!.length).toBeLessThanOrEqual(90);
    expect(lede).toHaveClass('text-body', 'text-text-dim', 'max-w-[560px]');
    // No figure: the timeline follows the lede directly, with no h2 of its own (it is the hero).
    expect(hero).toContainElement(timeline);
    expect(lede.compareDocumentPosition(timeline) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(within(hero).queryAllByRole('heading', { level: 2 })).toHaveLength(0);
    expect(screen.queryByRole('heading', { name: 'Which VM runs a script, over time' })).toBeNull();
    expect(Number(timeline.getAttribute('height'))).toBeGreaterThanOrEqual(280);
    // Its name and its hint line are inside the plot, top-left; no figcaption.
    expect(timeline.querySelector('[data-role="chart-name"]')).toHaveTextContent(
      'which VM runs a script, over time · dots: when each known script version was deployed'
    );
    expect(tier1.querySelector('figcaption')).toBeNull();
    // "tap or hover" once in the hero (R14): the readout row's.
    expect(hero.querySelectorAll('[data-hint="tap-or-hover"]')).toHaveLength(1);
    expect(hero.textContent!.match(/tap or hover/gi)).toHaveLength(1);
  });

  it('keeps the title row and the lede while the timeline loads or fails (H1)', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockRejectedValue(new Error('network down'));
    render(<HardforksPage />);
    const hero = screen.getByRole('region', { name: 'Hardforks' });
    expect(within(hero).getByRole('heading', { level: 1, name: 'Hardforks' })).toBeInTheDocument();
    expect(within(hero).getByText(/^Editions change CKB's rules/)).toBeInTheDocument();
    expect(await within(hero).findByRole('alert')).toHaveTextContent(
      'Failed to load hardfork timeline'
    );
  });

  it('moves the whole opening paragraph to tier 2 "How hash_type picks a VM" (H1)', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue(mockTimeline);
    const { container } = render(<HardforksPage />);
    await screen.findByRole('img', { name: /which VM each hash type runs/i });
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    expect(within(tier2).getByRole('heading', { name: 'How hash_type picks a VM' })).toHaveClass(
      'text-body'
    );
    expect(tier2).toHaveTextContent(
      "CKB changes its rules in editions. Each one switches on at an agreed epoch, and every node must run it from there on. Both so far brought a new version of CKB-VM, the machine that runs scripts, and a cell's scripts choose which one runs them through their hash type."
    );
    const tier1 = container.querySelector('[data-tier="1"]') as HTMLElement;
    expect(tier1).not.toHaveTextContent(/Each one switches on at an agreed epoch/);
  });

  it('says under the timeline, in one caption, what a hash type pins; the full text is tier 2 (H2)', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue(mockTimeline);
    const { container } = render(<HardforksPage />);
    await screen.findByRole('img', { name: /which VM each hash type runs/i });
    const tier1 = container.querySelector('[data-tier="1"]') as HTMLElement;
    const hero = screen.getByRole('region', { name: 'Hardforks' });
    // Footnote budget under the graphic: its readout row (inside the hero) and ONE caption.
    const caption = within(tier1).getByText(
      /^A script named by its type hash follows every upgrade; one named by its data hash stays on the VM its hash type pins\./
    );
    expect(hero.nextElementSibling).toBe(caption);
    expect(caption.tagName).toBe('P');
    expect(caption).toHaveClass('text-body', 'text-text', 'max-w-[640px]');
    expect(caption.textContent!.length).toBeLessThanOrEqual(160);
    // The way down to the whole explanation.
    const how = within(caption).getByRole('link', { name: 'How it works ↓' });
    expect(how).toHaveAttribute('href', '#how-hash-type-picks-a-vm');
    const target = container.querySelector('#how-hash-type-picks-a-vm') as HTMLElement;
    expect(target).toContainElement(container.querySelector('[data-tier="2"]') as HTMLElement);
    // The 4-line paragraph left tier 1 whole and reads in full in tier 2.
    expect(tier1).not.toHaveTextContent(/data-pinned script runs/);
    expect(tier1).not.toHaveTextContent(/Live cells reference registered scripts/);
    expect(target).toHaveTextContent(
      'A script named by its type hash follows every upgrade: the same code runs on VM0, then VM1, then VM2. A script named by its data hash stays on the VM its hash type pins, so a hardfork never changes how an existing data-pinned script runs. Live cells reference registered scripts 1,830,780 times by type hash, 98,551 by data1, 0 by data2 and 472 by data.'
    );
  });

  it('sets each edition card as title, date line and at most two sentences; RFC rows and links stay (H3)', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue(mockTimeline);
    const { container } = render(<HardforksPage />);
    const mirana = await screen.findByRole('article', { name: 'CKB Edition Mirana' });
    const meepo = screen.getByRole('article', { name: 'CKB Edition Meepo' });
    const story = (card: HTMLElement, opening: RegExp) => {
      const p = within(card).getByText(opening);
      expect(p.tagName).toBe('P');
      expect(p).toHaveClass('text-body', 'max-w-[640px]');
      expect(p.textContent!.length).toBeLessThanOrEqual(160);
      expect(p.textContent!.match(/[.!?](?=\s|$)/g)!.length).toBeLessThanOrEqual(2);
      return p;
    };
    expect(story(mirana, /^Added CKB-VM version 1/)).toHaveTextContent(
      /^Added CKB-VM version 1 and the hash type data1 to pin it\.$/
    );
    expect(story(meepo, /^Added CKB-VM version 2/)).toHaveTextContent(
      /^Added CKB-VM version 2, with the Spawn syscalls, and the hash type data2\. No script CKBadger knows uses data2 yet, 486 days on\.$/
    );
    for (const card of [mirana, meepo]) {
      expect(within(card).getByRole('heading', { level: 3 })).toHaveClass('text-section');
      // The date line is meta; the status beside the title is plain ink, not jade (R11).
      expect(within(card).getByText(/^epoch [\d,]+ ·/)).toHaveClass(
        'text-meta',
        'text-text-dim',
        'self-start'
      );
      expect(within(card).getByText('activated')).toHaveClass('text-text-dim');
      expect(card).not.toHaveTextContent(/Scripts named by type hash moved/);
    }
    // RFC rows and links unchanged, the links in plain ink (R11).
    expect(meepo).toHaveTextContent('RFC 0049CKB-VM version 2');
    const overview = within(meepo).getByRole('link', { name: 'CKB2023 overview ↗' });
    expect(overview).toHaveAttribute('href', 'https://example.com/ckb2023');
    expect(overview).toHaveClass('text-text');
    // What left the cards reads in tier 2, after the paragraph on editions it continues.
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    const moved = within(tier2).getByText(
      "Scripts named by type hash moved to VM1 at Mirana's epoch, 5,414, and to VM2 at Meepo's, 12,293. Meepo's Spawn syscalls let scripts start and talk to other scripts."
    );
    expect(moved.previousElementSibling).toHaveTextContent(/^CKB changes its rules in editions\./);
  });

  it('keeps the consensus grid as it is, its closing sentence at meta, sections 64 and 72 px apart (H3, R6)', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue(mockTimeline);
    render(<HardforksPage />);
    const consensus = await screen.findByRole('region', {
      name: "As this node's consensus states it",
    });
    // The DAO page's section head: the h2 in its own row, 18 px above the evidence, so the
    // heading's box is its words, not the column (R6; the R16 text-area rule counts boxes).
    const h2 = within(consensus).getByRole('heading', { level: 2 });
    expect(h2).toHaveClass('text-section', 'text-text-bright');
    expect(h2.parentElement).toHaveClass('flex', 'mb-[18px]');
    expect(h2.parentElement!.parentElement).toBe(consensus);
    // One-line source label: as wide as its words.
    expect(within(consensus).getByText('get_consensus · hardfork_features · mainnet')).toHaveClass(
      'text-small',
      'w-fit'
    );
    const cells = consensus.querySelectorAll('dl > div');
    expect(cells).toHaveLength(9);
    expect(cells[0]).toHaveTextContent('RFC 0028epoch 5,414');
    expect(
      within(consensus).getByText(
        "Today's node applies RFC 0029, 0030, 0031, 0036 and 0038 from epoch 0, so they do not show as switching on with an edition."
      )
    ).toHaveClass('text-meta', 'text-text-dim', 'max-w-[640px]');
    // Rhythm: hero → caption → the editions 64 px under it → the consensus 72 px under them.
    const hero = screen.getByRole('region', { name: 'Hardforks' });
    const caption = hero.nextElementSibling as HTMLElement;
    const editions = caption.nextElementSibling as HTMLElement;
    expect(editions).toContainElement(screen.getByRole('article', { name: 'CKB Edition Mirana' }));
    expect(editions).toHaveClass('mt-16');
    expect(editions.nextElementSibling).toBe(consensus);
    expect(consensus).toHaveClass('mt-[72px]');
  });

  it('writes no coloured text in tier 1: no figure, so no gold, jade or rouge words (R11)', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue({
      ...producerDeployments,
      ...ends,
      hashTypeLanes: lanes,
      consensusFeatures: features,
      scriptDeployments: producerDeployments.scriptDeployments.map((d) => ({
        ...d,
        hashTypes: [],
      })),
    });
    const { container } = render(<HardforksPage />);
    // A focused dot opens its evidence links in tier 1: they too are plain ink.
    fireEvent.focus(await screen.findByRole('button', { name: /Known Lock v2.*block #10/i }));
    expect(screen.getByRole('link', { name: 'Block #10' })).toBeInTheDocument();
    const tier1 = container.querySelector('[data-tier="1"]') as HTMLElement;
    const COLOURED =
      /^text-(jade|emphasis|positive|negative|gold|warning|rouge|aqua|info|interactive)(-dim|-bright)?(\/\d+)?$/;
    const coloured = Array.from(tier1.querySelectorAll('[class]')).filter((el) =>
      el
        .getAttribute('class')!
        .split(/\s+/)
        .some((c) => COLOURED.test(c))
    );
    expect(coloured.map((el) => `${el.tagName} ${el.textContent}`)).toEqual([]);
    for (const t of Array.from(tier1.querySelectorAll('svg text, svg tspan')))
      expect((t.getAttribute('fill') ?? '').toLowerCase()).not.toMatch(
        /var(--c-jade)|var(--c-gold)|var(--c-rouge)/
      );
  });

  it('keeps tier 1 within the prose budget: at most three caption-length blocks (§0, §4)', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    const base = producerDeployments.scriptDeployments[0];
    getHardforksMock.mockResolvedValue({
      ...mockTimeline,
      scriptDeployments: [
        {
          ...base,
          versionName: 'xUDT',
          versionHash: `0x${'b'.repeat(64)}`,
          timestampMs: Date.parse('2024-01-08T12:00:00+08:00'),
          hashTypes: ['data1'],
        },
      ],
    });
    const { container } = render(<HardforksPage />);
    await screen.findByRole('article', { name: 'CKB Edition Mirana' });
    expectTier1Budget(container, { maxProseBlocks: 3, maxChars: 160 });
  });
});
