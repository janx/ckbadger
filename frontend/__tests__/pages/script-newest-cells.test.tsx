import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { phoneLines, stubLayoutWidths } from '@/__tests__/utils/phone-row';
import fixture from '@/__tests__/fixtures/scripts/newest-cells.json';
import ScriptDetailPage from '@/app/scripts/[name]/client-page';
import { api, type Cell } from '@/lib/api';
import { expectPrompt } from '@/__tests__/utils/prompt-heading';
import { pagedRead } from '@/__tests__/utils/paged-read';

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
  ApiRequestError: class ApiRequestError extends Error {},
  isNotFoundApiError: () => false,
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div /> }));
const route = vi.hoisted(() => ({ name: 'Default Lock' }));
vi.mock('@/src/navigation', () => ({
  useParams: () => ({ name: route.name }),
  usePathname: () => `/scripts/${route.name}`,
  useSearchParams: () => new URLSearchParams(),
}));

type ScriptName = keyof typeof fixture.scripts;
const EMPTY = pagedRead({ data: [], total: 0, limit: 12, hasMore: false, nextCursor: null });

/**
 * The lane's real captures (worktree API on the live store, tip #20,911,460): each script's family
 * detail, usage, one `order=desc` page per registered reference and the detail of every listed
 * cell. "Its cells" (the oldest-first list) is answered empty: this file is about the newest.
 */
function serve(name: ScriptName, newest?: (key: string) => unknown) {
  route.name = name;
  const script = fixture.scripts[name];
  vi.mocked(api.getScriptFamilyDetail).mockResolvedValue(script.family as never);
  vi.mocked(api.getScriptUsage).mockResolvedValue(script.usage as never);
  vi.mocked(api.getScriptCapacityChart).mockResolvedValue({
    data: [],
    series: [],
    title: '',
    totalPeak: null,
  });
  vi.mocked(api.lookupScripts).mockResolvedValue({});
  vi.mocked(api.getCellsByScriptRef).mockImplementation(async (params) => {
    if (params.order !== 'desc') return EMPTY;
    const key = `${params.codeHash}:${params.hashType}`;
    const page = newest ? newest(key) : script.newest[key as keyof typeof script.newest];
    if (!page) throw new Error(`no desc page captured for ${key}`);
    return page as never;
  });
  vi.mocked(api.getCell).mockImplementation(async (txHash, outputIndex) => {
    const detail = (script.details as Record<string, unknown>)[`${txHash}:${outputIndex}`];
    if (detail) return detail as Cell;
    // A code cell of a version (the versions table reads it); not what this file is about.
    return {
      txHash,
      outputIndex,
      capacity: '0',
      commonKnowledgeSize: '0',
      lockScriptHash: '0x01',
      dataSize: 0,
      createdAtBlock: 0,
    };
  });
}

async function renderPage() {
  render(<ScriptDetailPage name={route.name} />);
  return screen.findByRole('region', { name: 'Newest cells' });
}
const tableRows = (section: HTMLElement) =>
  within(within(section).getByRole('table', { name: 'Newest cells' }))
    .getAllByRole('row')
    .slice(1);

describe('Script page · Newest cells (lane S, S1/S2)', () => {
  beforeEach(() => stubLayoutWidths({ chart: 900, list: 1248 }));
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('puts the section in tier 1 after the hero, before who can change the code and its cells', async () => {
    serve('Default Lock');
    const section = await renderPage();
    const tier1 = document.querySelector('[data-tier="1"]') as HTMLElement;
    expect(tier1).toContainElement(section);
    // Phase 5 (A20): the family's cells command, run on the newest few the list reads.
    expectPrompt(within(section).getByRole('heading', { level: 2 }), 'Cells', '--newest 5');
    const order = ['Default Lock', 'Newest cells', 'Who can change its code', 'Its cells'].map(
      (name) => screen.getByRole('region', { name })
    );
    for (let i = 1; i < order.length; i++)
      expect(
        order[i - 1].compareDocumentPosition(order[i]) & Node.DOCUMENT_POSITION_FOLLOWING
      ).toBeTruthy();
    expect(section).toHaveClass('mt-16');
    expect(screen.getByRole('region', { name: 'Who can change its code' })).toHaveClass(
      'mt-[72px]'
    );
  });

  it('says in one caption what the list is, for a lock', async () => {
    serve('Default Lock');
    const section = await renderPage();
    const caption = section.querySelector('[data-role="caption"]') as HTMLElement;
    expect(caption.textContent).toBe(
      'The newest live cells this script locks, newest block first · it is not a list of transactions'
    );
    expect(caption.textContent!.length).toBeLessThanOrEqual(160);
    // re-pinned on purpose (lane P2's follow-up, item 4): the caption states what the list is, a
    // statement in the reading ink (it was the quiet ink)
    expect(caption).toHaveClass('text-small', 'text-text');
    // R16 measures a text node's parent box: the words sit in their own inline box, not the
    // full-width paragraph.
    expect(caption.children).toHaveLength(1);
    expect(caption.firstElementChild!.tagName).toBe('SPAN');
    expect(caption.firstElementChild!.textContent).toBe(caption.textContent);
  });

  it('reads each registered reference newest block first from its lock index, five at most', async () => {
    serve('Default Lock');
    await renderPage();
    const references = fixture.scripts['Default Lock'].family.references;
    expect(references).toHaveLength(3);
    for (const ref of references)
      expect(api.getCellsByScriptRef).toHaveBeenCalledWith({
        codeHash: ref.referenceHash,
        hashType: ref.hashType,
        scriptKind: 'lock',
        limit: 5,
        order: 'desc',
      });
  });

  it('lists the five newest of the three references, merged on the API key order', async () => {
    serve('Default Lock');
    const section = await renderPage();
    const rows = tableRows(section);
    expect(rows).toHaveLength(5);
    expect(rows.map((row) => within(row).getAllByRole('link')[0].textContent)).toEqual([
      // U-09 (re-pinned): the table's out points at the row size
      '0xfc79070b…5bcf91:0',
      '0x55d53352…2a136f:2',
      '0x55d53352…2a136f:1',
      '0x55d53352…2a136f:0',
      '0x3daee52a…b84722:2',
    ]);
    expect(
      within(within(section).getByRole('table', { name: 'Newest cells' }))
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual(['Cell', 'What it is', 'Owner', 'Created in']);
  });

  it('gives each row its out point as the one bright link, then what it is, owner and origin', async () => {
    serve('Default Lock');
    const section = await renderPage();
    const [row, , timeInfo] = tableRows(section);
    const first = fixture.scripts['Default Lock'].newest[
      '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8:type'
    ].data.find((c) => c.txHash.startsWith('0xfc7907'))!;
    const key = within(row).getByRole('link', { name: '0xfc79070b…5bcf91:0' });
    expect(key).toHaveAttribute('href', `/mainnet/cell/${first.txHash}-0`);
    expect(key).toHaveAttribute('title', `${first.txHash}:0`);
    expect(key).toHaveClass('text-body', 'text-text-bright');
    expect(row.querySelectorAll('.text-text-bright')).toHaveLength(1);
    expect(row).toHaveClass('text-text');
    // What it is: a cell with no type script is a plain cell; a typed one names its type script.
    await waitFor(() => expect(row).toHaveTextContent('plain cell'));
    await waitFor(() =>
      expect(within(timeInfo).getByRole('link', { name: '.bit Time Info' })).toHaveAttribute(
        'href',
        expect.stringMatching(/^\/mainnet\/script\/0x[0-9a-f]{64}\?hashType=type$/)
      )
    );
    // Owner, then "tx 0x… · #block · day". R9 (re-pinned): an address's row size is 4…8.
    expect(
      within(row).getByRole('link', {
        name: `${first.address!.slice(0, 4)}…${first.address!.slice(-8)}`,
      })
    ).toHaveAttribute('href', `/mainnet/address/${first.address}`);
    // A shortened link keeps the full id in its title (lane L's rule).
    expect(
      within(row).getByRole('link', {
        name: `${first.address!.slice(0, 4)}…${first.address!.slice(-8)}`,
      })
    ).toHaveAttribute('title', first.address);
    // re-pinned on purpose (lane P2's follow-up, item 11): the creating transaction opens at
    // this cell, the anchor P1's builder writes, so its fold never hides the cell
    expect(within(row).getByRole('link', { name: '0xfc79070b…5bcf91' })).toHaveAttribute(
      'href',
      `/mainnet/tx/${first.txHash}#${first.txHash}-0`
    );
    expect(within(row).getByRole('link', { name: '#20,911,460' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/20911460'
    );
    await waitFor(() =>
      expect(row).toHaveTextContent('tx 0xfc79070b…5bcf91 · #20,911,460 · 2026-11-03')
    );
    expect(row.getAttribute('title')).toContain(`created in tx ${first.txHash}`);
    expect(row.getAttribute('title')).toContain(`owner ${first.address}`);
    // Live cells only: no status word in a row.
    expect(row).not.toHaveTextContent(/\b(live|spent)\b/);
  });

  it('ends with a jump to its cells and no activity link for a generic lock', async () => {
    serve('Default Lock');
    const section = await renderPage();
    const footer = section.querySelector('[data-role="newest-footer"]') as HTMLElement;
    // re-pinned on purpose (lane p-assets, the controls gate's work list): a way onward to a place
    // on this page, the onward look writing its "↓" (it was a raw anchor typing "All … ↓")
    const all = within(footer).getByRole('link', { name: /^all 887,562\scells$/ });
    expect(all).toHaveAttribute('href', '#its-cells');
    expect(all).toHaveTextContent(/^all 887,562\scells\s↓$/);
    expect(all).toHaveClass('text-jade');
    expect(document.getElementById('its-cells')).toHaveAttribute('aria-label', 'Its cells');
    expect(within(footer).getAllByRole('link')).toHaveLength(1);
    expect(section.querySelector('a[href*="/activities"]')).toBeNull();
  });

  // Readability F3-e: every column budgeted from the widest value it writes, at 0.6 em a glyph (the
  // out point 14 px, the cells 13) and the 16 px gap after every column but the last. What it is is
  // the flexible column: 20 characters at its minimum, the rest of the container beyond the sum.
  it('budgets every column from what it holds and changes form at their sum (F3-e)', async () => {
    const at = (text: string, px: number) => (text.length * px * 3) / 5; // 0.6 em, as textPx
    const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
    const budgets = [
      // the out point at the row size, an output index of four digits
      ceil(at('0x12345678…abcdef:9999', 14) + 16),
      // the flexible column: 20 characters of what the cell is
      ceil(at('x'.repeat(20), 13) + 16),
      // an owner's address at the row size
      ceil(at('ckb1…4s93hhsj', 13) + 16),
      // "tx", the hash at the row size, the block and the date, the last column
      ceil(at('tx 0x12345678…abcdef · #99,999,999 · 2026-11-02', 13)),
    ];
    expect(budgets).toEqual([201, 172, 118, 367]);
    const sum = budgets.reduce((s, w) => s + w, 0);
    expect(sum).toBe(858);
    serve('Default Lock');
    const section = await renderPage();
    const table = within(section).getByRole('table', { name: 'Newest cells' });
    const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
    expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual(budgets);
    expect(cols.map((c) => c.width)).toEqual(['201px', 'auto', '118px', '367px']);
    expect(Number.parseFloat(table.style.minWidth)).toBe(sum);
    for (const [width, form] of [
      [1232, 'table'],
      [sum, 'table'],
      [sum - 1, 'rows'],
      [608, 'rows'],
      [358, 'rows'],
    ] as const) {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
      stubLayoutWidths({ chart: 900, list: width });
      serve('Default Lock');
      cleanup();
      const shown = await renderPage();
      expect(shown.querySelector('[data-list-form]')).toHaveAttribute('data-list-form', form);
    }
  });

  it('names DAO cells as deposits and requests, says "types", and links the DAO feed', async () => {
    serve('Nervos DAO');
    const section = await renderPage();
    expect(section.querySelector('[data-role="caption"]')!.textContent).toBe(
      'The newest live cells this script types, newest block first · it is not a list of transactions'
    );
    expect(api.getCellsByScriptRef).toHaveBeenCalledWith(
      expect.objectContaining({ scriptKind: 'type', order: 'desc', limit: 5 })
    );
    const rows = tableRows(section);
    expect(rows).toHaveLength(5);
    await waitFor(() =>
      expect(
        within(rows[0]).getByRole('link', { name: 'Nervos DAO withdrawal request' })
      ).toHaveAttribute('href', '/mainnet/dao')
    );
    await waitFor(() =>
      expect(within(rows[2]).getByRole('link', { name: 'Nervos DAO deposit' })).toBeInTheDocument()
    );
    // 17:55 UTC on 2026-11-02 is 01:55 on 2026-11-03 in UTC+8.
    expect(rows[0]).toHaveTextContent('#20,907,299 · 2026-11-03');
    const footer = section.querySelector('[data-role="newest-footer"]') as HTMLElement;
    expect(within(footer).getByRole('link', { name: /^all 22,291\scells$/ })).toHaveAttribute(
      'href',
      '#its-cells'
    );
    expect(within(footer).getByRole('link', { name: 'Activity: Nervos DAO' })).toHaveAttribute(
      'href',
      '/mainnet/activities?filter=dao'
    );
  });

  it('names Spore cells as the objects they are and links the objects feed', async () => {
    serve('Spore');
    const section = await renderPage();
    const rows = tableRows(section);
    expect(rows).toHaveLength(5);
    const [newest] =
      fixture.scripts.Spore.newest[
        '0x4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5:data1'
      ].data;
    const details = fixture.scripts.Spore.details as Record<string, { type?: { args: string } }>;
    const args = details[`${newest.txHash}:${newest.outputIndex}`].type!.args;
    await waitFor(() =>
      expect(
        within(rows[0]).getByRole('link', {
          name: `Spore object ${args.slice(0, 10)}…${args.slice(-6)}`,
        })
      ).toHaveAttribute('href', `/mainnet/objects/${args}`)
    );
    expect(
      within(rows[0]).getByRole('link', {
        name: `Spore object ${args.slice(0, 10)}…${args.slice(-6)}`,
      })
    ).toHaveAttribute('title', args);
    const footer = section.querySelector('[data-role="newest-footer"]') as HTMLElement;
    expect(within(footer).getByRole('link', { name: 'Activity: Objects' })).toHaveAttribute(
      'href',
      '/mainnet/activities?filter=object'
    );
  });

  it('says so in place when the API answers out of order (one that predates order=desc)', async () => {
    // :8101 before lane S ignores order=desc and answers oldest first.
    serve('Nervos DAO', (key) => {
      const page =
        fixture.scripts['Nervos DAO'].newest[
          key as '0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e:type'
        ];
      return { ...page, data: [...page.data].reverse() };
    });
    const section = await renderPage();
    await waitFor(() =>
      expect(within(section).getByRole('alert')).toHaveTextContent(
        /Newest cells unavailable: 0x82d76d1b.* type: cells\/by-script order=desc listed .* before .*: not newest block first/
      )
    );
    expect(within(section).queryByRole('table', { name: 'Newest cells' })).toBeNull();
  });

  it('keeps every column on a phone row', async () => {
    vi.unstubAllGlobals();
    stubLayoutWidths({ chart: 358, list: 358 });
    serve('Nervos DAO');
    const section = await renderPage();
    const rows = await within(section).findAllByTestId('newest-cell-row-mobile');
    expect(rows).toHaveLength(5);
    await waitFor(() =>
      expect(phoneLines(rows[0])).toEqual([
        {
          key: 'cell',
          tone: 'head',
          // U-09 (re-pinned): the phone row's out point and owner at the tight size
          left: '0xfa30…31b9:0',
          right: 'ckb1…exld6f',
        },
        { key: 'what', tone: 'body', left: 'Nervos DAO withdrawal request', right: null },
        {
          key: 'created',
          tone: 'quiet',
          left: 'tx 0xfa30…31b9 · #20,907,299 · 2026-11-03',
          right: null,
        },
      ])
    );
    // The coordinator's sweep (F3-e): a phone row's line is budgeted as a column is. The created
    // line's widest, "tx 0xfa30…31b9 · #99,999,999 · date unavailable" (47 characters at 12 px,
    // 338.4 px), is past a 328 px row, so it breaks between its facts, never inside one: no
    // nowrap run holds the whole line (each fact keeps its own).
    const at = (text: string, px: number) => (text.length * px * 3) / 5;
    expect(at('tx 0xfa30…31b9 · #99,999,999 · date unavailable', 12)).toBeGreaterThan(328);
    const created = rows[0].querySelector('[data-line="created"] > [data-part="left"]')!;
    expect(created.querySelector('[data-dotted]')!.closest('.whitespace-nowrap')).toBeNull();
  });
});
