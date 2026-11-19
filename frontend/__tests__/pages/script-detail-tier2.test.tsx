import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import {
  dottedItems,
  phoneHeadPx,
  phoneLines,
  statedHeadPx,
  stubLayoutWidths,
} from '@/__tests__/utils/phone-row';
import { inkOf } from '@/__tests__/utils/ink-role';
import { shortenAddress } from '@/lib/shorten';
import fixture from '@/__tests__/fixtures/scripts/joyid-family-detail.json';
import { byScript, rowDetail } from '@/__tests__/utils/script-newest-cells';
import ScriptDetailPage from '@/app/scripts/[name]/client-page';
import { api, type Cell } from '@/lib/api';
import { TYPE_SCALE } from '@/lib/type-scale';

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
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div /> }));
vi.mock('@/src/navigation', () => ({
  useParams: () => ({ name: 'JoyID' }),
  usePathname: () => '/scripts/JoyID',
  useSearchParams: () => new URLSearchParams(),
}));

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
const tier2 = () => document.querySelector('[data-tier="2"]') as HTMLElement;

// Table-rows plan L1-5 (board ScriptDetail): "Its cells" keeps its one-line rows with the key cell
// at 13 px bright (T5); tier 2 is trimmed to the board's two blocks — "How to read this page" as
// one paragraph and "The script" as one definition list per reference. Every sentence and fact the
// four former sections held is in one of the two.
describe('Script page · Its cells and tier 2 (table-rows L1-5)', () => {
  beforeEach(() => {
    stubLayoutWidths({ chart: 900, list: 1248 });
    vi.mocked(api.getScriptFamilyDetail).mockResolvedValue(fixture.family as never);
    vi.mocked(api.getScriptUsage).mockResolvedValue(fixture.usage);
    vi.mocked(api.getScriptCapacityChart).mockResolvedValue(fixture.chart);
    // Lane S: the newest cells read the same fixture rows newest first, and each row's detail.
    vi.mocked(api.getCell).mockImplementation(
      async (tx, index) => rowDetail(fixture.cells.data as never, tx, index) ?? codeCell(tx, index)
    );
    vi.mocked(api.getCellsByScriptRef).mockImplementation(byScript(() => fixture.cells as never));
    vi.mocked(api.lookupScripts).mockResolvedValue({});
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('leads each of its cells with the cell at 13 px bright, the rest of the row in body ink (T5)', async () => {
    await renderJoyid();
    const table = screen.getByRole('table', { name: 'Its cells' });
    const [row] = within(table).getAllByRole('row').slice(1);
    const key = within(row).getByRole('link', { name: '0x44e5e96c…f8517f:1' });
    expect(key).toHaveClass('text-body', 'text-text-bright', 'leading-row');
    expect(row).toHaveClass('text-text');
    expect(row.querySelectorAll('.text-text-bright')).toHaveLength(1);
    // T5: column heads at 11 px.
    for (const h of within(table).getAllByRole('columnheader'))
      expect(h.firstElementChild).toHaveClass('text-meta');
  });

  // Readability review 2026-11-18 (lane A): the heads of "Registered references" ran together,
  // "hash typecode hash" (script-family--n--04): the hash type column was budgeted for its values
  // ("data1", 5 characters) and not for its head, so "hash type" overran it into "code hash". A
  // head is one line at the meta step, its characters at the mono advance (0.6 em), and a cell's
  // right padding (16 px, none on the last column) keeps it from the next; the table still stands
  // in tier 2's right column at 1280, (1232 − 56) / 2 = 588 px, as a table.
  it('budgets every column of Registered references for its head, in the 588 px it has at 1280', async () => {
    await renderJoyid();
    const table = within(tier2()).getByRole('table', { name: 'Registered references' });
    const advance = 0.6 * parseFloat(TYPE_SCALE.meta[0]);
    const heads = within(table).getAllByRole('columnheader');
    const budgets = Array.from(table.querySelectorAll('col')).map((col) =>
      parseFloat(col.style.minWidth)
    );
    expect(budgets).toHaveLength(heads.length);
    const short = heads.flatMap((head, i) => {
      const needs = head.textContent!.length * advance + (i < heads.length - 1 ? 16 : 0);
      return budgets[i] >= needs ? [] : [`${head.textContent} needs ${needs}, has ${budgets[i]}`];
    });
    expect(short).toEqual([]);
    expect(budgets.reduce((sum, w) => sum + w, 0)).toBeLessThanOrEqual((1232 - 56) / 2);
  });

  it('keeps tier 2 to the two board blocks: one paragraph and the script facts', async () => {
    await renderJoyid();
    const headings = within(tier2())
      .getAllByRole('heading', { level: 2 })
      .map((h) => h.textContent);
    expect(headings).toEqual(['How to read this page', 'The script']);
    const read = within(tier2())
      .getByRole('heading', { name: 'How to read this page' })
      .closest('section')!;
    expect(read.querySelectorAll('p')).toHaveLength(1);
    const paragraph = read.querySelector('p')!;
    // Every sentence the former sections wrote, in one paragraph.
    expect(within(paragraph).getByTestId('script-summary')).toHaveTextContent(
      'JoyID decides who can spend 161,049 live cells'
    );
    expect(within(paragraph).getByTestId('script-history-peak')).toHaveTextContent(
      // re-pinned on purpose (lane P2's follow-up, item 5): a sentence writes the summary form (it was
      // "456,002,179.57000000")
      /CKB held peaked at 456,002,179 CKB on 2026-10-29/
    );
    expect(paragraph).toHaveTextContent('A cell must hold enough CKB to pay for its own bytes.');
    expect(paragraph).toHaveTextContent(
      "JoyID cells point at their code through one code cell's type id, not at a fixed binary."
    );
    expect(paragraph).toHaveTextContent(
      'Code versions: one type id, 0xd00c84f0…318323, 5 binaries.'
    );
  });

  // Lane E (C6): one map of the script's shape and one row per reference, instead of one definition
  // list per reference; every fact of the former lists is in a row, its title, or the fold below.
  it('draws the script once and gives each reference one row, the full facts folded', async () => {
    await renderJoyid();
    const facts = within(tier2()).getByRole('heading', { name: 'The script' }).closest('section')!;
    expect(within(facts).getByTestId('script-cells-summary')).toHaveTextContent(
      '161,049 live cells use JoyID as their lock'
    );
    expect(facts).toHaveTextContent('Script · 33 bytes, then the args each cell sets');
    expect(
      [...within(facts).getByRole('group', { name: 'lock script byte map' }).children].map(
        (p) => (p as HTMLElement).dataset.segment
      )
    ).toEqual(['code_hash', 'hash_type']);
    const table = within(facts).getByRole('table', { name: 'Registered references' });
    const rows = within(table).getAllByRole('row').slice(1);
    expect(rows).toHaveLength(fixture.family.references.length);
    expect(rows[0]).toHaveTextContent('type');
    expect(rows[0]).toHaveTextContent('161,049');
    expect(rows[0]).toHaveAttribute(
      'title',
      expect.stringContaining('0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323')
    );
    expect(within(rows[0]).getByRole('link', { name: '0x673e1843…1355f9:0' })).toHaveAttribute(
      'href',
      '/mainnet/cell/0x673e1843035ce9c249efc36b62865e7a7c9f6e2ca6fc9c66fd108307e81355f9-0'
    );
    await waitFor(() => expect(rows[0]).toHaveTextContent('A multisig can replace'));
    expect(facts).toHaveTextContent(
      'A type reference follows every upgrade; its data hash with hash type data1 or data2 pins this exact binary; an upgrade will not reach cells that pin it.'
    );
    // the former definition list, per reference, in one fold
    const full = within(facts).getByText('Each reference in full').closest('details')!;
    expect(full).not.toHaveAttribute('open');
    // the library's inner disclosure in tier 2 (lane cd-fixes-controls, the disclosure sweep): its
    // words 13 px in the quiet ink, as they were (they took the 13 px from the section around them)
    expect(full.querySelector('summary')).toHaveClass('text-small', 'text-text-dim');
    const list = full.querySelector('dl')!;
    expect(
      within(list)
        .getAllByRole('term')
        .map((t) => t.textContent)
    ).toEqual(['code hash', 'hash type', 'current code', 'pin instead', 'cell dep', 'held by']);
    const dd = (term: string) =>
      within(list)
        .getAllByRole('term')
        .find((t) => t.textContent === term)!.nextElementSibling as HTMLElement;
    expect(dd('code hash')).toHaveTextContent(
      /^0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323$/
    );
    expect(dd('hash type')).toHaveTextContent(
      /^type · follows every upgrade · 161,049 live cells$/
    );
    expect(dd('current code')).toHaveTextContent(
      /^0x4af7d832e7c328aa4ad6f2deb669743446aea7b9a2f0c25646897501ac4f1ebe$/
    );
    expect(dd('cell dep')).toHaveTextContent(
      '0x673e1843035ce9c249efc36b62865e7a7c9f6e2ca6fc9c66fd108307e81355f9:0 · dep type code · the live code cell of the current version'
    );
    expect(dd('held by')).toHaveTextContent('Lock args 0x65983f1d7520543a6f182a50354fb4ae2deed696');
  });
});

// Readability F3-e: every column of the family page's tables budgeted from the widest value it
// writes, at 0.6 em a glyph (keys 14 px, cells 13, heads, tags and notes 12) and the 16 px gap after
// every column but the last; a table's flexible column takes a stated minimum and the rest of the
// container. Each list changes form where the sum of its columns stops fitting.
describe('Script page · the family tables budgeted from what they hold (readability F3-e)', () => {
  const at = (text: string, px: number) => (text.length * px * 3) / 5; // 0.6 em, as textPx
  const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
  const sum = (budgets: number[]) => budgets.reduce((s, w) => s + w, 0);
  const references = [
    // its head, wider than "data1"
    ceil(at('hash type', 12)) + 16,
    // the code hash at the row size
    ceil(at('0x12345678…abcdef', 13) + 16),
    // its head, wider than a reference's live cells (seven digits)
    ceil(Math.max(at('live cells', 12), at('9,999,999', 13))) + 16,
    // the code cell's out point at the row size (an index of two digits) and " +9" more
    ceil(at('0x12345678…abcdef:99 +9', 13) + 16),
    // the flexible column, the last: its head, and nine characters of the holder's words
    ceil(Math.max(at('held by', 12), at('x'.repeat(9), 13))),
  ];
  const versions = [
    // its head, wider than "v99" at 14 px
    ceil(Math.max(at('v99', 14), at('Version', 12)) + 16),
    // the date
    ceil(at('2023-10-07', 13) + 16),
    // the block
    ceil(at('#99,999,999', 13) + 16),
    // the out point at the row size, its 8 px gap and a "+9" tag
    ceil(at('0x12345678…abcdef:99', 13) + 8 + at('+9', 12) + 12 + 16),
    // the flexible column: a holder by its kind and its tight id, "multisig 0x1234…abcd"
    ceil(at('multisig 0x1234…abcd', 13) + 16),
    // "consumed in" and the replacing transaction at the row size
    ceil(at('consumed in 0x12345678…abcdef', 13) + 16),
    // eight digits of live cells, or "unavailable"; the last column
    ceil(Math.max(at('99,999,999', 13), at('unavailable', 13))),
  ];
  const narrowVersions = [
    // "v99", "current" under it at 12 px
    ceil(Math.max(at('v99', 13), at('current', 12)) + 16),
    // the date, the block under it, a link, at 13 px (re-pinned on purpose, the ink and size roles
    // round: it was 12 px, 96)
    ceil(Math.max(at('2023-10-07', 13), at('#99,999,999', 13)) + 16),
    // the flexible column: the out point at the row size; its notes and the hash wrap under it
    ceil(at('0x12345678…abcdef:99', 13) + 16),
    // a status breaks before the hash it names; the hash is one unit
    ceil(at('0x12345678…abcdef', 13) + 16),
    ceil(Math.max(at('99,999,999', 13), at('unavailable', 13))),
  ];
  const cells = [
    // the out point at 14 px, an index of four digits
    ceil(at('0x12345678…abcdef:9999', 14) + 16),
    // the owner's address at the row size
    ceil(at('ckb1…4s93hhsj', 13) + 16),
    // the flexible column: a type's code hash at the row size (a name takes the rest, wrapping)
    ceil(at('0x12345678…abcdef', 13) + 16),
    // the anatomy bar (48 px at least), or "unavailable" when it is not known
    ceil(Math.max(48, at('unavailable', 13)) + 16),
    // a cell's capacity under ten billion CKB, every decimal, and its unit
    ceil(at('9,999,999,999.99999999', 13) + at(' CKB', 12) + 16),
    // occupied bytes under ten million, or "unavailable"
    ceil(Math.max(at('9,999,999 B', 13), at('unavailable', 13)) + 16),
    // the block and the date, the last column
    ceil(at('#99,999,999 · 2026-11-02', 13)),
  ];

  const mount = async (list: number) => {
    stubLayoutWidths({ chart: 900, list });
    vi.mocked(api.getScriptFamilyDetail).mockResolvedValue(fixture.family as never);
    vi.mocked(api.getScriptUsage).mockResolvedValue(fixture.usage);
    vi.mocked(api.getScriptCapacityChart).mockResolvedValue(fixture.chart);
    vi.mocked(api.getCell).mockImplementation(
      async (tx, index) => rowDetail(fixture.cells.data as never, tx, index) ?? codeCell(tx, index)
    );
    vi.mocked(api.getCellsByScriptRef).mockImplementation(byScript(() => fixture.cells as never));
    vi.mocked(api.lookupScripts).mockResolvedValue({});
    const shown = render(<ScriptDetailPage name="JoyID" />);
    await screen.findByRole('heading', { name: 'JoyID', level: 1 });
    return shown;
  };
  /** The form the list labelled `label` chose. */
  const formOf = async (label: string) =>
    (
      await waitFor(() => {
        const list =
          screen.queryByRole('table', { name: label }) ??
          screen.queryByRole('list', { name: `${label}, compact` });
        if (!list) throw new Error(`no ${label}`);
        return list;
      })
    )
      .closest('[data-list-form]')!
      .getAttribute('data-list-form');
  const minWidths = (label: string) =>
    Array.from(screen.getByRole('table', { name: label }).querySelectorAll('col')).map((c) =>
      Number.parseFloat((c as HTMLElement).style.minWidth)
    );
  /** At each container width, the form each named list chose. */
  const formsAt = async (cases: readonly (readonly [number, Record<string, string>])[]) => {
    for (const [list, expected] of cases) {
      const { unmount } = await mount(list);
      for (const [label, form] of Object.entries(expected))
        expect(await formOf(label), `${label} at ${list}`).toBe(form);
      unmount();
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    }
  };
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('pins every column of the four tables to its content', async () => {
    expect(references).toEqual([81, 149, 88, 196, 71]);
    expect(versions).toEqual([67, 94, 102, 207, 172, 243, 86]);
    expect(narrowVersions).toEqual([67, 102, 172, 149, 86]);
    expect(cells).toEqual([201, 118, 149, 102, 217, 102, 188]);
    expect([sum(references), sum(versions), sum(narrowVersions), sum(cells)]).toEqual([
      585, 971, 576, 1077,
    ]);
    await mount(1248);
    await formOf('Its cells');
    expect(minWidths('Registered references')).toEqual(references);
    expect(minWidths('Code versions')).toEqual(versions);
    expect(minWidths('Its cells')).toEqual(cells);
    // Registered references stays a table in tier 2's right column at 1280, (1232 − 56) / 2
    expect(sum(references)).toBeLessThanOrEqual((1232 - 56) / 2);
  });

  it('draws the 608 px versions table from its own columns', async () => {
    await mount(608);
    expect(await formOf('Code versions')).toBe('narrow-table');
    expect(minWidths('Code versions')).toEqual(narrowVersions);
  });

  it('changes Its cells and the versions to their next form where their sums stop fitting', async () => {
    await formsAt([
      [1232, { 'Its cells': 'table', 'Code versions': 'table' }],
      [1077, { 'Its cells': 'table' }],
      [1076, { 'Its cells': 'rows' }],
    ]);
  });

  // re-pinned on purpose (the ink and size roles round): the narrow table's sum is 576 (570 with the
  // deployment's block at 12 px)
  it('changes the versions to the 608 px table below 971 and to rows below 576', async () => {
    await formsAt([
      [971, { 'Code versions': 'table' }],
      [970, { 'Code versions': 'narrow-table' }],
      [576, { 'Code versions': 'narrow-table' }],
      [575, { 'Code versions': 'rows' }],
    ]);
  });

  it('changes Registered references to rows below 585, and every list on a phone', async () => {
    await formsAt([
      [585, { 'Registered references': 'table' }],
      [584, { 'Registered references': 'rows' }],
      [358, { 'Its cells': 'rows', 'Code versions': 'rows', 'Registered references': 'rows' }],
    ]);
  });
});

// Lane D4 (the phone-rows board): a script's cells on a phone were five lines (PA-08), the owner and
// the type joined by a bare "· —" where there is none (PA-09). The row is the board's three parts:
// the out point and the cell's capacity; who owns it, the type that also guards it and where it was
// made, each after its name (a cell with no type script says nothing of one); the list's one bar,
// what the cell holds, with the bytes it occupies named.
describe('Script page · its cells on a phone (lane D4)', () => {
  beforeEach(() => {
    stubLayoutWidths({ chart: 326, list: 358 });
    vi.mocked(api.getScriptFamilyDetail).mockResolvedValue(fixture.family as never);
    vi.mocked(api.getScriptUsage).mockResolvedValue(fixture.usage);
    vi.mocked(api.getScriptCapacityChart).mockResolvedValue(fixture.chart);
    vi.mocked(api.getCell).mockImplementation(
      async (tx, index) => rowDetail(fixture.cells.data as never, tx, index) ?? codeCell(tx, index)
    );
    vi.mocked(api.getCellsByScriptRef).mockImplementation(byScript(() => fixture.cells as never));
    vi.mocked(api.lookupScripts).mockResolvedValue({});
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('is three parts at 358 px: out point and capacity, owner · type · created, the bar and its bytes', async () => {
    render(<ScriptDetailPage name="JoyID" />);
    await screen.findByRole('heading', { name: 'JoyID', level: 1 });
    const section = document.getElementById('its-cells')!;
    const rows = await within(section).findAllByTestId('script-cell-row-mobile');
    const [plain, typed] = fixture.cells.data;
    expect(phoneLines(rows[0]).map((l) => [l.key, l.tone, l.left, l.right])).toEqual([
      ['cell', 'head', '0x44e5…517f:1', '153.99 CKB'],
      [
        'held',
        'body',
        `owner ${shortenAddress(plain.address, 'tight')} · created #11,141,234`,
        null,
      ],
      ['anatomy', 'quiet', '', '63 B occupied'],
    ]);
    expect(dottedItems(rows[1].querySelector('[data-line="held"]')!)).toEqual([
      `owner ${shortenAddress(typed.address, 'tight')}`,
      'type COTA',
      'created #11,286,483',
    ]);
    // no bare dash where a cell has no type script (PA-09)
    expect(rows[0].textContent).not.toContain('—');
    // every figure after its name, the name in the quiet ink
    const labels = Array.from(rows[1].querySelectorAll('[data-role="figure-label"]'));
    expect(labels.map((l) => l.textContent)).toEqual(['owner', 'type', 'created', 'occupied']);
    for (const label of labels) expect(inkOf(label)).toBe('text-text-dim');
    // the list's one bar stands on line 3 beside the bytes that name it
    expect(
      rows[0].querySelector('[data-line="anatomy"] [data-part="left"]')!.children
    ).toHaveLength(1);
    // the out point is the row's key, its one bright thing
    expect(statedHeadPx(rows[0])).toBe(276.6);
    expect(phoneHeadPx(rows[0], 360)).toBeLessThanOrEqual(statedHeadPx(rows[0]));
  });
});
