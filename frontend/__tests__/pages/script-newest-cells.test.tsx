import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { phoneLines, stubLayoutWidths } from '@/__tests__/utils/phone-row';
import fixture from '@/__tests__/fixtures/scripts/newest-cells.json';
import ScriptDetailPage from '@/app/scripts/[name]/client-page';
import { api, type Cell } from '@/lib/api';
import { expectPrompt } from '@/__tests__/utils/prompt-heading';

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
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div /> }));
const route = vi.hoisted(() => ({ name: 'Default Lock' }));
vi.mock('@/src/navigation', () => ({
  useParams: () => ({ name: route.name }),
  usePathname: () => `/scripts/${route.name}`,
  useSearchParams: () => new URLSearchParams(),
}));

type ScriptName = keyof typeof fixture.scripts;
const EMPTY = { data: [], total: 0, limit: 12, hasMore: false, nextCursor: null };

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
    expect(caption).toHaveClass('text-small', 'text-text-dim');
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
      '0xfc7907…cf91:0',
      '0x55d533…136f:2',
      '0x55d533…136f:1',
      '0x55d533…136f:0',
      '0x3daee5…4722:2',
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
    const key = within(row).getByRole('link', { name: '0xfc7907…cf91:0' });
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
    // Owner, then "tx 0x… · #block · day".
    expect(
      within(row).getByRole('link', {
        name: `${first.address!.slice(0, 10)}…${first.address!.slice(-6)}`,
      })
    ).toHaveAttribute('href', `/mainnet/address/${first.address}`);
    // A shortened link keeps the full id in its title (lane L's rule).
    expect(
      within(row).getByRole('link', {
        name: `${first.address!.slice(0, 10)}…${first.address!.slice(-6)}`,
      })
    ).toHaveAttribute('title', first.address);
    expect(within(row).getByRole('link', { name: '0xfc7907…cf91' })).toHaveAttribute(
      'href',
      `/mainnet/tx/${first.txHash}`
    );
    expect(within(row).getByRole('link', { name: '#20,911,460' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/20911460'
    );
    await waitFor(() =>
      expect(row).toHaveTextContent('tx 0xfc7907…cf91 · #20,911,460 · 2026-11-03')
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
    const all = within(footer).getByRole('link', { name: 'All 887,562 cells ↓' });
    expect(all).toHaveAttribute('href', '#its-cells');
    expect(document.getElementById('its-cells')).toHaveAttribute('aria-label', 'Its cells');
    expect(within(footer).getAllByRole('link')).toHaveLength(1);
    expect(section.querySelector('a[href*="/activities"]')).toBeNull();
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
    expect(within(footer).getByRole('link', { name: 'All 22,291 cells ↓' })).toHaveAttribute(
      'href',
      '#its-cells'
    );
    expect(within(footer).getByRole('link', { name: 'Activity: Nervos DAO →' })).toHaveAttribute(
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
    expect(within(footer).getByRole('link', { name: 'Activity: Objects →' })).toHaveAttribute(
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
          left: '0xfa30a7…31b9:0',
          right: expect.stringMatching(/^ckb1qzda0c…/),
        },
        { key: 'what', tone: 'body', left: 'Nervos DAO withdrawal request', right: null },
        {
          key: 'created',
          tone: 'quiet',
          left: 'tx 0xfa30a7…31b9 · #20,907,299 · 2026-11-03',
          right: null,
        },
      ])
    );
  });
});
