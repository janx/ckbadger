import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { expectTier1Budget } from '@/__tests__/utils/hierarchy';
import { screen, waitFor, within, fireEvent } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ScriptsPage from '@/app/scripts/page';
import { api, type KnownScript } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: { getScripts: vi.fn() },
  isWarmupPendingError: () => false,
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div /> }));

// Rows shaped like /scripts family list items after scriptFamilyListItemToKnownScript.
function family(overrides: Partial<KnownScript>): KnownScript {
  return {
    codeHash: 'family:x',
    name: 'X',
    description: null,
    scriptKind: 'lock',
    rfc: null,
    website: null,
    sourceUrl: null,
    decoderType: null,
    network: '',
    hashType: null,
    dataHash: null,
    typeHash: null,
    tag: null,
    deprecated: false,
    isSystem: false,
    codeCellTxHash: null,
    codeCellOutputIndex: null,
    deployedAt: null,
    ownedCapacitySum: '100000000',
    ownedKnowledgeSum: '50000000',
    liveCellsCount: 1,
    cellsCount: 2,
    versionsCount: 1,
    codeClasses: [],
    firstDeployedAt: null,
    ...overrides,
  };
}

const joyid = family({
  codeHash: 'family:joyid',
  name: 'JoyID',
  description: 'An universal Account Protocol for Web3 Mass-adoption',
  ownedCapacitySum: '45600217957000000',
  ownedKnowledgeSum: '1909628300000000',
  liveCellsCount: 161049,
  cellsCount: 2707778,
  versionsCount: 5,
  firstDeployedAt: {
    blockNumber: 11132140,
    timestampMs: 1696664371829,
    versionHash: '0x0b6a8ef9dee12629aaa738266a56e6e0ec3330782af9ac29f50e04b0daa1845c',
  },
  codeClasses: [
    {
      class: 'multisig',
      lockCellsCount: 161049,
      lockOwnedCapacitySum: '45600217957000000',
      referenceCount: 1,
    },
  ],
});
const defaultLock = family({
  codeHash: 'family:default-lock',
  name: 'Default Lock',
  ownedCapacitySum: '4100000000000000000',
  ownedKnowledgeSum: '50000000000000000',
  liveCellsCount: 1717237,
  cellsCount: 78239443,
  versionsCount: 2,
  firstDeployedAt: {
    blockNumber: 0,
    timestampMs: 1573852190812,
    versionHash: '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649',
  },
  codeClasses: [
    { class: 'fixed', lockCellsCount: 1, lockOwnedCapacitySum: '1', referenceCount: 1 },
    { class: 'unspendable', lockCellsCount: 2, lockOwnedCapacitySum: '2', referenceCount: 1 },
  ],
});
const godwoken = family({
  codeHash: 'family:godwoken',
  name: 'Godwoken Custodian Lock',
  deprecated: true,
  ownedCapacitySum: '10000000000000',
  ownedKnowledgeSum: '10000000',
  codeClasses: [],
});

const response = {
  data: [joyid, defaultLock, godwoken],
  total: 3,
  limit: 50,
  hasMore: false,
  nextCursor: null,
  roleCounts: { all: 3, locks: 3, types: 0 },
};

describe('Scripts list against the Scripts board', () => {
  beforeEach(() => {
    vi.mocked(api.getScripts).mockResolvedValue(response);
  });

  // Responsive fix plan 2026-11-01 (A12d, report C#12): one clamped line cut the description
  // mid-sentence on narrow widths; below md it may take two lines, the desktop table keeps one.
  it('lets a description take two lines below md', async () => {
    render(<ScriptsPage />);
    const descriptions = await screen.findAllByText(
      'An universal Account Protocol for Web3 Mass-adoption'
    );
    for (const d of descriptions) {
      expect(d).toHaveClass('line-clamp-2', 'md:line-clamp-1');
      expect(d).not.toHaveClass('truncate');
    }
  });

  it('keeps the board lead sentence (tier 2 since v3) and a name search (SC-L5)', async () => {
    render(<ScriptsPage />);
    expect(
      screen.getByText(
        'Every cell runs one lock script, which decides who can spend it, and may run one type script, which decides what the cell is and how it may change.'
      )
    ).toBeInTheDocument();
    expect(screen.getByPlaceholderText('Search by name or code hash')).toBeInTheDocument();
  });

  it('writes the Code column as Immutable or Upgradable · N versions · by whom (SC-L2, SC-L4)', async () => {
    render(<ScriptsPage />);
    const table = await screen.findByRole('table', { name: 'Scripts' });
    const row = (name: string) =>
      within(table).getByRole('link', { name }).closest('tr') as HTMLElement;
    const joyCode = within(row('JoyID')).getByTestId('script-code-class');
    expect(joyCode.textContent).toContain('Upgradable · 5 versions');
    expect(joyCode.textContent).toContain('by a multisig');
    expect(within(row('Default Lock')).getByTestId('script-code-class').textContent).toContain(
      'Immutable'
    );
    expect(table.textContent).not.toMatch(/Fixed bytecode|code classes/);
    // No class evidence from the API stays visible as such, never invented.
    expect(
      within(row('Godwoken Custodian Lock')).getByTestId('script-code-class').textContent
    ).toBe('Code evidence unavailable');
  });

  it('shows "of N ever" and exact CKB, and a rounded occupied share (SC-L6, SC-L7)', async () => {
    render(<ScriptsPage />);
    const table = await screen.findByRole('table', { name: 'Scripts' });
    const joyRow = within(table).getByRole('link', { name: 'JoyID' }).closest('tr') as HTMLElement;
    expect(joyRow.textContent).toContain('161,049');
    expect(joyRow.textContent).toContain('of 2,707,778 ever');
    expect(joyRow.textContent).toContain('456,002,179.57');
    expect(joyRow.textContent).not.toContain('456.00M');
    expect(joyRow.textContent).toContain('4.19%');
    const godRow = within(table)
      .getByRole('link', { name: 'Godwoken Custodian Lock' })
      .closest('tr') as HTMLElement;
    expect(godRow.textContent).toContain('0.0001%');
    expect(godRow.textContent).not.toContain('0.00%');
  });

  it('marks deprecated in grey, not red (SC-L8)', async () => {
    render(<ScriptsPage />);
    const table = await screen.findByRole('table', { name: 'Scripts' });
    const mark = within(table).getByText('deprecated');
    expect(mark.className).not.toMatch(/negative|rouge/);
    expect(mark.className).toContain('text-text-dim');
  });

  it('uses underline role tabs and one "sorted by" control in board words (SC-L8)', async () => {
    render(<ScriptsPage />);
    const tabs = await screen.findByRole('group', { name: 'Role' });
    const all = within(tabs).getByRole('button', { name: /All/ });
    expect(all).toHaveAttribute('aria-pressed', 'true');
    expect(all.className).toContain('border-b-2');
    expect(screen.queryByText('Used (CKB)')).toBeNull();
    expect(screen.queryByText('Capacity (CKB)')).toBeNull();
    const sort = screen.getByRole('combobox', { name: 'Sort scripts' });
    expect(sort).toHaveValue('capacity');
    expect(within(sort).getByRole('option', { name: 'CKB in their cells' })).toBeInTheDocument();
    fireEvent.change(sort, { target: { value: 'liveCells' } });
    await waitFor(() =>
      expect(api.getScripts).toHaveBeenLastCalledWith(
        expect.objectContaining({ sortKey: 'liveCells', sortDirection: 'desc' })
      )
    );
  });

  it('never says "Common Knowledge" in the narrow row (SC-L8)', async () => {
    render(<ScriptsPage />);
    await screen.findByRole('table', { name: 'Scripts' });
    expect(document.body.textContent).not.toMatch(/Common Knowledge/i);
  });
});

describe('Scripts list · visual pass (R2b-b)', () => {
  beforeEach(() => {
    vi.mocked(api.getScripts).mockResolvedValue(response);
  });

  it('dates the code: "since" for upgradable code, "genesis" for genesis code (SC-L4)', async () => {
    render(<ScriptsPage />);
    const table = await screen.findByRole('table', { name: 'Scripts' });
    const code = (name: string) =>
      within(within(table).getByRole('link', { name }).closest('tr') as HTMLElement).getByTestId(
        'script-code-class'
      );
    expect(code('JoyID')).toHaveTextContent('by a multisig · since 2023-10-07');
    expect(code('Default Lock')).toHaveTextContent('genesis · code cell unspendable');
    expect(code('Godwoken Custodian Lock')).toHaveTextContent('Code evidence unavailable');
  });

  it('searches by code hash through the API (SC-L5)', async () => {
    render(<ScriptsPage />);
    fireEvent.change(screen.getByRole('textbox', { name: 'Search scripts' }), {
      target: { value: ' 0xd00c84f0 ' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await waitFor(() =>
      expect(api.getScripts).toHaveBeenLastCalledWith(
        expect.objectContaining({ search: '0xd00c84f0' })
      )
    );
  });

  it('has no second "Scripts" heading over the table (board: one sort line)', async () => {
    render(<ScriptsPage />);
    await screen.findByRole('table', { name: 'Scripts' });
    expect(screen.queryByRole('heading', { name: 'Scripts', level: 2 })).toBeNull();
    expect(screen.getByText('3 scripts')).toBeInTheDocument();
  });
});

// Hierarchy v3 (plan §1b R11–R16, lane C tasks C0, C1, C3): the index opens like the DAO page —
// the title row with the search, one short lede, the CKB under these scripts as the one figure and
// "who can change the code" as its picture; the role tabs head the table; prose in tier 2.
/** Hue text classes (R11): DAO gold, jade / rouge, the link blues; greys are ink, not colour. */
const COLOURED_TEXT =
  /^text-(jade|gold|positive|negative|rouge|warning|emphasis|aqua|info)(-|$)|^text-\[#(2edba3|6aa9c4|68ccf0|f2c55c)\]$/;

describe('Scripts index hero (hierarchy v3, C0 / C1 / C3)', () => {
  const classSummary = [
    {
      class: 'multisig' as const,
      lockCellsCount: 161049,
      lockOwnedCapacitySum: '172700000000000000',
      referenceCount: 18,
      familyCount: 18,
    },
    {
      class: 'fixed' as const,
      lockCellsCount: 9000000,
      lockOwnedCapacitySum: '4803000000000000000',
      referenceCount: 40,
      familyCount: 36,
    },
    {
      class: 'unspendable' as const,
      lockCellsCount: 3,
      lockOwnedCapacitySum: '0',
      referenceCount: 2,
      familyCount: 4,
    },
    {
      class: 'one_key' as const,
      lockCellsCount: 92,
      lockOwnedCapacitySum: '3730000000000000',
      referenceCount: 7,
      familyCount: 7,
    },
  ];
  const withSummary = {
    ...response,
    classSummary,
    roleCounts: { all: 70, locks: 41, types: 34 },
  };
  const LEAD =
    'Every cell runs one lock script, which decides who can spend it, and may run one type script, which decides what the cell is and how it may change.';

  beforeEach(() => {
    stubChartWidth(1376);
    vi.mocked(api.getScripts).mockResolvedValue(withSummary);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  const hero = () => screen.getByRole('region', { name: 'Scripts' });
  const tier1 = () => document.querySelector('[data-tier="1"]') as HTMLElement;
  const tier2 = () => document.querySelector('[data-tier="2"]') as HTMLElement;
  const figure = () => hero().querySelector('[data-role="figure"]') as HTMLElement;

  it('opens with the title row — h1 and the search — and a lede within 90 characters (R13)', async () => {
    render(<ScriptsPage />);
    await within(hero()).findByText((_, el) => el?.getAttribute('data-role') === 'figure-amount');
    const h1 = within(hero()).getByRole('heading', { level: 1, name: 'Scripts' });
    expect(h1).toHaveClass('text-index', 'text-text-bright');
    // The search sits on the h1's row.
    const row = h1.parentElement!;
    expect(row).toHaveClass('flex', 'items-baseline', 'justify-between');
    const input = within(row).getByRole('textbox', { name: 'Search scripts' });
    // One inline group of controls, as the DAO page's range buttons: one line on the title row.
    const form = input.closest('form')!;
    expect(form).toHaveClass('whitespace-nowrap');
    expect(form).not.toHaveClass('flex');
    expect(within(form).getByRole('button', { name: 'Search' })).toHaveClass('inline-flex', 'h-7');
    expect(input).toHaveClass('h-7');
    const lede = hero().querySelector('[data-role="lede"]') as HTMLElement;
    // ≤ 90 (R13), and one line: 71 characters of 13 px mono (7.8 px each) fill the 560 px measure.
    expect(lede.textContent!.length).toBeLessThanOrEqual(71);
    expect(lede).toHaveClass('text-body', 'text-text-dim', 'max-w-[560px]');
    expect(within(lede).getByRole('link', { name: 'How it works ↓' })).toHaveAttribute(
      'href',
      '#evidence'
    );
    // The board's lead sentence, whole, is tier 2's first paragraph (nothing deleted).
    expect(tier1()).not.toHaveTextContent(LEAD);
    expect(within(tier2()).getByText(LEAD)).toBeInTheDocument();
    expect(document.getElementById('evidence')).toContainElement(tier2());
  });

  it('leads with the CKB under these scripts as the one figure, bright, its counts on the same baseline row (R4, R11, R13)', async () => {
    render(<ScriptsPage />);
    const amount = await within(hero()).findByText(
      (_, el) => el?.getAttribute('data-role') === 'figure-amount'
    );
    expect(figure()).toHaveTextContent(/^CKB under these scripts/);
    // 1,727,000,000 + 48,030,000,000 + 0 + 37,300,000 CKB: every class's lock-owned CKB.
    expect(amount).toHaveTextContent(/^49,794,300,000\.0+ CKB$/);
    expect(amount).toHaveClass('text-index', 'sm:text-display', 'text-text-bright');
    const row = amount.parentElement!;
    expect(row).toHaveClass('flex', 'items-baseline', 'justify-between');
    expect(row.lastElementChild).toHaveTextContent(
      /^locked by 41 lock scripts · 70 scripts in all · 34 types$/
    );
    expect(row.lastElementChild).toHaveClass('text-body', 'text-text-dim');
  });

  it('draws who can change the code as a 28 px bar under the figure, names and shares inside the segments that fit (R12, R15, C1)', async () => {
    render(<ScriptsPage />);
    const bar = await within(hero()).findByRole('img', {
      name: 'CKB under lock scripts by who can change their code',
    });
    expect(bar).toHaveClass('h-[28px]');
    // The picture's name above it, quiet; no heading of its own (R13).
    expect(bar.previousElementSibling).toHaveTextContent(/^who can change the code$/);
    expect(bar.previousElementSibling).toHaveClass('text-meta', 'text-text-dim');
    expect(within(hero()).queryByRole('heading', { level: 2 })).toBeNull();
    // Figure before the picture.
    expect(figure().compareDocumentPosition(bar) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const segments = [...bar.querySelectorAll<HTMLElement>('[data-code-class-segment]')];
    // A class without lock-owned CKB draws no segment; the order is immutable → multisig → one key.
    expect(segments.map((s) => s.dataset.codeClassSegment)).toEqual([
      'fixed',
      'multisig',
      'one_key',
    ]);
    // 96.5 % of 1,376 px holds its words; 3.47 % and 0.075 % do not, and stay empty (R12: no
    // exact amounts inside the bar).
    expect(segments.map((s) => s.textContent)).toEqual([
      'Immutable · pinned by data hash 96.5%',
      '',
      '',
    ]);
    expect(bar.textContent).not.toMatch(/\d\.\d{8}/);
  });

  it('keys the bar in one quiet row and explains it in one caption under it (R12, R14, C1)', async () => {
    render(<ScriptsPage />);
    const legend = await screen.findByRole('list', { name: 'Who can change the code, in shares' });
    expect(legend).toHaveClass('text-small', 'text-text-dim');
    expect(
      within(legend)
        .getAllByRole('listitem')
        .map((li) => li.textContent)
    ).toEqual([
      'Immutable · pinned by data hash 96.5%',
      'A multisig can replace 3.47%',
      'One key can replace 0.075%',
    ]);
    for (const share of within(legend).getAllByText(/%$/)) expect(share).toHaveClass('text-text');
    expect(legend.textContent).not.toMatch(/\d\.\d{8}/);
    const caption = tier1().querySelector('[data-role="caption"]') as HTMLElement;
    expect(caption).toHaveClass('text-body', 'max-w-[640px]');
    expect(caption.textContent!.length).toBeLessThanOrEqual(160);
    expect(legend.compareDocumentPosition(caption) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('moves the sentence with numbers, the exact amounts per class and the unspendable note to tier 2 (C1)', async () => {
    render(<ScriptsPage />);
    await screen.findByRole('list', { name: 'Who can change the code, in shares' });
    expect(within(tier1()).queryByTestId('code-class-sentence')).toBeNull();
    expect(within(tier2()).getByTestId('code-class-sentence')).toHaveTextContent(
      /^Of the 49,794,300,000(\.0+)? CKB under these 41 lock scripts, /
    );
    const exact = within(tier2()).getByRole('list', { name: 'Code classes' });
    expect(exact).toHaveTextContent('1,727,000,000');
    expect(exact).toHaveTextContent('· 18 scripts');
    // Tier 1 states no exact class amount (R12: they wait in tier 2).
    expect(tier1().textContent).not.toContain('1,727,000,000');
    expect(within(tier1()).queryByTestId('unspendable-note')).toBeNull();
    // Tier 2's two sections side by side from lg, as the DAO page's evidence (page height).
    expect(
      within(tier2()).getByRole('heading', { name: 'How scripts work' }).closest('section')!
        .parentElement
    ).toHaveClass('grid', 'lg:grid-cols-2');
    expect(within(tier2()).getByTestId('unspendable-note')).toHaveTextContent(
      'secp256k1 lock over 20 zero bytes'
    );
  });

  it('heads the table with the All / Locks / Types tabs, 64 px under the hero (C0, R6)', async () => {
    render(<ScriptsPage />);
    const list = await screen.findByRole('region', { name: 'Script list' });
    expect(list).toHaveClass('mt-16');
    const tabs = within(list).getByRole('group', { name: 'Role' });
    expect(hero()).not.toContainElement(tabs);
    const table = await within(list).findByRole('table', { name: 'Scripts' });
    expect(tabs.compareDocumentPosition(table) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(within(tabs).getByRole('button', { name: 'Types 34' })).toHaveClass('text-section');
  });

  it('keeps the figure on every script when a tab filters the table', async () => {
    render(<ScriptsPage />);
    const amount = await within(hero()).findByText(
      (_, el) => el?.getAttribute('data-role') === 'figure-amount'
    );
    vi.mocked(api.getScripts).mockResolvedValue({
      ...withSummary,
      classSummary: [classSummary[2]],
    });
    fireEvent.click(screen.getByRole('button', { name: 'Types 34' }));
    await waitFor(() =>
      expect(api.getScripts).toHaveBeenLastCalledWith(expect.objectContaining({ role: 'type' }))
    );
    // The hero's request never carries the role: the tab is the table's.
    for (const [params] of vi.mocked(api.getScripts).mock.calls)
      if (params?.limit === 1) expect(params.role).toBeUndefined();
    expect(amount).toHaveTextContent(/^49,794,300,000\.0+ CKB$/);
  });

  it('writes no coloured text in tier 1: the figure is bright and the bar keeps its colours (R11)', async () => {
    render(<ScriptsPage />);
    await screen.findByRole('table', { name: 'Scripts' });
    await screen.findByRole('list', { name: 'Who can change the code, in shares' });
    const coloured = [...tier1().querySelectorAll('[class]')].filter((el) =>
      el
        .getAttribute('class')!
        .split(/\s+/)
        .some((c) => COLOURED_TEXT.test(c))
    );
    expect(coloured.map((el) => el.outerHTML.slice(0, 80))).toEqual([]);
  });

  it('keeps tier 1 within the prose budget (§0, §4)', async () => {
    const { container } = render(<ScriptsPage />);
    await screen.findByRole('table', { name: 'Scripts' });
    await screen.findByRole('list', { name: 'Who can change the code, in shares' });
    expectTier1Budget(container, { maxProseBlocks: 3, maxChars: 160 });
  });
});

// Board L6 (responsive plan, L3): a script family is three lines on a phone, two at 640.
describe('Scripts list phone row (board L6)', () => {
  beforeEach(() => {
    vi.mocked(api.getScripts).mockResolvedValue(response);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('is three lines at 358 px: name and CKB · role, code, counts · the description in full', async () => {
    atContainerWidth(358);
    render(<ScriptsPage />);
    const rows = await screen.findAllByTestId('script-row-mobile');
    expect(screen.queryByRole('table', { name: 'Scripts' })).toBeNull();
    expect(phoneLines(rows[0])).toEqual([
      { key: 'identity', tone: 'head', left: 'JoyID', right: '456,002,179.57000000 CKB' },
      {
        key: 'code',
        tone: 'body',
        left: 'lock·Upgradable · 5 versions· by a multisig · since 2023-10-07',
        right: '161,049 live · of 2,707,778 · 4.19% occupied',
      },
      {
        key: 'description',
        tone: 'quiet',
        left: 'An universal Account Protocol for Web3 Mass-adoption',
        right: null,
      },
    ]);
    // Never clamped (C#12): the whole sentence, no line clamp anywhere in the row.
    expect(rows[0].querySelector('[class*="line-clamp"]')).toBeNull();
    expect(phoneLines(rows[1])[1].left).toMatch(/^lock·Immutable· genesis · code cell unspendable/);
    expect(rows[2]).toHaveTextContent('Godwoken Custodian Lockdeprecated');
  });

  it('is two lines at 608 px: role and code beside the name, counts beside the description', async () => {
    atContainerWidth(608);
    render(<ScriptsPage />);
    const [row] = await screen.findAllByTestId('script-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      [
        'identity',
        'JoyID · lock·Upgradable · 5 versions· by a multisig · since 2023-10-07',
        '456,002,179.57000000 CKB',
      ],
      [
        'description',
        'An universal Account Protocol for Web3 Mass-adoption',
        '161,049 live · of 2,707,778 · 4.19% occupied',
      ],
    ]);
  });
});
