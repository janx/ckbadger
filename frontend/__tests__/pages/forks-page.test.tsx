import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderPaused } from '@/__tests__/utils/paused-read';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { expectTier1Budget } from '@/__tests__/utils/hierarchy';
import { atContainerWidth } from '@/__tests__/utils/phone-row';
import { loadUnderStillClock, T0, tick } from '@/__tests__/utils/still-clock';
import { rects, roundedRects } from '@/__tests__/utils/square-corners';
import ForksPage from '@/app/forks/page';
import { api } from '@/lib/api';
import listFixture from '@/__tests__/fixtures/fork-api-producer/fork-list.json';
import overviewFixture from '@/__tests__/fixtures/fork-api-producer/fork-overview.json';
import deepFixture from '@/__tests__/fixtures/fork-api-producer/fork-deep-detail.json';
import { fullText } from '@/__tests__/utils/full-text';
import { MEASURE_CHARS } from '@/lib/type-metrics';

vi.mock('@/lib/api', () => ({
  api: { getForks: vi.fn(), getForkOverview: vi.fn() },
}));
vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

type List = Awaited<ReturnType<typeof api.getForks>>;
const list = () => listFixture as unknown as List;
const overview = () =>
  overviewFixture as unknown as Awaited<ReturnType<typeof api.getForkOverview>>;
const table = () => screen.getByRole('table', { name: 'Branch switches on this node' });

describe('ForksPage', () => {
  beforeEach(() => vi.clearAllMocks());

  it('leads with the uncle rate as the one display figure, the days under it, the node’s record in one quiet row (FF-L1, R11, R13)', async () => {
    vi.mocked(api.getForks).mockResolvedValue(list());
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    const { container } = render(<ForksPage />);
    const hero = screen.getByRole('region', { name: 'Forks' });
    expect(within(hero).getByRole('heading', { level: 1, name: 'Forks' })).toHaveClass(
      'text-index'
    );
    // R13: one short lede, ≤ 90 characters, with the way down to tier 2; in the reading ink, a
    // sentence that explains (re-pinned with readability ruling R3; it was dim)
    const lede = within(hero).getByText(/^Two blocks, one height: one stays, one is an uncle\./);
    // re-pinned with readability R4 (lane B7): on the measure, 76 characters (was max-w-[560px])
    expect(lede).toHaveClass('text-body', 'text-text', 'max-w-measure');
    // one line of the measure with room to spare, as the DAO page's lede: at most 67 characters,
    // where the measure holds MEASURE_CHARS, 76, at the body step (it said 560 px, 8.3 px a
    // character at 13 px)
    expect(lede.textContent!.length).toBeLessThanOrEqual(67);
    expect(within(lede).getByRole('link', { name: 'How it works ↓' })).toHaveAttribute(
      'href',
      '#evidence'
    );
    // the figure: the window's uncle rate, bright (Forks has no class colour, R11), against the target
    const rate = await within(hero).findByText('250.00');
    expect(rate).toHaveClass('sm:text-display', 'text-text-bright');
    expect(within(rate).getByText('%')).toHaveClass('text-hero');
    expect(
      within(hero).getByText(fullText('uncle rate · 3 days through 2026-10-22'))
    ).toBeInTheDocument();
    expect(within(hero).getByText(fullText('5 uncles in 2 blocks · target 2.50%'))).toHaveClass(
      'text-body'
    );
    // the days are the picture, inside the hero section
    expect(hero.querySelectorAll('[data-uncles]')).toHaveLength(3);
    // the node's own record (R2b-b): one quiet row under the reading, outside the hero
    await waitFor(() => expect(screen.getAllByRole('term')).toHaveLength(3));
    expect(screen.getAllByRole('term').map((t) => t.textContent)).toEqual([
      'This node switched branch',
      'Its orphans',
      'Orphaned transactions',
    ]);
    const row = screen.getAllByRole('term')[0].closest('dl')!;
    expect(hero.contains(row)).toBe(false);
    expect(
      within(row)
        .getAllByRole('definition')
        .map((dd) => dd.textContent)
    ).toEqual([
      // re-pinned (A4 round 2): each count a unit, number and noun joined by a plain space
      '1 time · since 2025-08-01',
      '1 uncle · 1 one block later',
      '1 of 1 · committed in a later block',
    ]);
    // R11: no coloured text in tier 1 (the glyphs and bars keep their colours)
    const tier1 = container.querySelector('[data-tier="1"]') as HTMLElement;
    await waitFor(() => expect(table()).toBeInTheDocument());
    const coloured = Array.from(tier1.querySelectorAll('[class]')).filter((el) =>
      el
        .getAttribute('class')!
        .split(/\s+/)
        .some((c) => /^text-(jade|gold|rouge)(\/\d+)?$/.test(c))
    );
    expect(coloured).toEqual([]);
    expectTier1Budget(container, { maxProseBlocks: 3, maxChars: 160 });
    // the full concept sentence lives in tier 2
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    expect(tier2).toHaveTextContent(/aiming at one in 40 blocks\./);
    expect(tier1).not.toHaveTextContent(/aiming at one in 40 blocks/);
    expect(api.getForkOverview).toHaveBeenCalledWith(30);
  });

  it('claims nothing about orphans before the node has switched at all (visual pass, live empty store)', async () => {
    vi.mocked(api.getForks).mockResolvedValue({
      ...list(),
      data: [],
      total: 0,
      hasMore: false,
      nextCursor: null,
    });
    const record = overview().nodeRecord;
    const fates = record.orphanFates!;
    vi.mocked(api.getForkOverview).mockResolvedValue({
      ...overview(),
      nodeSwitchesAllTime: 0,
      nodeSwitchesInWindow: 0,
      days: overview().days.map((day) => ({ ...day, nodeSwitches: 0 })),
      nodeRecord: {
        ...record,
        orphanedBlocks: 0,
        orphanedTransactions: 0,
        orphanFates: {
          ...fates,
          eventsWithEvidence: 0,
          blocks: { ...fates.blocks, total: 0, includedAsUncle: 0, uncleDistances: [] },
          userTransactions: { ...fates.userTransactions, total: 0, committedLater: 0 },
          competing: { compared: 0, sameMiner: 0, minerUnknown: 0, gapMedianMs: null },
        },
      },
    });
    render(<ForksPage />);
    await waitFor(() => expect(screen.getAllByRole('term')).toHaveLength(1));
    expect(screen.queryByText('Its orphans')).toBeNull();
    expect(screen.queryByText(/held only cellbases/)).toBeNull();
    expect(screen.queryByText(/0 of 0 were one miner/)).toBeNull();
  });

  it('leaves out the orphan figures when the node reader is not open, and says so', async () => {
    vi.mocked(api.getForks).mockResolvedValue(list());
    vi.mocked(api.getForkOverview).mockResolvedValue({
      ...overview(),
      nodeRecord: {
        ...overview().nodeRecord,
        evidenceStatus: 'node_reader_unavailable',
        orphanFates: null,
        switchesDeeperThanOneBlock: 2,
      },
    });
    render(<ForksPage />);
    await waitFor(() => expect(screen.getAllByRole('term')).toHaveLength(1));
    expect(screen.getByRole('definition')).toHaveTextContent(
      /^1 time · since 2025-08-01, 2 of them more than one block deep$/
    );
    expect(
      screen.getByText(/what became of the dropped blocks needs the node reader/)
    ).toBeInTheDocument();
  });

  it('draws the days as grey uncle bars with jade switches; one reading and one caption under them, the rest in tier 2 (FF-L2, F3)', async () => {
    vi.mocked(api.getForks).mockResolvedValue(list());
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    const { container } = render(<ForksPage />);
    await waitFor(() => expect(container.querySelectorAll('[data-uncles]')).toHaveLength(3));
    expect(container.querySelectorAll('[data-switches]')).toHaveLength(1);
    // hierarchy v3: the picture has no separate heading
    expect(
      screen.queryByRole('heading', { name: 'The chain’s record and this node’s view' })
    ).toBeNull();
    // under the days: the reading row, then one caption (≤ 2 lines of text, F3)
    const reading = screen.getByRole('status', { name: 'Day readout' });
    const under = reading.closest('[data-role="reading"]') as HTMLElement;
    expect(Array.from(under.children).map((el) => el.tagName)).toEqual(['DIV', 'P']);
    const caption = under.querySelector('p')!;
    // re-pinned with readability R4 (lane B7): on the measure, 76 characters (was max-w-[640px])
    expect(caption).toHaveClass('text-body', 'max-w-measure');
    // one line of the measure: MEASURE_CHARS, 76 characters at the body step's 8.4 px a
    // glyph (it was length × 8 ≤ 640, which let 77 to 80 characters through)
    expect(caption.textContent!.length).toBeLessThanOrEqual(MEASURE_CHARS);
    // the paragraph and the scope note moved to tier 2, every sentence kept
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    expect(
      within(tier2).getByText(
        fullText(
          /The chain recorded 5 uncles in these 3 days; this node switched branches 1 time in the same days\. It only notices a fork when the losing block reached it first and it had to switch\. The uncles are the complete record, and they are on chain for anyone to recount\./
        )
      )
    ).toBeInTheDocument();
    expect(
      within(tier2).getByText(
        /indexed chain time, node switches use this indexer's detection wall time/
      )
    ).toBeInTheDocument();
    expect(within(tier2).getByText(/focus the chart and use ← →/)).toBeInTheDocument();
    // two balanced columns: the concept and the window's record | how the chart is drawn
    const columns = tier2.querySelector('[data-tier="2"] > div')!.children;
    expect(columns).toHaveLength(2);
    expect(columns[0]).toHaveTextContent(/aiming at one in 40 blocks/);
    expect(columns[1]).toHaveTextContent(/focus the chart and use ← →/);
    expect(columns[1]).toHaveTextContent(/indexer's detection wall time/);
  });

  it('gives every switch a fork glyph, the miners, the gap and its fate, linked by D7 (FF-L3, FF-L4)', async () => {
    vi.mocked(api.getForks).mockResolvedValue(list());
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    await waitFor(() => expect(table()).toBeInTheDocument());
    expect(
      within(table())
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual([
      'Seen (UTC+8)',
      'Fork point',
      'Shape',
      'Dropped block’s miner vs winner’s',
      'Found apart',
      'What became of it',
    ]);
    const seen = within(table()).getByRole('link', { name: '2025-08-01 06:13:20' });
    expect(seen).toHaveAttribute('href', '/mainnet/forks/10/17404795');
    // R14: the list's record note left the h2 row for tier 2; its key stays, in neutral words
    expect(
      within(document.querySelector('[data-tier="2"]') as HTMLElement).getByText(
        '0 of 1 were one miner against itself · median gap 50 ms'
      )
    ).toBeInTheDocument();
    const key = screen.getByRole('list', { name: 'Shape key' });
    expect(key).toHaveTextContent(
      'dashed: the block it dropped→ the block that took it as an uncle'
    );
    expect(key.querySelector('.text-jade')).toBeNull();
    expect(within(table()).getByText('depth 1')).toBeInTheDocument();
    // R15 (toward 32–34 px rows): 6 px above and below the 44 px glyph
    for (const cell of within(table()).getAllByRole('cell'))
      expect(cell).toHaveClass('text-body', 'py-1.5');
    // the heads hold their words in an inline element, and a cell's second line is as wide as
    // its words (R16: a text box is its words)
    for (const head of within(table()).getAllByRole('columnheader'))
      expect(head.firstElementChild?.tagName).toBe('SPAN');
    expect(within(table()).getByText('depth 1')).toHaveClass('block', 'w-fit');
    expect(within(table()).getByText('1 of 1 tx committed later')).toHaveClass('block', 'w-fit');
    expect(table().querySelector('[data-uncle-link]')).not.toBeNull();
    expect(within(table()).getByText('50 ms')).toBeInTheDocument();
    // Table-rows T5 (was R11's bright ink): the fork point is the row's one bright thing, so the
    // fate reads in body ink; the glyph's arrow keeps the jade
    const fate = within(table()).getByText('uncle in #12 (+1)');
    expect(fate).toHaveClass('text-text');
    expect(fate).not.toHaveClass('text-text-bright');
    expect(fate).not.toHaveClass('text-jade');
    expect(within(table()).getByText('1 of 1 tx committed later')).toBeInTheDocument();
    expect(within(table()).getAllByText('witness unavailable')).toHaveLength(2);
    expect(api.getForks).toHaveBeenCalledWith({ cursor: undefined, limit: 50 });
  });

  it('draws the rows’ fork glyphs and the shape key’s block swatch square (U-02, R1)', async () => {
    vi.mocked(api.getForks).mockResolvedValue(list());
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    await waitFor(() => expect(table()).toBeInTheDocument());
    const key = screen.getByRole('list', { name: 'Shape key' });
    expect(rects(key)).toHaveLength(1);
    expect(roundedRects(key)).toEqual([]);
    expect(rects(table()).length).toBeGreaterThanOrEqual(5);
    expect(roundedRects(table())).toEqual([]);
  });

  it('shows the first 5 branch switches, then every one in place under “Show all 18 branch switches ↓” (R15)', async () => {
    const event = list().data[0];
    const rows = Array.from({ length: 18 }, (_, i) => ({ ...event, id: i + 1 }));
    vi.mocked(api.getForks).mockResolvedValue({ ...list(), data: rows, total: 18 });
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    await waitFor(() => expect(table()).toBeInTheDocument());
    // Phase 5 (B8): the list as a command run on every switch it counts and whose view it is;
    // the shape key stays the row's control beside it.
    const heading = screen.getByRole('heading', {
      level: 2,
      name: 'branch switches 18 · this node',
    });
    expect(heading).toHaveClass('prompt', 'text-section', 'text-text-bright');
    expect(heading).toHaveAttribute('id', 'fork-switches-heading');
    expect(heading.lastElementChild).toHaveTextContent(/^18 · this node$/);
    expect(heading.lastElementChild).toHaveClass('text-text-dim');
    expect(heading.nextElementSibling).toBe(screen.getByRole('list', { name: 'Shape key' }));
    const bodyRows = () => table().querySelectorAll('tbody tr');
    // collapsed by default: the first five rows, the expander, no pager yet
    expect(bodyRows()).toHaveLength(5);
    // re-pinned (lane C2 topics): the expander is the controls board's short-list form of the
    // pager, "all 18 branch switches ↓" in celadon words (it was "Show all 18 branch switches ↓"
    // in the reading ink, a disclosure with aria-expanded; the board's is a way to go, the rest of
    // the list, and it goes once taken); the count is a unit, held by no-break spaces
    const more = screen.getByRole('button', { name: 'all 18 branch switches ↓' });
    expect(more).toHaveClass('text-small', 'text-jade');
    expect(more).not.toHaveAttribute('aria-expanded');
    expect(screen.queryByText(fullText(/18 switches/))).toBeNull();
    // expanding is the link: every row, every column, in place. Re-pinned on purpose (readability
    // B5): the eighteen are one page, and one page draws no pager (it said "Showing 1–18 of 18
    // switches"); the count stays in the heading.
    fireEvent.click(more);
    expect(bodyRows()).toHaveLength(18);
    expect(within(table()).getAllByRole('columnheader')).toHaveLength(6);
    expect(screen.queryByRole('button', { name: /^all \d/ })).toBeNull();
    expect(screen.queryByRole('navigation', { name: 'branch switches' })).toBeNull();
    expect(screen.queryByText(/18 switches/)).toBeNull();
    expect(heading).toHaveAccessibleName('branch switches 18 · this node');
  });

  // Lane C2 (topics), written before the expander and the pager are replaced: a list longer than
  // a page shows its first five, then its first page whole, then pages older by the cursor the API
  // served and newer back to the page before, a later page always whole.
  it('pages a list longer than a page by its served cursor once expanded, a later page whole', async () => {
    // the API answers eight a page (fewer rows keep the test quick): fifteen take two pages
    const event = list().data[0];
    const first = Array.from({ length: 8 }, (_, i) => ({ ...event, id: i + 1 }));
    const second = Array.from({ length: 7 }, (_, i) => ({ ...event, id: 9 + i }));
    vi.mocked(api.getForks).mockImplementation(async (params) =>
      params?.cursor === 'after-eight'
        ? { ...list(), data: second, total: 15, limit: 8, hasMore: false, nextCursor: null }
        : { ...list(), data: first, total: 15, limit: 8, hasMore: true, nextCursor: 'after-eight' }
    );
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    // re-pinned (lane C2 topics): the controls board's cursor in time, newest first, and its
    // short-list form (they were the buttons Next and Previous, and "Show all 15 branch
    // switches ↓")
    const older = () => screen.getByRole('button', { name: 'older →' });
    const newer = () => screen.getByRole('button', { name: '← newer' });
    const pager = () => screen.getByRole('navigation', { name: 'branch switches' });
    render(<ForksPage />);
    await waitFor(() => expect(table()).toBeInTheDocument());
    const bodyRows = () => table().querySelectorAll('tbody tr');
    expect(bodyRows()).toHaveLength(5);
    fireEvent.click(screen.getByRole('button', { name: 'all 15 branch switches ↓' }));
    expect(bodyRows()).toHaveLength(8);
    // the page of the pages the served total makes at the served page size; newer is quiet words
    expect(pager()).toHaveTextContent('newest first← newerpage 1 of 2older →');
    expect(within(pager()).queryByRole('button', { name: '← newer' })).toBeNull();
    fireEvent.click(older());
    await waitFor(() => expect(bodyRows()).toHaveLength(7));
    expect(api.getForks).toHaveBeenLastCalledWith({ cursor: 'after-eight', limit: 50 });
    expect(pager()).toHaveTextContent('newest first← newerpage 2 of 2older →');
    expect(within(pager()).queryByRole('button', { name: 'older →' })).toBeNull();
    fireEvent.click(newer());
    await waitFor(() => expect(bodyRows()).toHaveLength(8));
    expect(screen.queryByRole('button', { name: /^all \d/ })).toBeNull();
  });

  it('needs no expander for five switches or fewer', async () => {
    const event = list().data[0];
    const rows = Array.from({ length: 5 }, (_, i) => ({ ...event, id: i + 1 }));
    vi.mocked(api.getForks).mockResolvedValue({ ...list(), data: rows, total: 5 });
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    await waitFor(() => expect(table()).toBeInTheDocument());
    expect(table().querySelectorAll('tbody tr')).toHaveLength(5);
    // re-pinned (lane C2 topics): the short-list form's words, "all … ↓" (was "Show all …")
    expect(screen.queryByRole('button', { name: /^all \d/ })).toBeNull();
    // Re-pinned on purpose (readability B5): five switches are one page, and one page draws no
    // pager (it said "Showing 1–5 of 5 switches"); the count stays in the heading.
    expect(screen.queryByText(/5 switches/)).toBeNull();
    expect(
      screen.getByRole('heading', { level: 2, name: 'branch switches 5 · this node' })
    ).toBeInTheDocument();
  });

  it('names both miners by address tail and marks a fork one miner had against itself (FF-L3)', async () => {
    const miner = `0x${'ab'.repeat(32)}`;
    const address = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqw93hhsj';
    const event = list().data[0];
    vi.mocked(api.getForks).mockResolvedValue({
      ...list(),
      data: [
        {
          ...event,
          preview: {
            ...event.preview!,
            oldMinerLockHash: miner,
            oldMinerAddress: address,
            competingMinerLockHash: miner,
            competingMinerAddress: address,
          },
        },
      ],
    });
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    await waitFor(() => expect(table()).toBeInTheDocument());
    expect(within(table()).getByText('same miner')).toBeInTheDocument();
    // U-09 (re-pinned): the table's unnamed miners at the row size (it was the tail alone); R9
    // (re-pinned): an address's row size is `ckb1…` and its last 8 characters
    const miners = within(table()).getAllByRole('link', { name: 'ckb1…qw93hhsj' });
    expect(miners).toHaveLength(2);
    expect(miners[0]).toHaveAttribute('href', `/mainnet/address/${address}`);
  });

  it('names a miner by its pool when pools.toml does, else by address tail (R-12)', async () => {
    const event = list().data[0];
    const address = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqw93hhsj';
    vi.mocked(api.getForks).mockResolvedValue({
      ...list(),
      data: [
        {
          ...event,
          preview: {
            ...event.preview!,
            oldMinerLockHash: `0x${'ab'.repeat(32)}`,
            oldMinerAddress: address,
            oldMinerPool: 'Example Pool',
            competingMinerLockHash: `0x${'cd'.repeat(32)}`,
            competingMinerAddress: address.replace('w93hhsj', 'q2cdyx8'),
            competingMinerPool: null,
          },
        },
      ],
    });
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    await waitFor(() => expect(table()).toBeInTheDocument());
    expect(within(table()).getByRole('link', { name: 'Example Pool' })).toHaveAttribute(
      'href',
      `/mainnet/address/${address}`
    );
    expect(within(table()).getByRole('link', { name: 'ckb1…qq2cdyx8' })).toBeInTheDocument();
    // T5: the fork point is the row's one bright thing; both miners read in body ink.
    for (const name of ['Example Pool', 'ckb1…qq2cdyx8'])
      expect(within(table()).getByRole('link', { name })).not.toHaveClass('text-text-bright');
  });

  it('links a row from firstOrphanHash even without the node preview', async () => {
    const event = list().data[0];
    vi.mocked(api.getForks).mockResolvedValue({
      ...list(),
      data: [{ ...event, preview: null, previewStatus: 'node_reader_unavailable' }],
    });
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    await waitFor(() => expect(table()).toBeInTheDocument());
    expect(within(table()).getByRole('link', { name: '2025-08-01 06:13:20' })).toHaveAttribute(
      'href',
      '/mainnet/forks/10/17404795'
    );
  });

  it('distinguishes no node switches from an overview load failure', async () => {
    vi.mocked(api.getForks).mockResolvedValue({
      ...list(),
      data: [],
      total: 0,
      hasMore: false,
      nextCursor: null,
    });
    vi.mocked(api.getForkOverview).mockRejectedValue(new Error('missing daily block stats'));
    render(<ForksPage />);
    await waitFor(() =>
      expect(screen.getByRole('alert')).toHaveTextContent('missing daily block stats')
    );
    expect(screen.getByText(/No branch switches observed by this node/)).toBeInTheDocument();
  });

  it('labels a deep detection as no rollback, without a route it cannot have', async () => {
    vi.mocked(api.getForks).mockResolvedValue({
      ...list(),
      data: [{ ...deepFixture.event, previewStatus: 'no_old_branch', preview: null }],
    } as unknown as List);
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    await waitFor(() =>
      expect(within(table()).getByText('No rollback at detection')).toBeInTheDocument()
    );
    expect(within(table()).getByText('indexed #12 → node #13')).toBeInTheDocument();
    expect(within(table()).queryByRole('link', { name: /^20\d\d-/ })).toBeNull();
    // Table-rows T1: why there is no link is a footnote; it left the row for the row's title.
    expect(within(table()).queryByText('nothing was rolled back, so no detail page')).toBeNull();
    expect(within(table()).getAllByRole('row')[1]).toHaveAttribute(
      'title',
      expect.stringContaining('nothing was rolled back, so no detail page')
    );
  });

  // Table-rows T5 / T1: one bright thing per row, the fork point at body; "Seen" is one line in
  // body ink, its age and any missing-address words in the row's title; the gap is a 12 px number.
  // The phone row keeps every line it had.
  it('leads each switch with its fork point; seen is one line, its age in the row title (T5, T1)', async () => {
    vi.mocked(api.getForks).mockResolvedValue(list());
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    await waitFor(() => expect(table()).toBeInTheDocument());
    const row = within(table()).getAllByRole('row')[1];
    const point = within(row).getByRole('link', { name: '#10' });
    expect(point).toHaveClass('text-text-bright');
    expect(point.closest('td')).toHaveClass('text-body');
    expect(within(row).getByText('depth 1')).toHaveClass('text-meta', 'text-text-dim');
    const brights = Array.from(row.querySelectorAll('.text-text-bright'));
    expect(brights).toEqual([point]);
    const seen = within(row).getByRole('link', { name: '2025-08-01 06:13:20' });
    expect(seen).toHaveClass('text-text');
    const seenCell = seen.closest('td')!;
    expect(seenCell).toHaveTextContent(/^2025-08-01 06:13:20$/);
    expect(row.getAttribute('title')).toMatch(
      /^seen 2025-08-01 06:13:20 UTC\+8, .+ ago · fork point #10, depth 1$/
    );
    expect(within(row).getByText('50 ms')).toHaveClass('text-small', 'tabular-nums');
  });
});

// Lane E4: a switch's age, in the table row's title and on the phone row's own line, reads the
// page's one clock, so it moves with no data change; both were written once at render. The clock
// stops with the page.
describe('ForksPage: a switch’s age reads the one clock (E4)', () => {
  const seenAt = (detectedAt: string): List => {
    const served = list();
    return { ...served, data: [{ ...served.data[0], detectedAt }, ...served.data.slice(1)] };
  };
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers({ now: T0 });
    vi.mocked(api.getForks).mockResolvedValue(seenAt(new Date(T0 - 20_000).toISOString()));
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
  });
  afterEach(() => {
    vi.useRealTimers();
    // the phone width's ResizeObserver stub and its getBoundingClientRect spy
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('moves the age in a table row’s title', async () => {
    const { unmount } = render(<ForksPage />);
    const title = () =>
      within(screen.getByRole('table', { name: 'Branch switches on this node' }))
        .getAllByRole('row')[1]
        .getAttribute('title');
    await loadUnderStillClock(
      () => screen.queryByRole('table', { name: 'Branch switches on this node' }) !== null
    );
    expect(title()).toMatch(/ UTC\+8, 20s ago · /);
    tick(1000);
    expect(title()).toMatch(/ UTC\+8, 21s ago · /);
    tick(38_000);
    expect(title()).toMatch(/ UTC\+8, 59s ago · /);
    tick(1000);
    expect(title()).toMatch(/ UTC\+8, 1m ago · /);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('moves the age on a phone row’s own line', async () => {
    atContainerWidth(358);
    const { unmount } = render(<ForksPage />);
    const age = () =>
      within(
        within(
          screen.getByRole('list', { name: 'Branch switches on this node, compact' })
        ).getAllByRole('listitem')[0]
      ).getByText(/^\d+[smh] ago$/).textContent;
    await loadUnderStillClock(
      () => screen.queryByRole('list', { name: 'Branch switches on this node, compact' }) !== null
    );
    expect(age()).toBe('20s ago');
    tick(1000);
    expect(age()).toBe('21s ago');
    tick(38_000);
    expect(age()).toBe('59s ago');
    tick(1000);
    expect(age()).toBe('1m ago');
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});

// Readability F3-e: every column of the branch switches budgeted from the widest value it holds,
// at 0.6 em a glyph (the row's 14 px, the gap's 13, a meta line's 12) and the 16 px gap after
// every column but the last. The miners are the flexible column (a pool name has no bound): two
// unnamed miners on one line at its minimum, the rest of the container beyond the sum.
describe('ForksPage · branch switches budgeted from what they hold (readability F3-e)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getForks).mockResolvedValue(list());
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const at = (text: string, px: number) => (text.length * px * 3) / 5; // 0.6 em, as textPx
  const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
  const budgets = [
    // when this node saw it, to the second on the UTC+8 clock
    ceil(at('2026-10-22 22:43:55', 14) + 16),
    // the fork point, eight digits (its depth on a meta line under it)
    ceil(at('#99,999,999', 14) + 16),
    // the fork glyph's 210 px (components/forks/fork-glyph.tsx)
    ceil(210 + 16),
    // two unnamed miners at the row size and "vs"
    ceil(at('ckb1…4s93hhsj vs ckb1…4s93hhsj', 14) + 16),
    // six digits of milliseconds, wider than the head's words
    ceil(Math.max(at('999,999 ms', 13), at('apart', 12)) + 16),
    // the widest fate on its line, or a deep switch's tips on its meta line; the last column
    ceil(
      Math.max(
        at('uncle inclusion still possible', 14),
        at('indexed #99,999,999 → node #99,999,999', 12)
      )
    ),
  ];

  it('pins every column to its content and changes form at their sum, 1,147 px', async () => {
    expect(budgets).toEqual([176, 109, 226, 268, 94, 274]);
    const sum = budgets.reduce((s, w) => s + w, 0);
    expect(sum).toBe(1147);
    const { unmount } = render(<ForksPage />);
    const shown = await screen.findByRole('table', { name: 'Branch switches on this node' });
    const cols = Array.from(shown.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
    expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual(budgets);
    expect(cols[3].width).toBe('auto');
    expect(Number.parseFloat(shown.style.minWidth)).toBe(sum);
    unmount();
    for (const [width, form] of [
      [1232, 'table'],
      [1147, 'table'],
      [1146, 'rows'],
      [608, 'rows'],
      [358, 'rows'],
    ] as const) {
      vi.mocked(api.getForks).mockResolvedValue(list());
      vi.mocked(api.getForkOverview).mockResolvedValue(overview());
      atContainerWidth(width);
      const { unmount: drop } = render(<ForksPage />);
      const listed = await screen.findByRole(form === 'table' ? 'table' : 'list', {
        name:
          form === 'table'
            ? 'Branch switches on this node'
            : 'Branch switches on this node, compact',
      });
      expect(listed.closest('[data-list-form]')).toHaveAttribute('data-list-form', form);
      drop();
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    }
  });
});

// A1 review fix, round 2: a read paused offline, or in a hidden tab between retries, is still
// awaited; the section says it is loading, never "no …" or "failed", and resumes with the data.
describe('ForksPage while its reads are paused', () => {
  it('says both are loading, not "No completed fork days", then draws them once back online', async () => {
    vi.mocked(api.getForks).mockResolvedValue(list());
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    const { resume } = renderPaused(<ForksPage />);
    expect(await screen.findByText('Loading fork history…')).toHaveAttribute('role', 'status');
    expect(screen.getByText('Loading branch switches…')).toHaveAttribute('role', 'status');
    expect(screen.queryByText('No completed fork days are indexed.')).toBeNull();
    resume();
    expect(await screen.findByText('250.00')).toBeInTheDocument();
    expect(
      await screen.findByRole('table', { name: 'Branch switches on this node' })
    ).toBeInTheDocument();
    expect(screen.queryByText(/^Loading/)).toBeNull();
  });
});
