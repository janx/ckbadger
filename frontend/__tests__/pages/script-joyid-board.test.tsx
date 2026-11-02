import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { phoneLines, stubLayoutWidths } from '@/__tests__/utils/phone-row';
import fixture from '@/__tests__/fixtures/scripts/joyid-family-detail.json';
import ScriptDetailPage from '@/app/scripts/[name]/client-page';
import { api, type Cell } from '@/lib/api';
import { DATA_COLOR, HELD_CAPACITY_COLOR } from '@/lib/design-tokens';
import { expectTier1Budget } from '@/__tests__/utils/hierarchy';

vi.mock('@/lib/api', () => ({
  api: {
    getScriptFamilyDetail: vi.fn(),
    getScriptUsage: vi.fn(),
    getScriptCapacityChart: vi.fn(),
    getScriptCapacityChartByCodeHash: vi.fn(),
    getCell: vi.fn(),
    getCellsByScriptRef: vi.fn(),
    getCodeCells: vi.fn(),
    lookupScripts: vi.fn(),
  },
  isNotFoundApiError: () => false,
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div /> }));
vi.mock('@/src/navigation', () => ({
  useParams: () => ({ name: 'JoyID' }),
  usePathname: () => '/scripts/JoyID',
  useSearchParams: () => new URLSearchParams(),
}));

// JoyID as the chain has it (ScriptJoyid board, V80): five binaries under one Type ID, held by
// three multisigs; the API shapes (versions, history markers, references) are /scripts/JoyID's.
const codeCell = (txHash: string, outputIndex: number): Cell => {
  const v = fixture.versions.find((x) => x.tx === txHash && x.idx === outputIndex);
  if (!v) throw new Error(`unexpected code cell ${txHash}:${outputIndex}`);
  return {
    txHash,
    outputIndex,
    capacity: String(v.cap),
    commonKnowledgeSize: String(v.cap),
    lockScriptHash: '0x01',
    dataSize: v.size,
    createdAtBlock: v.block,
    status: v.v === 5 ? 'live' : 'dead',
  };
};

async function renderJoyid() {
  render(<ScriptDetailPage name="JoyID" />);
  await screen.findByRole('heading', { name: 'JoyID', level: 1 });
  await screen.findByRole('table', { name: 'Its cells' });
}

describe('JoyID script page against the ScriptJoyid board', () => {
  beforeEach(() => {
    // Charts at 900 px; the lists at the page's full content width (1248 at 1280 and up).
    stubLayoutWidths({ chart: 900, list: 1248 });
    vi.mocked(api.getScriptFamilyDetail).mockResolvedValue(fixture.family as never);
    vi.mocked(api.getScriptUsage).mockResolvedValue(fixture.usage);
    vi.mocked(api.getScriptCapacityChart).mockResolvedValue(fixture.chart);
    vi.mocked(api.getCell).mockImplementation(async (tx, index) => codeCell(tx, index));
    vi.mocked(api.getCellsByScriptRef).mockResolvedValue(fixture.cells as never);
    vi.mocked(api.lookupScripts).mockResolvedValue({});
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('opens with breadcrumb, role, the figure and one row of figures; the summary sentence in tier 2 (SC-S1, hierarchy v3)', async () => {
    await renderJoyid();
    expect(screen.getByRole('navigation', { name: 'Breadcrumb' })).toHaveTextContent(
      'Scripts / Locks'
    );
    expect(screen.getByText('lock script · upgradable')).toBeInTheDocument();
    const tier2 = document.querySelector('[data-tier="2"]') as HTMLElement;
    const summary = within(tier2).getByTestId('script-summary').textContent!;
    expect(summary).toContain(
      'JoyID decides who can spend 161,049 live cells holding 456,002,179.57'
    );
    expect(summary).toContain('2,707,778 cells have used it since 2023-10-07');
    expect(summary).toContain('it has been replaced 4 times, last on 2025-11-21');
    expect(summary).toContain('now sits under a multisig that has never signed');
    // The former facts rail: its figures are the hero's figure, companion and metric row.
    expect(screen.queryByRole('complementary', { name: 'Script facts' })).toBeNull();
    const hero = screen.getByRole('region', { name: 'JoyID' });
    expect(hero.querySelector('[data-role="figure"]')).toHaveTextContent(
      /in 161,049 live cells · 4\.19% occupied$/
    );
    const metrics = within(hero.parentElement!).getByLabelText('Script figures');
    expect(metrics).toHaveTextContent('2,707,778 cells');
    expect(metrics).toHaveTextContent('Upgradable · 5 versions · last replaced 2025-11-21');
    expect(metrics).toHaveTextContent('multisig 0x6598…d696 · has never signed');
    expect(metrics).toHaveTextContent('In use since2023-10-07');
  });

  it('draws History with occupied in jade, a separate occupied panel and the code track (SC-S2, SC-S3)', async () => {
    const { container } = { container: document.body };
    await renderJoyid();
    const fills = [...container.querySelectorAll('svg path[fill]')].map((p) =>
      p.getAttribute('fill')
    );
    expect(fills).toContain(DATA_COLOR);
    expect(fills).toContain(HELD_CAPACITY_COLOR);
    expect(fills).not.toContain('#58a5b5');
    // Hierarchy v3: each panel's name is written inside its plot (R13).
    expect(
      [...document.querySelectorAll('[data-role="chart-name"]')].map((t) => t.textContent)
    ).toEqual(['CKB in JoyID cells · occupied and free', "occupied by those cells' own bytes"]);
    const reading = screen.getByTestId('script-history-reading');
    expect(reading).toHaveTextContent('2026-10-29');
    expect(reading).toHaveTextContent('CKB held 456,002,179.57');
    expect(reading).toHaveTextContent('code v5 held by multisig 0x6598…d696');
    const track = screen.getByLabelText('Code track');
    expect(within(track).getByText('v1 · v2')).toBeInTheDocument();
    expect(within(track).getByText('v3 · v4')).toBeInTheDocument();
    expect(within(track).getByText('v5')).toBeInTheDocument();
    expect(track).toHaveTextContent('multisig 0xd1db…b95c · 3 of 5 signatures');
    expect(track).toHaveTextContent('multisig 0x6598…d696 · signers not on chain yet');
    expect(document.body.textContent).not.toMatch(/common knowledge/i);
  });

  it('shows who can change the code as three multisigs, key by key (SC-S4)', async () => {
    await renderJoyid();
    const holders = screen.getByRole('list', { name: 'Code cell holders' });
    // one column at 390 px, one per holder from md up (no horizontal overflow on phones)
    expect(holders).toHaveClass('grid-cols-1');
    expect(holders.style.getPropertyValue('--holders')).toBe('3');
    const columns = within(holders)
      .getAllByRole('listitem')
      .filter((li) => li.dataset.holder);
    expect(columns).toHaveLength(3);
    expect(columns[0]).toHaveTextContent('2023-10-07 → 2025-08-28');
    expect(columns[0]).toHaveTextContent('3 of 5 signatures');
    expect(columns[0]).toHaveTextContent('signed v2 and v3');
    expect(columns[1]).toHaveTextContent('signed v4 and v5');
    expect(columns[2]).toHaveTextContent('has never signed');
    expect(columns[2]).toHaveTextContent(
      'Its keys and threshold appear on chain the first time it signs.'
    );
    expect(columns[0].querySelectorAll('[data-kept="true"]')).toHaveLength(2);
    expect(columns[1].querySelectorAll('[data-kept="true"]')).toHaveLength(2);
    expect(
      screen.getByText(/kept across the handover on 2025-08-28: 2 of 5 keys/)
    ).toBeInTheDocument();
    expect(screen.getByText(/Three multisigs have held it\./)).toBeInTheDocument();
  });

  it('merges the versions into one full-width table (SC-S5)', async () => {
    await renderJoyid();
    expect(screen.queryByRole('table', { name: 'Code cell deployments' })).toBeNull();
    const table = screen.getByRole('table', { name: 'Code versions' });
    const rows = within(table).getAllByRole('row').slice(1);
    expect(rows.map((row) => row.textContent!.match(/^v\d/)![0])).toEqual([
      'v1',
      'v2',
      'v3',
      'v4',
      'v5',
    ]);
    await waitFor(() => expect(rows[0]).toHaveTextContent('96,760 B'));
    expect(rows[0]).toHaveTextContent('2023-10-07 15:39');
    expect(rows[0]).toHaveTextContent('#11,132,140');
    expect(rows[0]).toHaveTextContent('consumed by v2');
    expect(rows[0]).toHaveTextContent('first deployed');
    expect(rows[0]).toHaveTextContent('into multisig 0xd1db…b95c');
    expect(rows[2]).toHaveTextContent('hands it to 0x07a2…68ca');
    expect(rows[4]).toHaveTextContent('current');
    expect(rows[4]).toHaveTextContent('100,438.00000000');
    expect(rows[4]).toHaveTextContent('live');
    expect(screen.getByText(/one type id, 0xd00c84f0…318323, 5 binaries/)).toBeInTheDocument();
  });

  it('says how to use it in a transaction (SC-S6)', async () => {
    await renderJoyid();
    const section = screen
      .getByRole('heading', { name: 'Use it in a transaction' })
      .closest('section')!;
    expect(section).toHaveTextContent(
      '0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323'
    );
    expect(section).toHaveTextContent('type · follows every upgrade');
    expect(section).toHaveTextContent(
      '0x673e1843035ce9c249efc36b62865e7a7c9f6e2ca6fc9c66fd108307e81355f9:0'
    );
    expect(section).toHaveTextContent('Pin this exact binary instead');
    expect(section).toHaveTextContent(
      '0x4af7d832e7c328aa4ad6f2deb669743446aea7b9a2f0c25646897501ac4f1ebe'
    );
  });

  it('lists all its cells with type names, free in the anatomy and occupied bytes (SC-S7, SC-S10)', async () => {
    await renderJoyid();
    expect(screen.getByTestId('script-cells-summary')).toHaveTextContent(
      '161,049 live cells use JoyID as their lock'
    );
    const table = screen.getByRole('table', { name: 'Its cells' });
    const rows = within(table).getAllByRole('row').slice(1);
    expect(rows[0]).toHaveTextContent('0x44e5e9…517f:1');
    expect(rows[0]).toHaveTextContent('63 B');
    expect(rows[0]).toHaveTextContent('—');
    expect(rows[1]).toHaveTextContent('COTA');
    expect(rows[1]).not.toHaveTextContent('0x1122a4fb');
    expect(rows[0].querySelector('[data-part="free"]')).not.toBeNull();
    expect(rows[0].innerHTML).not.toMatch(/#f2c55c|text-gold|text-dao/);
    expect(rows[0].textContent).not.toContain(
      'ckb1qrgqep8saj8agswr30pls73hra28ry8jlnlc3ejzh3dl2ju7xxpjxqgqq03dwzcucjtl5qcg72kxx9euah4avhq3dv77xsf8'
    );
    // Every registered reference is listed, not only a selected version's.
    expect(api.getCellsByScriptRef).toHaveBeenCalledWith(
      expect.objectContaining({
        codeHash: '0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323',
        hashType: 'type',
        scriptKind: 'lock',
      })
    );
  });

  it('keeps registered references in the developer tier (SC-S9)', async () => {
    await renderJoyid();
    const tier1 = document.querySelector('[data-tier="1"]') as HTMLElement;
    const tier2 = document.querySelector('[data-tier="2"]') as HTMLElement;
    expect(within(tier1).queryByRole('heading', { name: 'Registered references' })).toBeNull();
    expect(
      within(tier2).getByRole('heading', { name: 'Registered references' })
    ).toBeInTheDocument();
  });
});

describe('JoyID script page · visual pass', () => {
  beforeEach(() => {
    // Charts at 900 px; the lists at the page's full content width (1248 at 1280 and up).
    stubLayoutWidths({ chart: 900, list: 1248 });
    vi.mocked(api.getScriptFamilyDetail).mockResolvedValue(fixture.family as never);
    vi.mocked(api.getScriptUsage).mockResolvedValue(fixture.usage);
    vi.mocked(api.getScriptCapacityChart).mockResolvedValue(fixture.chart);
    vi.mocked(api.getCell).mockImplementation(async (tx, index) => codeCell(tx, index));
    vi.mocked(api.getCellsByScriptRef).mockResolvedValue(fixture.cells as never);
    vi.mocked(api.lookupScripts).mockResolvedValue({});
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('draws the upgrade days as dashed lines through every panel, not as a link list', async () => {
    await renderJoyid();
    const history = screen.getByTestId('script-history');
    expect(history.querySelectorAll('[data-upgrade-line]')).toHaveLength(3);
    expect(within(history).queryByRole('link', { name: /v1 · v2 · 2023-10-07/ })).toBeNull();
  });

  it('staggers code-track labels so neighbours never overlap (390 px)', async () => {
    await renderJoyid();
    const track = screen.getByLabelText('Code track');
    expect(
      [...track.querySelectorAll<HTMLElement>('[data-upgrade-day]')].map((n) => n.dataset.row)
    ).toEqual(['0', '1', '0']);
    const custodians = [...track.querySelectorAll<HTMLElement>('[data-custodian-label]')];
    // one row per holder: at 390 px the first and last labels would otherwise collide
    expect(custodians.map((n) => n.dataset.row)).toEqual(['0', '1', '2']);
    // a label that starts past the middle hangs from the right edge instead of running off it
    expect(custodians[2].style.right).toBe('0%');
    expect(custodians[0].style.left).toBe('0%');
  });

  it('states the peak of CKB held from the API (SC-S8, R2b-b totalPeak)', async () => {
    await renderJoyid();
    expect(screen.getByTestId('script-history-peak')).toHaveTextContent(
      /CKB held peaked at 456,002,179\.570* CKB on 2026-10-29/
    );
  });

  // Hierarchy plan rule (topic-page lists 5 rows; was the board's 12): the pagination under the
  // table stays the way to every cell, as no page lists a script's cells on its own.
  it('pages its cells 5 at a time, oldest first', async () => {
    await renderJoyid();
    expect(api.getCellsByScriptRef).toHaveBeenCalledWith(expect.objectContaining({ limit: 5 }));
    expect(screen.getByText(/Oldest first, as the cell index keeps them/)).toBeInTheDocument();
  });
});

/** Hue text classes (R11): DAO gold, jade / rouge, the link blues; greys are ink, not colour. */
const COLOURED_TEXT =
  /^text-(jade|gold|positive|negative|rouge|warning|emphasis|aqua|info)(-|$)|^text-\[#(2edba3|6aa9c4|68ccf0|f2c55c)\]$/;

// Hierarchy v3 (plan §1b R11–R16; lane C tasks C0′, C2, C3): the page opens like the DAO page —
// the title row with the range, the description as the lede, CKB in its cells as the one figure
// and its history right under it; the reading and one row of figures follow; prose in tier 2.
describe('JoyID script page · hierarchy v3 (C0′, C2, C3)', () => {
  beforeEach(() => {
    stubLayoutWidths({ chart: 900, list: 1248 });
    vi.mocked(api.getScriptFamilyDetail).mockResolvedValue(fixture.family as never);
    vi.mocked(api.getScriptUsage).mockResolvedValue(fixture.usage);
    vi.mocked(api.getScriptCapacityChart).mockResolvedValue(fixture.chart);
    vi.mocked(api.getCell).mockImplementation(async (tx, index) => codeCell(tx, index));
    vi.mocked(api.getCellsByScriptRef).mockResolvedValue(fixture.cells as never);
    vi.mocked(api.lookupScripts).mockResolvedValue({});
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const hero = () => screen.getByRole('region', { name: 'JoyID' });
  const tier1 = () => document.querySelector('[data-tier="1"]') as HTMLElement;
  const tier2 = () => document.querySelector('[data-tier="2"]') as HTMLElement;
  const follows = (a: Element, b: Element) =>
    Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);

  it('opens with the title row — h1, its role and the range buttons — and the description as the lede (R13)', async () => {
    await renderJoyid();
    const nav = screen.getByRole('navigation', { name: 'Breadcrumb' });
    expect(tier1()).toContainElement(nav);
    expect(follows(nav, hero())).toBe(true);
    const h1 = within(hero()).getByRole('heading', { level: 1, name: 'JoyID' });
    expect(h1).toHaveClass('text-entity', 'text-text-bright');
    const row = h1.parentElement!.parentElement!;
    expect(row).toHaveClass('flex', 'items-baseline', 'justify-between');
    expect(within(row).getByText('lock script · upgradable')).toBeInTheDocument();
    const range = within(row).getByRole('group', { name: 'Range' });
    const all = within(range).getByRole('button', { name: 'ALL' });
    expect(all).toHaveAttribute('aria-pressed', 'true');
    // A pressed range stays neutral (R11; D3 call 8): no jade text in tier 1.
    expect(all).toHaveClass('text-text-bright');
    expect(all.className).not.toMatch(/jade/);
    const lede = hero().querySelector('[data-role="lede"]') as HTMLElement;
    expect(lede).toHaveTextContent(
      /^An universal Account Protocol for Web3 Mass-adoption · joy\.id ↗$/
    );
    expect(lede).toHaveClass('text-body', 'text-text-dim', 'max-w-[560px]');
    expect(within(lede).getByRole('link', { name: 'joy.id ↗' })).toHaveAttribute(
      'href',
      'https://joy.id'
    );
  });

  it('leads with CKB in its cells as the one figure, bright, its live cells on the baseline row (R4, R11, R13)', async () => {
    await renderJoyid();
    const figure = hero().querySelector('[data-role="figure"]') as HTMLElement;
    expect(figure).toHaveTextContent(/^CKB in its cells/);
    const amount = figure.querySelector('[data-role="figure-amount"]') as HTMLElement;
    expect(amount).toHaveTextContent(/^456,002,179\.570* CKB$/);
    expect(amount).toHaveClass('text-index', 'sm:text-display', 'text-text-bright');
    const row = amount.parentElement!;
    expect(row).toHaveClass('flex', 'items-baseline', 'justify-between');
    expect(row.lastElementChild).toHaveTextContent(/^in 161,049 live cells · 4\.19% occupied$/);
    expect(within(row.lastElementChild as HTMLElement).getByRole('link')).toHaveAttribute(
      'href',
      '#its-cells'
    );
  });

  it('draws its capacity history right under the figure: ≥ 280 px, named inside, no History heading (R13, tier 1 always)', async () => {
    await renderJoyid();
    expect(within(tier1()).queryByRole('heading', { name: 'History' })).toBeNull();
    expect(tier1().querySelector('figcaption')).toBeNull();
    const history = within(hero()).getByTestId('script-history');
    expect(follows(hero().querySelector('[data-role="figure"]')!, history)).toBe(true);
    const [main, occupied] = [...history.querySelectorAll('svg[role="img"]')];
    expect(Number(main.getAttribute('height'))).toBeGreaterThanOrEqual(280);
    expect(main.querySelector('[data-role="chart-name"]')).toHaveTextContent(
      'CKB in JoyID cells · occupied and free'
    );
    expect(Number(occupied.getAttribute('height'))).toBeLessThan(
      Number(main.getAttribute('height'))
    );
    // Values inside the plot from its left edge, no axis column (R13).
    const ticks = [...main.querySelectorAll('[data-role="y-tick"]')];
    expect(ticks.length).toBeGreaterThan(0);
    for (const t of ticks) expect(t.getAttribute('text-anchor')).not.toBe('end');
  });

  it('reads the last full day in one row under the code track, then the remaining figures in one quiet row (R13)', async () => {
    await renderJoyid();
    const track = screen.getByLabelText('Code track');
    const reading = screen.getByTestId('script-history-reading');
    const metrics = screen.getByLabelText('Script figures');
    expect(follows(hero(), track)).toBe(true);
    expect(follows(track, reading)).toBe(true);
    expect(follows(reading, metrics)).toBe(true);
    expect(reading).toHaveClass('text-small', 'border-t');
    expect(reading).toHaveTextContent('2026-10-29');
    expect(reading).toHaveTextContent('code v5 held by multisig 0x6598…d696');
    // The swatches keep the series colours; the values are ink (R11).
    for (const el of reading.querySelectorAll<HTMLElement>('[style]'))
      if (el.textContent!.trim()) expect(el.style.color).toBe('');
    const terms = within(metrics).getAllByRole('term');
    expect(terms.map((t) => t.textContent)).toEqual([
      'Code',
      'Code cell held by',
      'In use since',
      'Ever used',
    ]);
    for (const t of terms) {
      expect(t).toHaveClass('text-meta', 'text-text-dim');
      expect(t.nextElementSibling).toHaveClass('text-section', 'text-text-bright');
      expect(t.parentElement!.children).toHaveLength(2);
    }
    expect(metrics).toHaveClass('grid-cols-2', 'lg:grid-cols-4');
  });

  it('reads the hovered day across the picture, and lets it go when the range changes', async () => {
    await renderJoyid();
    const reading = screen.getByTestId('script-history-reading');
    expect(reading).toHaveTextContent('2026-10-29 · last full day');
    const [main] = [
      ...within(hero()).getByTestId('script-history').querySelectorAll('svg[role="img"]'),
    ];
    fireEvent.keyDown(main, { key: 'ArrowLeft' });
    expect(reading).not.toHaveTextContent('last full day');
    expect(reading).not.toHaveTextContent('2026-10-29');
    fireEvent.click(within(hero()).getByRole('button', { name: '90D' }));
    // The picture and its reading are drawn again for the new range.
    await waitFor(() =>
      expect(screen.getByTestId('script-history-reading')).toHaveTextContent(
        '2026-10-29 · last full day'
      )
    );
  });

  it('moves the summary sentence, the chart notes and the peak to tier 2 (C2)', async () => {
    await renderJoyid();
    for (const id of ['script-summary', 'script-history-peak']) {
      expect(within(tier1()).queryByTestId(id)).toBeNull();
      expect(within(tier2()).getByTestId(id)).toBeInTheDocument();
    }
    expect(tier1()).not.toHaveTextContent('A cell must hold enough CKB');
    expect(tier2()).toHaveTextContent(
      'A cell must hold enough CKB to pay for its own bytes. Occupied is that cost for every cell JoyID guards; free is CKB its owners keep there. Dashed lines mark the days its code was replaced.'
    );
  });

  it('says who can change its code in two sentences under the holders; the mechanism in tier 2 (C2)', async () => {
    await renderJoyid();
    const section = screen.getByRole('region', { name: 'Who can change its code' });
    expect(within(section).getByRole('heading', { level: 2 })).toHaveClass('text-section');
    const holders = within(section).getByRole('list', { name: 'Code cell holders' });
    const caption = section.querySelector('[data-role="caption"]') as HTMLElement;
    expect(caption).toHaveTextContent(
      /^Whoever can spend its code cell can put new code in it, and every JoyID cell runs it from the next block\. Three multisigs have held it\.$/
    );
    expect(caption.textContent!.length).toBeLessThanOrEqual(160);
    expect(caption).toHaveClass('text-body', 'max-w-[640px]');
    expect(follows(holders, caption)).toBe(true);
    expect(section.lastElementChild).toBe(caption);
    expect(tier1()).not.toHaveTextContent('point at their code through one code cell');
    expect(tier2()).toHaveTextContent(
      "JoyID cells point at their code through one code cell's type id, not at a fixed binary. Whoever can spend that cell can put new code in it, and every JoyID cell runs the new code from the next block."
    );
  });

  it('keeps the code versions table in tier 1 and its heading note and the transaction recipe in tier 2 (C3, R14)', async () => {
    await renderJoyid();
    const versions = screen.getByRole('table', { name: 'Code versions' });
    expect(tier1()).toContainElement(versions);
    const h2 = within(tier1()).getByRole('heading', { name: 'Code versions', level: 2 });
    expect(h2).toHaveClass('text-section');
    expect(h2.parentElement!.children).toHaveLength(1);
    expect(
      within(tier2()).getByText(/one type id, 0xd00c84f0…318323, 5 binaries/)
    ).toBeInTheDocument();
    expect(
      within(tier2()).getByRole('heading', { name: 'Use it in a transaction' })
    ).toBeInTheDocument();
    expect(within(tier1()).queryByRole('heading', { name: 'Use it in a transaction' })).toBeNull();
    // The cells' summary line, with its exact totals, is tier 2's too (R14).
    expect(within(tier2()).getByTestId('script-cells-summary')).toBeInTheDocument();
  });

  it('spaces tier 1 like the DAO page: 64 px under the hero, then 72 px, h2 18 px above its evidence (R6)', async () => {
    await renderJoyid();
    const custody = screen.getByRole('region', { name: 'Who can change its code' });
    expect(custody).toHaveClass('mt-16');
    for (const name of ['Code versions', 'Its cells']) {
      const h2 = within(tier1()).getByRole('heading', { name, level: 2 });
      expect(h2.closest('section')).toHaveClass('mt-[72px]');
      // The heading row is a flex row, as the DAO page's: the h2 is as wide as its words.
      expect(h2.parentElement).toHaveClass('mb-[18px]', 'flex', 'items-baseline');
    }
    expect(within(custody).getByRole('heading', { level: 2 }).parentElement).toHaveClass(
      'mb-[18px]',
      'flex'
    );
  });

  it('writes no coloured text in tier 1: the figure is bright, the graphics keep their colours (R11)', async () => {
    await renderJoyid();
    await waitFor(() =>
      expect(screen.getByRole('table', { name: 'Code versions' })).toHaveTextContent('96,760 B')
    );
    const coloured = [...tier1().querySelectorAll('[class]')].filter((el) =>
      el
        .getAttribute('class')!
        .split(/\s+/)
        .some((c) => COLOURED_TEXT.test(c))
    );
    expect(coloured.map((el) => el.outerHTML.slice(0, 90))).toEqual([]);
    const inked = [...tier1().querySelectorAll<HTMLElement>('[style]')].filter(
      (el) => el.style.color && el.textContent!.trim()
    );
    expect(inked.map((el) => el.outerHTML.slice(0, 90))).toEqual([]);
  });

  it('keeps tier 1 within the prose budget (§0, §4)', async () => {
    await renderJoyid();
    expectTier1Budget(document.body, { maxProseBlocks: 3, maxChars: 160 });
  });
});

// Board L7 (responsive plan, L3): the code versions on a phone and in the 640 band.
describe('JoyID code versions at phone and 640 widths (board L7)', () => {
  const mount = (list: number) => {
    stubLayoutWidths({ chart: 900, list });
    vi.mocked(api.getScriptFamilyDetail).mockResolvedValue(fixture.family as never);
    vi.mocked(api.getScriptUsage).mockResolvedValue(fixture.usage);
    vi.mocked(api.getScriptCapacityChart).mockResolvedValue(fixture.chart);
    vi.mocked(api.getCell).mockImplementation(async (tx, index) => codeCell(tx, index));
    vi.mocked(api.getCellsByScriptRef).mockResolvedValue(fixture.cells as never);
    vi.mocked(api.lookupScripts).mockResolvedValue({});
    render(<ScriptDetailPage name="JoyID" />);
  };
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('writes a version in lines at 358 px: version, date, code cell, status · the hash · its figures', async () => {
    mount(358);
    const rows = await screen.findAllByTestId('code-version-row-mobile');
    expect(screen.queryByRole('table', { name: 'Code versions' })).toBeNull();
    expect(rows).toHaveLength(5);
    await waitFor(() => expect(rows[0]).toHaveTextContent('96,760 B'));
    const first = phoneLines(rows[0]);
    expect(first.map((l) => [l.key, l.tone])).toEqual([
      ['version', 'body'],
      ['hash', 'quiet'],
      ['detail', 'quiet'],
    ]);
    expect(first[0].left).toMatch(/^v1 2023-10-07 · 0x[0-9a-f]{8}…[0-9a-f]{6}:\d$/);
    expect(first[0].right).toBe('consumed by v2');
    // The version hash keeps a line of its own, whole: it is what a developer copies.
    expect(first[1].left).toMatch(/^0x[0-9a-f]{64}$/);
    expect(first[2].left).toMatch(
      /^#11,132,140 · 96,760 B · .+ CKB · first deployed into multisig 0xd1db…b95c$/
    );
    expect(first[2].right).toMatch(/^live cells using it: /);
    const last = phoneLines(rows[4]);
    expect(last[0].left).toMatch(/^v5 current /);
    expect(last[0].right).toBe('live');
  });

  it('draws the five-column table of board L7 in the 608 px band, the hash under the code cell', async () => {
    mount(608);
    const table = await screen.findByRole('table', { name: 'Code versions' });
    expect(
      within(table)
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual(['v', 'Deployed', 'Code cell · version hash', 'Status', 'Live cells']);
    expect(Number.parseFloat(table.style.minWidth)).toBeLessThanOrEqual(608);
    const [first] = within(table).getAllByRole('row').slice(1);
    const cell = first.querySelectorAll('td')[2];
    await waitFor(() => expect(cell).toHaveTextContent('96,760 B · first deployed into multisig'));
    expect(cell.lastElementChild!.textContent).toMatch(/^0x[0-9a-f]{64}$/);
    expect(first.querySelectorAll('td')[3]).toHaveTextContent('consumed by v2');
  });
});
