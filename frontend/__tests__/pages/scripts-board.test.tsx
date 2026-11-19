import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  atContainerWidth,
  dottedItems,
  phoneHeadPx,
  phoneLines,
  statedHeadPx,
} from '@/__tests__/utils/phone-row';
import { inkOf } from '@/__tests__/utils/ink-role';
import { PHONE_LINE_PX } from '@/lib/type-metrics';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { expectTier1Budget } from '@/__tests__/utils/hierarchy';
import { screen, waitFor, within, fireEvent } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ScriptsPage from '@/app/scripts/page';
import { DEPRECATED_TITLE } from '@/components/scripts/script-code-evidence';
import { api, type KnownScript } from '@/lib/api';
import { fullText } from '@/__tests__/utils/full-text';

vi.mock('@/lib/api', () => ({
  api: { getScripts: vi.fn() },
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
  // mid-sentence on narrow widths. Table-rows L1-1 (D3): the table row no longer shows it — the
  // whole sentence is the row's title, and the phone row writes it in full (never clamped).
  it('keeps the whole description in the row title, never clamped', async () => {
    render(<ScriptsPage />);
    const table = await screen.findByRole('table', { name: 'Scripts' });
    const row = within(table).getByRole('link', { name: 'JoyID' }).closest('tr') as HTMLElement;
    expect(row.getAttribute('title')).toContain(
      ' · An universal Account Protocol for Web3 Mass-adoption · '
    );
    expect(row).not.toHaveTextContent('An universal Account Protocol');
    expect(table.querySelector('[class*="line-clamp"]')).toBeNull();
  });

  it('keeps the board lead sentence (tier 2 since v3) and a name search (SC-L5)', async () => {
    render(<ScriptsPage />);
    expect(
      screen.getByText(
        'Every cell runs one lock script, which decides who can spend it, and may run one type script, which decides what the cell is and how it may change.'
      )
    ).toBeInTheDocument();
    expect(screen.getByPlaceholderText('search by name or code hash')).toBeInTheDocument();
  });

  // Re-pinned on purpose (lists lane P3, the plan: "upgradable" with who can upgrade is the
  // reading ink): the cell says who can replace the code, in the reading ink; the versions are its
  // title (renamed: "… upgradable · N versions, by whom in its title")
  it('writes the Code column as immutable or upgradable by whom, the versions in its title (SC-L2, SC-L4, L1-1)', async () => {
    render(<ScriptsPage />);
    const table = await screen.findByRole('table', { name: 'Scripts' });
    const row = (name: string) =>
      within(table).getByRole('link', { name }).closest('tr') as HTMLElement;
    const joyCode = within(row('JoyID')).getByTestId('script-code-class');
    expect(joyCode.textContent).toBe('upgradable by a multisig');
    expect(within(joyCode).getByText('upgradable by a multisig')).toHaveClass('text-text');
    expect(joyCode.getAttribute('title')).toContain('5\u00a0versions');
    expect(joyCode.getAttribute('title')).toContain('by a multisig');
    expect(within(row('Default Lock')).getByTestId('script-code-class').textContent).toBe(
      'immutable'
    );
    expect(table.textContent).not.toMatch(/Fixed bytecode|code classes/);
    // No class evidence from the API stays visible as such, never invented.
    expect(
      within(row('Godwoken Custodian Lock')).getByTestId('script-code-class').textContent
    ).toBe('code evidence unavailable');
  });

  // readability R5, re-pinned on purpose (and renamed: "..., 2 dp in the row, ..."): the row's CKB
  // is the list's summary form.
  it('shows "of N ever" and exact CKB in the title, the summary form in the row, and a rounded occupied share (SC-L6, SC-L7, L1-1)', async () => {
    render(<ScriptsPage />);
    const table = await screen.findByRole('table', { name: 'Scripts' });
    const joyRow = within(table).getByRole('link', { name: 'JoyID' }).closest('tr') as HTMLElement;
    expect(joyRow.textContent).toContain('161,049');
    expect(joyRow.getAttribute('title')).toContain('161,049 live of 2,707,778\u00a0cells ever');
    expect(joyRow.textContent).toContain('456,002,179');
    expect(joyRow.textContent).not.toContain('456,002,179.57');
    expect(joyRow.getAttribute('title')).toContain('456,002,179.57000000 CKB');
    expect(joyRow.textContent).not.toContain('456.00M');
    expect(joyRow.textContent).toContain('4.19%');
    const godRow = within(table)
      .getByRole('link', { name: 'Godwoken Custodian Lock' })
      .closest('tr') as HTMLElement;
    expect(godRow.textContent).toContain('<0.01%');
    expect(godRow.textContent).not.toContain('0.00%');
  });

  // Lane F follow-up 2: old "marks deprecated in grey, not red: a ghost row tag" → the warning
  // (user: the dim dashed tag read as a footnote and did not warn).
  it('marks deprecated as the warning: a rouge warn row tag that says what it means (SC-L8, L1-1)', async () => {
    render(<ScriptsPage />);
    const table = await screen.findByRole('table', { name: 'Scripts' });
    const mark = within(table).getByText('deprecated');
    expect(mark).toHaveAttribute('data-row-tag', 'warn');
    expect(mark).toHaveClass('text-rouge', 'border-rouge');
    expect(mark).not.toHaveClass('border-dashed');
    expect(mark).toHaveAttribute('title', DEPRECATED_TITLE);
  });

  // Re-pinned on purpose (controls lane C2): the role row is the controls board's tab row (a
  // tablist, the current tab on its 2 px celadon line, lowercase names), the sort its select
  // named by "sorted by", its choices lowercase. It was a group of aria-pressed buttons, the
  // select named "Sort scripts" and "CKB in their cells".
  it('uses underline role tabs and one "sorted by" control in board words (SC-L8)', async () => {
    render(<ScriptsPage />);
    const tabs = await screen.findByRole('tablist', { name: 'Role' });
    const all = within(tabs).getByRole('tab', { name: /^all/ });
    expect(all).toHaveAttribute('aria-selected', 'true');
    expect(all.className).toContain('border-b-2');
    expect(all).toHaveClass('border-jade');
    expect(screen.queryByText('Used (CKB)')).toBeNull();
    expect(screen.queryByText('Capacity (CKB)')).toBeNull();
    const sort = screen.getByRole('combobox', { name: 'sorted by' });
    expect(sort).toHaveValue('capacity');
    expect(within(sort).getByRole('option', { name: 'ckb in their cells' })).toBeInTheDocument();
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

  it('dates the code in its title: "since" for upgradable code, "genesis" for genesis code (SC-L4, L1-1)', async () => {
    render(<ScriptsPage />);
    const table = await screen.findByRole('table', { name: 'Scripts' });
    const code = (name: string) =>
      within(within(table).getByRole('link', { name }).closest('tr') as HTMLElement).getByTestId(
        'script-code-class'
      );
    expect(code('JoyID').getAttribute('title')).toContain('by a multisig · since 2023-10-07');
    expect(code('Default Lock').getAttribute('title')).toContain('genesis · code cell unspendable');
    expect(code('Godwoken Custodian Lock')).toHaveTextContent('code evidence unavailable');
  });

  // Re-pinned on purpose (controls lane C2): the controls board's search field, a searchbox (it
  // was a plain text input). Re-pinned on purpose (lists lane P3, the placement lane's note): it
  // searches as typed, as the inventory's field does, with no Search button and no "×"
  it('searches by code hash through the API as typed (SC-L5)', async () => {
    render(<ScriptsPage />);
    fireEvent.change(screen.getByRole('searchbox', { name: 'Search scripts' }), {
      target: { value: ' 0xd00c84f0 ' },
    });
    expect(screen.queryByRole('button', { name: 'Search' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Clear search' })).toBeNull();
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
    expect(screen.getByText(fullText('3 scripts'))).toBeInTheDocument();
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
    await within(hero()).findByText((_, el) => el?.getAttribute('data-role') === 'summary');
    const h1 = within(hero()).getByRole('heading', { level: 1, name: 'Scripts' });
    expect(h1).toHaveClass('text-index', 'text-text-bright');
    // The search sits on the h1's row.
    const row = h1.parentElement!;
    expect(row).toHaveClass('flex', 'items-baseline', 'justify-between');
    // Re-pinned on purpose (controls lane C2): the controls board's search field, a searchbox in
    // the field's well, 28 px from sm (it was a plain text input, 28 px at every width).
    // Re-pinned on purpose (lists lane P3): the field alone on the title row, no form and no Search
    // button (it searches as typed, as the inventory's field)
    const input = within(row).getByRole('searchbox', { name: 'Search scripts' });
    expect(input.closest('form')).toBeNull();
    expect(within(row).queryByRole('button')).toBeNull();
    expect(input.closest('label')).toHaveClass('sm:h-7');
    const lede = hero().querySelector('[data-role="lede"]') as HTMLElement;
    // ≤ 90 (R13), and one line: 71 characters of 13 px mono (7.8 px each) fill the 560 px measure.
    expect(lede.textContent!.length).toBeLessThanOrEqual(71);
    // re-pinned with readability R4 (lane B7): on the measure, 76 characters (was max-w-[560px])
    // re-pinned with readability R3 / R4 (lane B7): a lede is a sentence that explains, the
    // reading ink (was text-text-dim)
    expect(lede).toHaveClass('text-body', 'text-text', 'max-w-measure');
    // re-pinned on purpose (lists lane P3, the link-look gate): a way onward, its arrow the look's
    expect(within(lede).getByRole('link', { name: /^How it works/ })).toHaveAttribute(
      'href',
      '#evidence'
    );
    // The board's lead sentence, whole, is tier 2's first paragraph (nothing deleted).
    expect(tier1()).not.toHaveTextContent(LEAD);
    expect(within(tier2()).getByText(LEAD)).toBeInTheDocument();
    expect(document.getElementById('evidence')).toContainElement(tier2());
  });

  // Re-pinned on purpose (lists lane P3, the plan: "the aggregate becomes one sentence and a bar
  // of 12 px", so rows stand in the first screen): no display figure; the counts the API states,
  // in one sentence in the reading ink, and no CKB total, which summed the classes in the page
  // (renamed: "leads with the CKB under these scripts as the one figure …")
  it('says what all the scripts are in one sentence: the API’s counts, no total summed in the page', async () => {
    render(<ScriptsPage />);
    const sentence = await within(hero()).findByText(
      (_, el) => el?.getAttribute('data-role') === 'summary'
    );
    expect(sentence).toHaveTextContent(
      /^70\sscripts: 41\slocks and 34\stypes\. Who can change the locks’ code, by the CKB under them:$/
    );
    expect(sentence).toHaveClass('text-body', 'text-text', 'max-w-measure');
    expect(figure()).toBeNull();
    expect(tier1().textContent).not.toContain('49,794,300,000');
  });

  // Re-pinned on purpose (lists lane P3): a 12 px bar under the sentence, which names it, with no
  // words inside (the legend row names every segment); it was a 28 px bar under the figure, its
  // name a quiet label above it and the widest segment's words inside (renamed: "… as a 28 px bar
  // under the figure, names and shares inside the segments that fit")
  it('draws who can change the code as a 12 px bar under the sentence, its key the legend row (R12, R15, C1)', async () => {
    render(<ScriptsPage />);
    const bar = await within(hero()).findByRole('img', {
      name: 'CKB under lock scripts by who can change their code',
    });
    expect(bar).toHaveClass('h-3');
    expect(bar).toHaveAttribute('data-bar-size', 'line');
    expect(within(hero()).queryByRole('heading', { level: 2 })).toBeNull();
    const sentence = hero().querySelector('[data-role="summary"]')!;
    expect(sentence.compareDocumentPosition(bar) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const segments = [...bar.querySelectorAll<HTMLElement>('[data-code-class-segment]')];
    // A class without lock-owned CKB draws no segment; the order is immutable → multisig → one key.
    expect(segments.map((s) => s.dataset.codeClassSegment)).toEqual([
      'fixed',
      'multisig',
      'one_key',
    ]);
    // no words inside a 12 px bar: the legend row says each segment's name and share
    expect(segments.map((s) => s.textContent)).toEqual(['', '', '']);
  });

  // Re-pinned on purpose (lists lane P3): the caption moved to tier 2's "How scripts work", so the
  // first screen holds rows (renamed: "… and explains it in one caption under it")
  it('keys the bar in one quiet row, its caption in tier 2 (R12, R14, C1)', async () => {
    render(<ScriptsPage />);
    const legend = await screen.findByRole('list', { name: 'Who can change the code, in shares' });
    expect(legend).toHaveClass('text-small', 'text-text-dim');
    expect(
      within(legend)
        .getAllByRole('listitem')
        .map((li) => li.textContent)
    ).toEqual([
      'Immutable · pinned by data hash 96.46%',
      'A multisig can replace 3.47%',
      'One key can replace 0.07%',
    ]);
    for (const share of within(legend).getAllByText(/%$/)) expect(share).toHaveClass('text-text');
    expect(legend.textContent).not.toMatch(/\d\.\d{8}/);
    expect(tier1().querySelector('[data-role="caption"]')).toBeNull();
    const caption = tier2().querySelector('[data-role="caption"]') as HTMLElement;
    expect(caption).toHaveTextContent(/^Code pinned by its data hash/);
    expect(caption.textContent!.length).toBeLessThanOrEqual(160);
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

  // Re-pinned on purpose (lists lane P3): 40 px under the summary (it stood 64 under the figure)
  it('heads the table with the All / Locks / Types tabs, 40 px under the summary (C0, R6)', async () => {
    render(<ScriptsPage />);
    const list = await screen.findByRole('region', { name: 'Script list' });
    expect(list).toHaveClass('mt-10');
    // Re-pinned on purpose (controls lane C2): the controls board's tab row, a tablist at the
    // section step from sm (it was a group of buttons at the section step at every width).
    const tabs = within(list).getByRole('tablist', { name: 'Role' });
    expect(hero()).not.toContainElement(tabs);
    const table = await within(list).findByRole('table', { name: 'Scripts' });
    expect(tabs.compareDocumentPosition(table) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(within(tabs).getByRole('tab', { name: 'types 34' })).toHaveClass('sm:text-section');
  });

  // Re-pinned on purpose (lists lane P3): the summary sentence, where the figure stood (renamed:
  // "keeps the figure on every script …")
  it('keeps the summary of every script when a tab filters the table', async () => {
    render(<ScriptsPage />);
    const sentence = await within(hero()).findByText(
      (_, el) => el?.getAttribute('data-role') === 'summary'
    );
    vi.mocked(api.getScripts).mockResolvedValue({
      ...withSummary,
      classSummary: [classSummary[2]],
    });
    fireEvent.click(screen.getByRole('tab', { name: 'types 34' }));
    await waitFor(() =>
      expect(api.getScripts).toHaveBeenLastCalledWith(expect.objectContaining({ role: 'type' }))
    );
    // The hero's request never carries the role: the tab is the table's.
    for (const [params] of vi.mocked(api.getScripts).mock.calls)
      if (params?.limit === 1) expect(params.role).toBeUndefined();
    expect(sentence).toHaveTextContent(/^70\sscripts: 41\slocks and 34\stypes\./);
  });

  it('writes no coloured text in tier 1: the figure is bright and the bar keeps its colours (R11)', async () => {
    render(<ScriptsPage />);
    await screen.findByRole('table', { name: 'Scripts' });
    await screen.findByRole('list', { name: 'Who can change the code, in shares' });
    // Lane F follow-up: immutable code is the Code cell's accent row tag (jade), the class mark the
    // script page already allows ("live · current"); every other text in tier 1 stays ink.
    const accents = [...tier1().querySelectorAll<HTMLElement>('[data-row-tag="accent"]')];
    expect(accents.length).toBeGreaterThan(0);
    expect(new Set(accents.map((el) => el.textContent))).toEqual(new Set(['immutable']));
    // Lane F follow-up 2: and "deprecated" is the warn row tag, the one red status of the page.
    const warns = [...tier1().querySelectorAll<HTMLElement>('[data-row-tag="warn"]')];
    expect(new Set(warns.map((el) => el.textContent))).toEqual(new Set(['deprecated']));
    // re-pinned on purpose (lists lane P3): the lede's "How it works ↓" is a way onward, celadon by
    // the link rule, the one coloured link of tier 1 (the lead's ruling for the topic heroes' own)
    const coloured = [...tier1().querySelectorAll('[class]')].filter(
      (el) =>
        !el.matches('[data-row-tag="accent"], [data-row-tag="warn"], a[href="#evidence"]') &&
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

  // The coordinator's overflow sweep (F3-e): at a 360 px window the counts, one nowrap run of 45
  // glyphs at 13 px ("817,008 live · of 78,764,381 · 0.13% occupied", 351 px), stood 7 px past
  // the 328 px row. Re-pinned on purpose (lane D4, the phone-rows board): the counts are the bar's
  // figures on line 3, "N live · N% occupied", a separated list that breaks between its two items,
  // each whole; the cells ever left the row (its title keeps them). Beside the bar's least track
  // (48 px) and the 12 px, the widest of them, "99,999,999 live · 100.00% occupied" (34 characters
  // at 12 px, 244.8), stands inside 328 (304.8). They were two nowrap lines at the right of line 2,
  // live of ever over the share.
  it('writes the bar’s two figures on line 3, each whole, the widest beside the bar inside 328 px', async () => {
    atContainerWidth(328);
    render(<ScriptsPage />);
    const rows = await screen.findAllByTestId('script-row-mobile');
    const right = rows[0].querySelector('[data-line="share"] > [data-part="right"]')!;
    const items = Array.from(right.querySelectorAll('[data-role="figure-label"]')).map(
      (label) => label.parentElement!
    );
    expect(items.map((l) => l.textContent)).toEqual(['161,049 live', '4.19% occupied']);
    for (const item of items) expect(item).toHaveClass('whitespace-nowrap');
    const at = (text: string, px: number) => (text.length * px * 3) / 5;
    expect(48 + 12 + at('99,999,999 live · 100.00% occupied', 12)).toBeLessThanOrEqual(328);
  });

  // Re-pinned on purpose (lane D4, the phone-rows board; the new describe below pins the row): the
  // role, the code class and its facts are line 2 in words ("lock · upgradable · 5 versions · by a
  // multisig", was "lock·Upgradable · 5 versions· by a multisig · since 2023-10-07" beside the
  // counts), the bar and its two figures are line 3, and the description left the row for the
  // script page and the row's title (it was line 3, whole).
  it('is three lines at 358 px: name and CKB · role and code · the share bar and its figures', async () => {
    atContainerWidth(358);
    render(<ScriptsPage />);
    const rows = await screen.findAllByTestId('script-row-mobile');
    expect(screen.queryByRole('table', { name: 'Scripts' })).toBeNull();
    expect(phoneLines(rows[0])).toEqual([
      // a phone row's summary form (readability R5: was "456,002,179.57000000 CKB")
      { key: 'identity', tone: 'head', left: 'JoyID', right: '456,002,179 CKB' },
      {
        key: 'code',
        tone: 'body',
        left: 'lock · upgradable · 5 versions · by a multisig',
        right: null,
      },
      { key: 'share', tone: 'quiet', left: '', right: '161,049 live · 4.19% occupied' },
    ]);
    // Never clamped (C#12): no line clamp anywhere in the row.
    expect(rows[0].querySelector('[class*="line-clamp"]')).toBeNull();
    // Lane F follow-up: immutable is the line's accent tag, lower-case as in the table (was the
    // dim word "Immutable"); upgradable is plain text, no longer the bright one beside the name.
    expect(phoneLines(rows[1])[1].left).toMatch(/^lock · immutable · code cell unspendable/);
    expect(within(rows[1]).getByText('immutable')).toHaveAttribute('data-row-tag', 'accent');
    expect(within(rows[0]).getByText('upgradable')).not.toHaveClass('text-text-bright');
    // the deprecation opens line 2 (it stood after the name on line 1)
    expect(phoneLines(rows[2])[1].left).toMatch(/^deprecated · lock · /);
    // Lane F follow-up 2: the phone row's "deprecated" is the warn tag too (was dim text).
    expect(within(rows[2]).getByText('deprecated')).toHaveAttribute('data-row-tag', 'warn');
  });

  // PA-03 (UI review 2026-11-14) drew the "bar: CKB | occupied" switch with the table only.
  // Re-pinned on purpose (readability B5, plan §7.6): the CKB bar left the list, so the switch had
  // one state and went; the table now needs 851 px (it needed 1,034: the CKB column lost its
  // 240 px track), so the 976 px of a 1024 window takes the table, a phone and 640 the rows.
  it('draws no bar switch at any width; a 1024 window now takes the table (PA-03, B5)', async () => {
    for (const width of [358, 608]) {
      atContainerWidth(width);
      const view = render(<ScriptsPage />);
      await screen.findAllByTestId('script-row-mobile');
      expect(screen.queryByRole('group', { name: 'Bar shows' }), `${width} px`).toBeNull();
      view.unmount();
      vi.restoreAllMocks();
      vi.mocked(api.getScripts).mockResolvedValue(response);
    }
    for (const width of [976, 1232]) {
      atContainerWidth(width);
      const view = render(<ScriptsPage />);
      await screen.findByRole('table', { name: 'Scripts' });
      expect(screen.queryByRole('group', { name: 'Bar shows' }), `${width} px`).toBeNull();
      view.unmount();
      vi.restoreAllMocks();
      vi.mocked(api.getScripts).mockResolvedValue(response);
    }
  });

  // Re-pinned on purpose (lane D4): the card at 608 px is the phone row's three parts (it was two
  // lines, the role and code beside the name and the counts beside the description).
  it('is the phone row’s three parts at 608 px', async () => {
    atContainerWidth(608);
    render(<ScriptsPage />);
    const [row] = await screen.findAllByTestId('script-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      // a phone row's summary form (readability R5)
      ['identity', 'JoyID', '456,002,179 CKB'],
      ['code', 'lock · upgradable · 5 versions · by a multisig', null],
      ['share', '', '161,049 live · 4.19% occupied'],
    ]);
  });
});

// Lane D4 (the phone-rows board, its scripts pane): up to ten lines a row today (PA-08). Line 1 the
// name and the CKB its cells hold, the figure the list sorts by, at the key's step in the bright
// ink; line 2 what the script is, in words that wrap between their items: its role, the code's
// class (the pip and "immutable", or "upgradable", its versions and who can change it), a
// deprecation; line 3 the list's one bar, the occupied share, with the two figures that name it,
// "N live · N% occupied". The description leaves the list (the script page and the row's title
// hold it), as do the cells ever and the date the code is known from.
describe('the scripts phone row on the phone-rows board (lane D4)', () => {
  beforeEach(() => {
    vi.mocked(api.getScripts).mockResolvedValue(response);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('is three parts at 358 px: name and CKB, what it is, the share bar with its figures', async () => {
    atContainerWidth(358);
    render(<ScriptsPage />);
    const [joy, base, old] = await screen.findAllByTestId('script-row-mobile');
    expect(phoneLines(joy).map((l) => [l.key, l.tone, l.left, l.right])).toEqual([
      ['identity', 'head', 'JoyID', '456,002,179 CKB'],
      ['code', 'body', 'lock · upgradable · 5 versions · by a multisig', null],
      ['share', 'quiet', '', '161,049 live · 4.19% occupied'],
    ]);
    expect(phoneLines(base)[1].left).toBe(
      'lock · immutable · code cell unspendable · pinned by data hash'
    );
    expect(within(base).getByText('immutable')).toHaveAttribute('data-row-tag', 'accent');
    expect(phoneLines(old)[1].left).toBe('deprecated · lock · code evidence unavailable');
    expect(within(old).getByText('deprecated')).toHaveAttribute('data-row-tag', 'warn');
    // the figure the list sorts by stands at the key's step, bright
    const held = joy.querySelector('[data-line="identity"] > [data-part="right"] > *')!;
    expect(held).toHaveClass('text-body', 'text-text-bright');
    // the bar and the two figures that name it, each after its value's name, quiet
    const share = joy.querySelector('[data-line="share"]')!;
    expect(share.querySelector('[data-share-track]')).not.toBeNull();
    expect(dottedItems(share)).toEqual(['161,049 live', '4.19% occupied']);
    const labels = Array.from(share.querySelectorAll('[data-role="figure-label"]'));
    expect(labels.map((l) => l.textContent)).toEqual(['live', 'occupied']);
    for (const label of labels) expect(inkOf(label)).toBe('text-text-dim');
    // the description, the cells ever and the date leave the row (its title keeps them)
    expect(joy.textContent).not.toMatch(/Mass-adoption|2,707,778|2023-10-07/);
    expect(joy.closest('[role="listitem"]')!.getAttribute('title')).toMatch(
      /Mass-adoption.*since 2023-10-07.*of 2,707,778|Mass-adoption.*2,707,778/
    );
  });

  // The board's head(): a 20-character name at the key's 14 px (168), the 12 px between the parts
  // and the widest CKB a row's cells hold, "99,999,999,999" at 14 px (117.6) and " CKB" at the
  // unit's 12 px (28.8): 326.4 of a 360 px phone's 328 px line. Ten of the 75 names the registry
  // writes are longer (21 to 25 characters): beside the widest figure the figure drops under them.
  it('states its first line, 326.4 of 328 px, and draws a 20-character name with the widest figure', async () => {
    atContainerWidth(PHONE_LINE_PX[360]);
    vi.mocked(api.getScripts).mockResolvedValue({
      ...response,
      data: [
        family({
          codeHash: 'family:intent',
          name: 'UTXOSwap Intent Lock',
          ownedCapacitySum: '9999999999999999999',
          ownedKnowledgeSum: '100000000',
        }),
      ],
    });
    render(<ScriptsPage />);
    const [row] = await screen.findAllByTestId('script-row-mobile');
    expect(phoneLines(row)[0].right).toBe('99,999,999,999 CKB');
    expect(statedHeadPx(row)).toBe(326.4);
    expect(phoneHeadPx(row, 360)).toBe(statedHeadPx(row));
  });
});
