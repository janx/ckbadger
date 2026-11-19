import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { phoneLines, stubLayoutWidths } from '@/__tests__/utils/phone-row';
import { stubViewportFromMd } from '@/__tests__/utils/viewport';
import fixture from '@/__tests__/fixtures/scripts/joyid-family-detail.json';
import { byScript, rowDetail } from '@/__tests__/utils/script-newest-cells';
import ScriptDetailPage from '@/app/scripts/[name]/client-page';
import { api, type Cell } from '@/lib/api';
import { expectPrompt, promptNote } from '@/__tests__/utils/prompt-heading';
import { DATA_COLOR, HELD_CAPACITY_COLOR } from '@/lib/design-tokens';
import { expectTier1Budget } from '@/__tests__/utils/hierarchy';
import { expectTicksInGutter, plotLeftOf, yTickLabels } from '@/__tests__/utils/y-ticks';
import { fullText } from '@/__tests__/utils/full-text';
import { factsOf } from '@/__tests__/utils/facts';
import { getCapacityRangeParams } from '@/lib/capacity-range';
import { linkLookClass } from '@/components/ui/link';
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
  isNotFoundApiError: () => false,
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

  it('opens with breadcrumb, role, the figure and one row of figures; the summary sentence in tier 2 (SC-S1, hierarchy v3)', async () => {
    await renderJoyid();
    // Consistency plan C1: the script family page names the tab.
    await waitFor(() => expect(document.title).toBe('JoyID · script · CKBadger'));
    expect(screen.getByRole('navigation', { name: 'Breadcrumb' })).toHaveTextContent(
      'Scripts / Locks'
    );
    expect(screen.getByText('lock script · upgradable')).toBeInTheDocument();
    // Lane F follow-up: upgradable code stays dim words; only immutable code carries the accent tag.
    expect(screen.getByText('lock script · upgradable').querySelector('[data-row-tag]')).toBeNull();
    const tier2 = document.querySelector('[data-tier="2"]') as HTMLElement;
    const summary = within(tier2).getByTestId('script-summary').textContent!;
    expect(summary).toContain(
      'JoyID decides who can spend 161,049 live cells holding 456,002,179.57'
    );
    expect(summary).toContain('2,707,778 cells have used it since 2023-10-07');
    // re-pinned (A4 round 2): a count is a unit of its own, its number and noun joined by a plain
    // space inside it (it was formatCount's no-break space in the text)
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
    // R9 (re-pinned): from sm the hero tile writes the holder at the row size
    expect(metrics).toHaveTextContent('multisig 0x65983f1d…eed696 · has never signed');
    expect(metrics).toHaveTextContent('In use since2023-10-07');
    // Lane L: the first deployment's transaction is named, and links to its page.
    const DEPLOY = '0xbac0f6f0a177a03974870df47ab0c3e761c5dd19e9ca165358d0b72bd7f433f5';
    // U-09 (re-pinned): a hero tile's note at the tight size (it was 8…6); R9 (re-pinned): the
    // tile is under 200 px only on a phone, and from sm the note takes the row size
    expect(metrics).toHaveTextContent('In use since2023-10-07 · deployed in 0xbac0f6f0…f433f5');
    const deployer = within(metrics).getByRole('link', { name: '0xbac0f6f0…f433f5' });
    expect(deployer).toHaveAttribute('href', `/mainnet/tx/${DEPLOY}`);
    expect(deployer).toHaveAttribute('title', DEPLOY);
  });

  // U-16, PA-05 (UI review 2026-11-14): the figures row broke its units on a phone and in a 300 px
  // column ("deployed in 0xe2fb19…" / "73ff5c", "2019-" / "11-16", "78,647,937" / "cells") and
  // left a "·" at a line's end. Each unit is whole; a note's dot goes with the note.
  it('keeps every unit of the figures row whole, a note’s dot with its note (U-16, PA-05)', async () => {
    await renderJoyid();
    const hero = screen.getByRole('region', { name: 'JoyID' });
    const metrics = within(hero.parentElement!).getByLabelText('Script figures');
    const whole = (el: Element) =>
      expect(el.closest('[data-nobreak], .whitespace-nowrap')).not.toBeNull();
    // the deployment's shortened hash, the first day, the replacement day, the holder's name
    whole(
      within(metrics).getByRole('link', { name: '0xbac0f6f0…f433f5' }).firstElementChild ??
        within(metrics).getByRole('link', { name: '0xbac0f6f0…f433f5' })
    );
    whole(within(metrics).getByText('2023-10-07'));
    whole(within(metrics).getByText('2025-11-21'));
    // re-pinned (A4 Chrome F2): the holder's id is the unit, its kind a word before it
    // ("multisig 0x6598…d696" whole is 204 px at 17 px, wider than the tile's 163 px at 390);
    // R9 (re-pinned): from sm the row size, 0x65983f1d…eed696
    whole(within(metrics).getByText('0x65983f1d…eed696'));
    // the count and its noun, one unit; "Upgradable" and its versions, two items of one list
    // re-pinned (A4 round 2): one unit each, number and noun joined by a plain space inside it,
    // so a tile narrower than the count breaks between them, never inside a word
    expect(metrics.textContent).toContain('2,707,778 cells');
    expect(metrics.textContent).toContain('5 versions');
    // every dot belongs to the item after it (the one separated list): none is loose text
    const loose = [...metrics.querySelectorAll('*')].flatMap((el) =>
      [...el.childNodes].filter(
        (n) =>
          n.nodeType === Node.TEXT_NODE &&
          /·/.test(n.textContent ?? '') &&
          !(el as Element).hasAttribute('data-dotted-separator')
      )
    );
    expect(loose.map((n) => n.textContent)).toEqual([]);
  });

  it('draws History with occupied in jade, a separate occupied panel and the version days in the plot (SC-S2, SC-S3, L1-3 ruling)', async () => {
    const { container } = { container: document.body };
    await renderJoyid();
    const fills = [...container.querySelectorAll('svg path[fill]')].map((p) =>
      p.getAttribute('fill')
    );
    expect(fills).toContain(DATA_COLOR);
    expect(fills).toContain(HELD_CAPACITY_COLOR);
    expect(fills).not.toContain('#58a5b5');
    // Hierarchy v3: each panel's name is written inside its plot (R13). Re-pinned with the lane A5
    // follow-up (U-19): the ticks are bare, so the occupied panel's name says its unit, CKB.
    expect(
      [...document.querySelectorAll('[data-role="chart-name"]')].map((t) => t.textContent)
      // Re-pinned with the plot-name fit (lane A5, Chrome at 360): the token and collection plots'
      // fixed name, as a script's name has no length bound; the h1 says which script.
    ).toEqual(['CKB held · occupied, free', "CKB occupied by those cells' own bytes"]);
    const reading = screen.getByTestId('script-history-reading');
    expect(reading).toHaveTextContent('2026-10-29');
    // re-pinned with readability R4 (lane B7): the reading is labelled facts, the detail pages' dl rows (was one line of facts joined by "·")
    expect(factsOf(reading).find(([label]) => label === 'CKB held')![1]).toMatch(
      /^456,002,179\.57/
    );
    expect(factsOf(reading)).toContainEqual(['code', 'v5 held by multisig 0x6598…d696']);
    // Table-rows L1-3 ruling (board ScriptDetail): no code track under the chart. The version days
    // are ghost labels inside the plot; the holders are the timeline's segments.
    expect(screen.queryByLabelText('Code track')).toBeNull();
    const history = screen.getByTestId('script-history');
    expect(
      [...history.querySelectorAll<HTMLElement>('[data-upgrade-label]')].map((l) => l.textContent)
    ).toEqual(['v3 · v4', 'v5']);
    const timeline = screen.getByTestId('holder-timeline');
    expect(timeline).toHaveTextContent('multisig 0xd1db…b95c · 3 of 5 signatures');
    expect(
      [...timeline.querySelectorAll<HTMLElement>('[data-holder]')].map((s) => s.title).join(' ')
    ).toContain('multisig 0x65983f1d…eed696 · signers not on chain yet');
    expect(document.body.textContent).not.toMatch(/common knowledge/i);
  });

  // Table-rows L1-3 (T8): the three columns of sentences became one holder timeline; the keys, what
  // each signed and the keys kept across a handover are each segment's title.
  it('shows who can change the code as three multisigs on one timeline, keys in the titles (SC-S4, L1-3)', async () => {
    await renderJoyid();
    const section = screen.getByRole('region', { name: 'Who can change its code' });
    // Phase 5 (A17): the question runs on the holders the timeline draws.
    expectPrompt(
      within(section).getByRole('heading', { level: 2 }),
      'Who can change its code',
      '3 owners'
    );
    expect(within(section).queryByRole('list', { name: 'Code cell holders' })).toBeNull();
    const bar = within(section).getByRole('group', { name: /^Who held JoyID's code cell/ });
    const segments = [...bar.querySelectorAll<HTMLElement>('[data-holder]')];
    expect(segments).toHaveLength(3);
    expect(segments[0].title).toContain('2023-10-07 → 2025-08-28');
    expect(segments[0].title).toContain('3 of 5 signatures');
    expect(segments[0].title).toContain('signed v2 and v3');
    expect(segments[1].title).toContain('signed v4 and v5');
    expect(segments[2].title).toContain('has never signed');
    expect(segments[2].title).toContain(
      'its keys and threshold appear on chain the first time it signs'
    );
    expect(segments[0].title.match(/0x[0-9a-f]{40}/g)).toHaveLength(6);
    expect(segments[1].title).toContain(
      'kept 2 of 5\u00a0keys of multisig 0xd1dbde92…0cb95c at the handover on 2025-08-28'
    );
    expect(screen.getByRole('status', { name: 'Code cell holder reading' })).toHaveTextContent(
      '3 holders · 5 versions · 2 handovers'
    );
  });

  // Table-rows L1-4: one line per version, newest first (board ScriptDetail); the exact time, the
  // binary, the code cell's CKB, the signer and the fate are each row's title.
  it('merges the versions into one full-width table, newest first (SC-S5, L1-4)', async () => {
    await renderJoyid();
    expect(screen.queryByRole('table', { name: 'Code cell deployments' })).toBeNull();
    const table = screen.getByRole('table', { name: 'Code versions' });
    const rows = within(table).getAllByRole('row').slice(1);
    expect(rows.map((row) => row.textContent!.match(/^v\d/)![0])).toEqual([
      'v5',
      'v4',
      'v3',
      'v2',
      'v1',
    ]);
    const title = (row: HTMLElement) => row.getAttribute('title')!;
    await waitFor(() => expect(title(rows[4])).toContain('96,760 B'));
    expect(title(rows[4])).toContain('2023-10-07 15:39');
    expect(rows[4]).toHaveTextContent('#11,132,140');
    expect(title(rows[4])).toContain('consumed by v2');
    expect(title(rows[4])).toContain('first deployed');
    expect(title(rows[4])).toContain('into multisig 0xd1dbde92…0cb95c');
    expect(title(rows[2])).toContain('hands it to 0x07a21dc2…9d68ca');
    expect(rows[0]).toHaveTextContent('live · current');
    expect(title(rows[0])).toContain('100,438.00000000');
    // re-pinned (A4 round 2): the id and the count are units of their own in the sentence
    expect(
      screen.getByText(fullText(/one type id, 0xd00c84f0…318323, 5 binaries/))
    ).toBeInTheDocument();
  });

  // Table-rows L1-5: the recipe is "The script" facts (board ScriptDetail's tier 2).
  it('says how to use it in a transaction, in the script facts (SC-S6, L1-5)', async () => {
    await renderJoyid();
    const section = screen.getByRole('heading', { name: 'The script' }).closest('section')!;
    expect(section).toHaveTextContent(
      '0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323'
    );
    expect(section).toHaveTextContent('type · follows every upgrade');
    expect(section).toHaveTextContent(
      '0x673e1843035ce9c249efc36b62865e7a7c9f6e2ca6fc9c66fd108307e81355f9:0'
    );
    expect(section).toHaveTextContent('pin instead');
    expect(section).toHaveTextContent('pins this exact binary');
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
    expect(rows[0]).toHaveTextContent('0x44e5e96c…f8517f:1');
    expect(rows[0]).toHaveTextContent('63 B');
    expect(rows[0]).toHaveTextContent('—');
    expect(rows[1]).toHaveTextContent('COTA');
    expect(rows[1]).not.toHaveTextContent('0x1122a4fb');
    expect(rows[0].querySelector('[data-part="free"]')).not.toBeNull();
    expect(rows[0].innerHTML).not.toMatch(/var(--c-gold)|text-gold|text-dao/);
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

  // Lane c2-chain, written before the cells' pager became the library's Pager: what it does,
  // whatever draws it — the next page asks for the family's cells after the last one shown, five
  // a page, and the previous one comes back to the first. Re-pinned (lane c2-finish, 1c): the page
  // was counted of the family's live cells (161,049 at five a page: "page 1 of 32,210"), a figure
  // of another read at another tip; the merged page counts no rows, so it is "page 1", "page 2".
  it('pages its cells forward from a cursor and back to the first page', async () => {
    // re-pinned (lane cd-fixes-controls, item 1): two pages that agree with themselves
    // (pagedRead), the fixture's three cells with more after them, then three newer and last
    // (it served the same page at every cursor, so page 2's next cursor repeated the one it was
    // asked with: an API fault the pager now refuses)
    const first = fixture.cells.data;
    const later = first.map((cell) => ({
      ...cell,
      outputIndex: cell.outputIndex + 100,
      createdAtBlock: cell.createdAtBlock + 1_000_000,
    }));
    vi.mocked(api.getCellsByScriptRef).mockImplementation(
      byScript((p) =>
        p.cursor
          ? (pagedRead(
              {
                ...fixture.cells,
                data: later,
                limit: 3,
                total: 6,
                hasMore: false,
                nextCursor: null,
              },
              3
            ) as never)
          : (pagedRead({ ...fixture.cells, limit: 3, total: 6 }) as never)
      )
    );
    await renderJoyid();
    const cells = document.getElementById('its-cells')!;
    expect(within(cells).getByText(/^page 1$/i)).toBeInTheDocument();
    fireEvent.click(within(cells).getByRole('button', { name: /^next( →)?$/i }));
    await waitFor(() =>
      expect(api.getCellsByScriptRef).toHaveBeenLastCalledWith(
        expect.objectContaining({ scriptKind: 'lock', limit: 5, cursor: expect.any(String) })
      )
    );
    expect(await within(cells).findByText(/^page 2$/i)).toBeInTheDocument();
    fireEvent.click(within(cells).getByRole('button', { name: /^(previous|← prev)$/i }));
    expect(await within(cells).findByText(/^page 1$/i)).toBeInTheDocument();
  });

  // Table-rows L1-5: the registered references are "The script" facts, one list per reference.
  it('keeps registered references in the developer tier (SC-S9, L1-5)', async () => {
    await renderJoyid();
    const tier1 = document.querySelector('[data-tier="1"]') as HTMLElement;
    const tier2 = document.querySelector('[data-tier="2"]') as HTMLElement;
    expect(within(tier1).queryByRole('heading', { name: 'The script' })).toBeNull();
    const facts = within(tier2).getByRole('heading', { name: 'The script' }).closest('section')!;
    // Lane E (C6): one row per reference (was one definition list each), the lists folded.
    expect(
      within(within(facts).getByRole('table', { name: 'Registered references' })).getAllByRole(
        'row'
      )
    ).toHaveLength(fixture.family.references.length + 1);
    expect(
      within(facts).getByText('Each reference in full').closest('details')!.querySelectorAll('dl')
    ).toHaveLength(fixture.family.references.length);
    expect(facts).toHaveTextContent('Lock args 0x65983f1d7520543a6f182a50354fb4ae2deed696');
  });
});

describe('JoyID script page · visual pass', () => {
  beforeEach(() => {
    // Charts at 900 px; the lists at the page's full content width (1248 at 1280 and up).
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

  // U-19 (UI review 2026-11-14): the history's round values sat inside the plots in the ghost
  // ink, crossed by a spike ("200M CKB"); both plots write them in one dim gutter, and the upgrade
  // lines and labels sit on the plots' x, after it.
  // Lane A5 follow-up: a tick is the bare value ("200M"), the unit said once in each plot's name,
  // so the shared gutter is at most 56 px.
  it('writes bare values in a gutter of at most 56 px, each plot naming CKB once (U-19)', async () => {
    await renderJoyid();
    const history = screen.getByTestId('script-history');
    const plots = [...history.querySelectorAll('svg[role="img"]')];
    for (const plot of plots) {
      for (const t of yTickLabels(plot)) expect(t.textContent).toMatch(/^[\d.,]+[kMB]?$/);
      expect(plot.querySelector('[data-role="chart-name"]')?.textContent).toContain('CKB');
      expect(plotLeftOf(plot)).toBeLessThanOrEqual(56);
    }
  });

  it('writes the history values in one dim gutter left of both plots (U-19)', async () => {
    await renderJoyid();
    const history = screen.getByTestId('script-history');
    const [main, occupied] = [...history.querySelectorAll('svg[role="img"]')];
    expectTicksInGutter(main, 'CKB in JoyID cells');
    expectTicksInGutter(occupied, 'occupied', 1);
    const left = plotLeftOf(main);
    expect(plotLeftOf(occupied)).toBe(left);
    expect(left).toBeGreaterThan(0);
    const lines = history.querySelector('[data-upgrade-line]')!.parentElement as HTMLElement;
    expect(lines.style.left).toBe(`${left}px`);
  });

  it('draws the upgrade days as dashed lines through every panel, not as a link list', async () => {
    await renderJoyid();
    const history = screen.getByTestId('script-history');
    expect(history.querySelectorAll('[data-upgrade-line]')).toHaveLength(3);
    expect(within(history).queryByRole('link', { name: /v1 · v2 · 2023-10-07/ })).toBeNull();
  });

  // Table-rows L1-3 ruling: the code track's staggered rows are gone with the track. The plot's
  // version labels share the name's line, one per deployment day, the newest first: a label that
  // would touch the plot's name or a newer label is left out (its dashed line stays).
  it('labels the upgrade days in the plot, one per day, never over the plot name or each other', async () => {
    await renderJoyid();
    const history = screen.getByTestId('script-history');
    const labels = [...history.querySelectorAll<HTMLElement>('[data-upgrade-label]')];
    expect(labels.map((l) => l.dataset.upgradeLabel)).toEqual(['2025-08-28', '2025-11-21']);
    expect(labels.map((l) => l.style.left)).toEqual(['60%', '80%']);
    for (const l of labels) {
      // re-pinned with lane E2's follow-up (ruling R4 covers the quiet ink): a day label is a
      // figure, so the dim ink, no longer the quiet slate
      expect(l).toHaveClass('text-meta', 'text-text-dim');
      expect(l.style.getPropertyValue('--ink')).toBe('');
    }
    // The first day sits under the plot's name at the left edge: its line, not its label.
    expect(history.querySelector('[data-upgrade-label="2023-10-07"]')).toBeNull();
    expect(history.querySelectorAll('[data-custodian]')).toHaveLength(0);
  });

  // Re-pinned with the plot-name fit (lane A5, Chrome at 360): at 358 px the plot is 300 px wide
  // after its 48 px gutter and its fixed name takes 8 + 25 × 7 = 183 of them, so a version label
  // can fit beside it; whatever is written never touches the name, and the three lines stay.
  it('writes no version label over the plot name on a phone (358 px); the lines stay', async () => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    stubLayoutWidths({ chart: 358, list: 358 });
    vi.mocked(api.getScriptFamilyDetail).mockResolvedValue(fixture.family as never);
    vi.mocked(api.getScriptUsage).mockResolvedValue(fixture.usage);
    vi.mocked(api.getScriptCapacityChart).mockResolvedValue(fixture.chart);
    // Lane S: the newest cells read the same fixture rows newest first, and each row's detail.
    vi.mocked(api.getCell).mockImplementation(
      async (tx, index) => rowDetail(fixture.cells.data as never, tx, index) ?? codeCell(tx, index)
    );
    vi.mocked(api.getCellsByScriptRef).mockImplementation(byScript(() => fixture.cells as never));
    vi.mocked(api.lookupScripts).mockResolvedValue({});
    render(<ScriptDetailPage name="JoyID" />);
    await screen.findByRole('heading', { name: 'JoyID', level: 1 });
    const history = await screen.findByTestId('script-history');
    const plot = 358 - 10 - 48;
    const nameEnd = 8 + 'CKB held · occupied, free'.length * 7;
    for (const label of history.querySelectorAll<HTMLElement>('[data-upgrade-label]')) {
      const w = 4 + label.textContent!.length * 7;
      const start = label.style.left
        ? (Number.parseFloat(label.style.left) / 100) * plot
        : plot - (Number.parseFloat(label.style.right) / 100) * plot - w;
      expect(start, label.textContent!).toBeGreaterThanOrEqual(nameEnd + 8);
    }
    expect(history.querySelectorAll('[data-upgrade-line]')).toHaveLength(3);
    expect(screen.getAllByTestId('holder-timeline')[0]).toHaveTextContent('v5 2025-11-21');
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
  const hero = () => screen.getByRole('region', { name: 'JoyID' });
  const tier1 = () => document.querySelector('[data-tier="1"]') as HTMLElement;
  const tier2 = () => document.querySelector('[data-tier="2"]') as HTMLElement;
  const follows = (a: Element, b: Element) =>
    Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);

  // Re-pinned on purpose (readability R7, lane p-assets; renamed, it was "opens with the title row
  // — h1, its role and the range buttons — …"): the range went with the history to the History
  // section's heading row; the title row is the h1 and its role.
  it('opens with the title row — h1 and its role — and the description as the lede; the range heads the History', async () => {
    await renderJoyid();
    const nav = screen.getByRole('navigation', { name: 'Breadcrumb' });
    expect(tier1()).toContainElement(nav);
    expect(follows(nav, hero())).toBe(true);
    const h1 = within(hero()).getByRole('heading', { level: 1, name: 'JoyID' });
    // PB-06 / PA-32: an entity h1, one type step smaller below md (30 → 26 px).
    expect(h1).toHaveClass('text-entity', 'max-md:text-index', 'text-text-bright');
    const row = h1.parentElement!.parentElement!;
    expect(row).toHaveClass('flex', 'items-baseline', 'justify-between');
    expect(within(row).getByText('lock script · upgradable')).toBeInTheDocument();
    expect(within(row).queryByRole('radiogroup', { name: 'range' })).toBeNull();
    // Re-pinned on purpose (lane c2-chain, the controls board): the range is the library's
    // RangeSelect, a radio group whose spans are written `30 d · 90 d · 1 y · all` (it was a group
    // of pressed buttons, "30D 90D 1Y ALL"); the chosen span is the board's one selected state, the
    // celadon line around the bright ink, so still no jade text in tier 1 (R11; D3 call 8).
    const historyHead = within(screen.getByRole('region', { name: 'History' })).getByRole(
      'heading',
      { name: 'History' }
    ).parentElement!;
    const range = within(historyHead).getByRole('radiogroup', { name: 'range' });
    expect(
      within(range)
        .getAllByRole('radio')
        .map((r) => r.textContent)
    ).toEqual(['30 d', '90 d', '1 y', 'all']);
    const all = within(range).getByRole('radio', { name: 'all' });
    expect(all).toHaveAttribute('aria-checked', 'true');
    expect(all).toHaveClass('text-text-bright', 'border-jade');
    expect(all.className).not.toMatch(/text-jade/);
    const lede = hero().querySelector('[data-role="lede"]') as HTMLElement;
    expect(lede).toHaveTextContent(
      /^An universal Account Protocol for Web3 Mass-adoption · joy\.id ↗$/
    );
    // re-pinned with readability R4 (lane B7): on the measure, 76 characters (was max-w-[560px])
    // re-pinned with readability R3 / R4 (lane B7): a lede is a sentence that explains, the
    // reading ink (was text-text-dim)
    expect(lede).toHaveClass('text-body', 'text-text', 'max-w-measure');
    expect(within(lede).getByRole('link', { name: 'joy.id ↗' })).toHaveAttribute(
      'href',
      'https://joy.id'
    );
  });

  // PA-18 (UI review 2026-11-14): on a phone "30D 90D 1Y ALL" sat between the kind line and the
  // description, about 250 px above the chart it controls. Re-pinned on purpose (readability R7,
  // lane p-assets; renamed, it was "puts the range buttons in the chart’s caption row below md, not
  // in the title row"): the one range group heads the History section at every width, right over
  // the sentence and the picture it redraws, after the hero's figures.
  it('puts the range buttons in the History heading row at every width, not in the title row (PA-18, R7)', async () => {
    for (const fromMd of [false, true]) {
      stubViewportFromMd(fromMd);
      await renderJoyid();
      // re-pinned (lane c2-chain): the range is the library's radio group
      const groups = screen.getAllByRole('radiogroup', { name: 'range' });
      expect(groups).toHaveLength(1);
      const [range] = groups;
      expect(hero()).not.toContainElement(range);
      const section = screen.getByRole('region', { name: 'History' });
      const head = within(section).getByRole('heading', { name: 'History' }).parentElement!;
      expect(head).toContainElement(range);
      expect(follows(head, within(section).getByTestId('history-sentence'))).toBe(true);
      expect(follows(screen.getByLabelText('Script figures'), section)).toBe(true);
      cleanup();
    }
    stubViewportFromMd(false);
    await renderJoyid();
    const range = screen.getByRole('radiogroup', { name: 'range' });
    // still the page's range: it draws the picture again
    fireEvent.click(within(range).getByRole('radio', { name: '90 d' }));
    expect(within(range).getByRole('radio', { name: '90 d' })).toHaveAttribute(
      'aria-checked',
      'true'
    );
  });

  it('leads with CKB in its cells as the one figure, bright, its live cells on the baseline row (R4, R11, R13)', async () => {
    await renderJoyid();
    const figure = hero().querySelector('[data-role="figure"]') as HTMLElement;
    expect(figure).toHaveTextContent(/^CKB in its cells/);
    const amount = figure.querySelector('[data-role="figure-amount"]') as HTMLElement;
    // the display figure in the summary form, exact in its title (readability R5, re-pinned on
    // purpose: was "456,002,179.57")
    expect(amount).toHaveTextContent(/^456,002,179 CKB$/);
    expect(amount.querySelector('[data-ckb-amount]')).toHaveAttribute(
      'title',
      '456,002,179.57 CKB'
    );
    expect(amount).toHaveClass('text-index', 'sm:text-display', 'text-text-bright');
    const row = amount.parentElement!;
    expect(row).toHaveClass('flex', 'items-baseline', 'justify-between');
    expect(row.lastElementChild).toHaveTextContent(/^in 161,049 live cells · 4\.19% occupied$/);
    expect(within(row.lastElementChild as HTMLElement).getByRole('link')).toHaveAttribute(
      'href',
      '#its-cells'
    );
  });

  // Re-pinned on purpose (readability R7, lane p-assets; renamed, it was "draws its capacity
  // history right under the figure: ≥ 280 px, named inside, no History heading (R13, tier 1
  // always)"): the history stays in tier 1, after the hero's figures, in a History section of its
  // own, 160 px under one sentence its points give.
  it('draws its capacity history after the figures: 160 px under its sentence, named inside (R7, tier 1 always)', async () => {
    await renderJoyid();
    const section = screen.getByRole('region', { name: 'History' });
    expect(tier1()).toContainElement(section);
    expect(tier1().querySelector('figcaption')).toBeNull();
    const history = within(section).getByTestId('script-history');
    expect(within(hero()).queryByTestId('script-history')).toBeNull();
    expect(follows(screen.getByLabelText('Script figures'), history)).toBe(true);
    expect(within(section).getByTestId('history-sentence')).toHaveTextContent(/^CKB held: /);
    const [main, occupied] = [...history.querySelectorAll('svg[role="img"]')];
    expect(Number(main.getAttribute('height'))).toBe(160);
    // Re-pinned with the plot-name fit: the fixed name of every CKB-held history plot.
    expect(main.querySelector('[data-role="chart-name"]')).toHaveTextContent(
      'CKB held · occupied, free'
    );
    expect(Number(occupied.getAttribute('height'))).toBeLessThan(
      Number(main.getAttribute('height'))
    );
    // Re-pinned with U-19 (UI review 2026-11-14): the values sit in a dim gutter left of the plot,
    // anchored at their end, no longer inside it from its left edge.
    expect(main.querySelectorAll('[data-role="y-tick"]')).toHaveLength(0);
    const ticks = yTickLabels(main);
    expect(ticks.length).toBeGreaterThan(0);
    for (const t of ticks) expect(t.getAttribute('text-anchor')).toBe('end');
  });

  // Table-rows L1-3 ruling: no code track between the picture and its reading any more.
  // Re-pinned on purpose (readability R7, lane p-assets; renamed, it was "reads the last full day
  // in one row under the picture, then the remaining figures in one quiet row (R13)"): the figures
  // come first, then the history and its reading.
  it('reads the figures in one quiet row after the hero, then the last full day in one row under the picture (R7)', async () => {
    await renderJoyid();
    expect(screen.queryByLabelText('Code track')).toBeNull();
    const reading = screen.getByTestId('script-history-reading');
    const metrics = screen.getByLabelText('Script figures');
    expect(follows(hero(), metrics)).toBe(true);
    expect(follows(metrics, reading)).toBe(true);
    expect(follows(screen.getByTestId('script-history'), reading)).toBe(true);
    expect(reading).toHaveClass('text-small', 'border-t');
    expect(reading).toHaveTextContent('2026-10-29');
    // re-pinned with readability R4 (lane B7): the reading is labelled facts, the detail pages' dl rows (was one line of facts joined by "·")
    expect(factsOf(reading)).toContainEqual(['code', 'v5 held by multisig 0x6598…d696']);
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
      // PA-25 (re-pinned): the value is the first item of the metric's separated list
      expect(t.nextElementSibling!.querySelector('.text-section')).toHaveClass('text-text-bright');
      expect(t.parentElement!.children).toHaveLength(2);
    }
    expect(metrics).toHaveClass('grid-cols-2', 'lg:grid-cols-4');
  });

  it('reads the hovered day across the picture, and lets it go when the range changes', async () => {
    await renderJoyid();
    const reading = screen.getByTestId('script-history-reading');
    // re-pinned with readability R4 (lane B7): the reading is labelled facts, the detail pages' dl rows (was one line of facts joined by "·")
    expect(factsOf(reading)[0]).toEqual(['day', '2026-10-29 · last full day']);
    // re-pinned (readability R7): the history and its range are the History section's
    const section = screen.getByRole('region', { name: 'History' });
    const [main] = [
      ...within(section).getByTestId('script-history').querySelectorAll('svg[role="img"]'),
    ];
    fireEvent.keyDown(main, { key: 'ArrowLeft' });
    expect(reading).not.toHaveTextContent('last full day');
    expect(reading).not.toHaveTextContent('2026-10-29');
    fireEvent.click(within(section).getByRole('radio', { name: '90 d' }));
    // The picture and its reading are drawn again for the new range.
    await waitFor(() =>
      expect(factsOf(screen.getByTestId('script-history-reading'))[0]).toEqual([
        'day',
        '2026-10-29 · last full day',
      ])
    );
  });

  // Lane c2-chain, written before the range became the library's RangeSelect: what the range does,
  // whatever draws it — the page opens on every day, and a span asks the API for its days.
  it('opens on every day and asks the API for the chosen span’s days', async () => {
    await renderJoyid();
    expect(api.getScriptCapacityChart).toHaveBeenLastCalledWith('JoyID', undefined);
    for (const [span, key] of [
      [/^90 ?d$/i, '90d'],
      [/^30 ?d$/i, '30d'],
    ] as const) {
      const days = getCapacityRangeParams(key);
      expect(days).toBeDefined();
      // re-pinned (readability R7): the range is the History section's
      fireEvent.click(within(screen.getByRole('region', { name: 'History' })).getByText(span));
      await waitFor(() =>
        expect(api.getScriptCapacityChart).toHaveBeenLastCalledWith('JoyID', days)
      );
    }
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

  it('says who can change its code in one caption under the holder timeline; the mechanism in tier 2 (C2, L1-3)', async () => {
    await renderJoyid();
    const section = screen.getByRole('region', { name: 'Who can change its code' });
    expect(within(section).getByRole('heading', { level: 2 })).toHaveClass('text-section');
    const timeline = within(section).getByTestId('holder-timeline');
    const caption = section.querySelector('[data-role="caption"]') as HTMLElement;
    expect(caption).toHaveTextContent(
      /^Whoever can spend the code cell can put new code in it; every JoyID cell runs the new code from the next block\.$/
    );
    expect(caption.textContent!.length).toBeLessThanOrEqual(160);
    // re-pinned with readability R4 (lane B7): on the measure, 76 characters (was max-w-[640px])
    expect(caption).toHaveClass('text-body', 'max-w-measure');
    expect(timeline.lastElementChild).toBe(caption);
    expect(tier1()).not.toHaveTextContent('point at their code through one code cell');
    expect(tier2()).toHaveTextContent(
      "JoyID cells point at their code through one code cell's type id, not at a fixed binary. Whoever can spend that cell can put new code in it, and every JoyID cell runs the new code from the next block."
    );
  });

  it('keeps the code versions table in tier 1 and its heading note and the transaction recipe in tier 2 (C3, R14)', async () => {
    await renderJoyid();
    const versions = screen.getByRole('table', { name: 'Code versions' });
    expect(tier1()).toContainElement(versions);
    const h2 = within(tier1()).getByRole('heading', { name: 'Versions 5', level: 2 });
    expect(h2).toHaveClass('text-section');
    expect(promptNote(h2)).toHaveTextContent(/^newest first$/);
    // The heading row holds the h2 and the board's quiet order note only (L1-4).
    expect(h2.parentElement!.children).toHaveLength(2);
    expect(h2.nextElementSibling).toHaveTextContent(/^newest first$/);
    expect(
      within(tier2()).getByText(fullText(/one type id, 0xd00c84f0…318323, 5 binaries/))
    ).toBeInTheDocument();
    // Table-rows L1-5: the recipe is "The script" facts.
    expect(within(tier2()).getByRole('heading', { name: 'The script' })).toBeInTheDocument();
    expect(within(tier1()).queryByRole('heading', { name: 'The script' })).toBeNull();
    // The cells' summary line, with its exact totals, is tier 2's too (R14).
    expect(within(tier2()).getByTestId('script-cells-summary')).toBeInTheDocument();
    // Tier 2: the board's two blocks side by side from lg, nothing else.
    const read = within(tier2()).getByRole('heading', { name: 'How to read this page' });
    expect(read.closest('section')!.parentElement).toHaveClass('grid', 'lg:grid-cols-2');
    expect(read.closest('section')!.parentElement!.children).toHaveLength(2);
  });

  it('spaces tier 1 like the DAO page: 64 px under the hero, then 72 px, h2 18 px above its evidence (R6)', async () => {
    await renderJoyid();
    // Lane S: "Newest cells" is the first section under the hero now; custody follows at 72 px.
    const newest = screen.getByRole('region', { name: 'Newest cells' });
    expect(newest).toHaveClass('mt-16');
    expect(within(newest).getByRole('heading', { level: 2 }).parentElement).toHaveClass(
      'mb-[18px]',
      'flex'
    );
    const custody = screen.getByRole('region', { name: 'Who can change its code' });
    expect(custody).toHaveClass('mt-[72px]');
    // Phase 5: the versions and the cells run on their counts (A18, A19).
    for (const name of ['Versions 5', 'Cells 161,049 live']) {
      const h2 = within(tier1()).getByRole('heading', { name, level: 2 });
      expect(h2.closest('section')).toHaveClass('mt-[72px]');
      // The heading row is a flex row, as the DAO page's: the h2 is as wide as its words; the
      // row (or, for a heading with a note, the note's row) sits 18 px above its evidence.
      expect(h2.parentElement).toHaveClass('flex', 'items-baseline');
      expect(h2.closest('section')!.firstElementChild).toHaveClass('mb-[18px]');
      expect(h2.closest('section')!.firstElementChild).toContainElement(h2);
    }
    expect(within(custody).getByRole('heading', { level: 2 }).parentElement).toHaveClass(
      'mb-[18px]',
      'flex'
    );
  });

  // Table-rows L1-4: the board's one accent, the "live · current" class-mark tag of the version the
  // cells run now, is the only coloured text tier 1 keeps (RowTag accent, as the boards draw it).
  it('writes no coloured text in tier 1 but the live · current tag: the figure is bright, the graphics keep their colours (R11)', async () => {
    await renderJoyid();
    await waitFor(() =>
      expect(
        within(screen.getByRole('table', { name: 'Code versions' }))
          .getAllByRole('row')[5]
          .getAttribute('title')
      ).toContain('96,760 B')
    );
    const accents = [...tier1().querySelectorAll<HTMLElement>('[data-row-tag="accent"]')];
    expect(accents.map((el) => el.textContent)).toEqual(['live · current']);
    // re-pinned on purpose (lane c2-chain): the cells' pager is the library's, whose way to go is
    // celadon on every page (the controls board; ui-color-guidelines' pager row), a control's
    // part and not coloured text; it was a boxed button in the reading ink
    const pager = screen.getByRole('navigation', { name: 'cells' });
    expect(within(pager).getByRole('button', { name: 'next →' })).toHaveClass('text-jade');
    // re-pinned on purpose (lane p-assets): a way onward is the link owner's onward look, celadon
    // by the link rule (components/ui/link.tsx) as the pager's way to go is, not coloured text: the
    // newest cells' "all N cells ↓" to its cells list (it was a raw anchor in the reading ink)
    const onward = linkLookClass('onward')!;
    const coloured = [...tier1().querySelectorAll('[class]')].filter(
      (el) =>
        !el.matches('[data-row-tag="accent"]') &&
        !pager.contains(el) &&
        !(el.matches('a') && el.classList.contains(onward)) &&
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
    // Lane S: the newest cells read the same fixture rows newest first, and each row's detail.
    vi.mocked(api.getCell).mockImplementation(
      async (tx, index) => rowDetail(fixture.cells.data as never, tx, index) ?? codeCell(tx, index)
    );
    vi.mocked(api.getCellsByScriptRef).mockImplementation(byScript(() => fixture.cells as never));
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
    // Newest first since table-rows L1-4: v1 is the last row.
    await waitFor(() => expect(rows[4]).toHaveTextContent('96,760 B'));
    const first = phoneLines(rows[4]);
    expect(first.map((l) => [l.key, l.tone])).toEqual([
      ['version', 'body'],
      ['hash', 'quiet'],
      ['detail', 'quiet'],
    ]);
    // U-09 (re-pinned): the phone row's out point at the tight size
    expect(first[0].left).toMatch(/^v1 2023-10-07 · 0x[0-9a-f]{4}…[0-9a-f]{4}:\d$/);
    expect(first[0].right).toBe('consumed by v2');
    // The version hash keeps a line of its own, whole: it is what a developer copies.
    expect(first[1].left).toMatch(/^0x[0-9a-f]{64}$/);
    expect(first[2].left).toMatch(
      /^#11,132,140 · 96,760 B · .+ CKB · first deployed into multisig 0xd1db…b95c$/
    );
    expect(first[2].right).toMatch(/^live cells using it: /);
    const last = phoneLines(rows[0]);
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
    // Newest first since table-rows L1-4: v1 is the last row.
    const first = within(table).getAllByRole('row').slice(1).at(-1)!;
    const cell = first.querySelectorAll('td')[2];
    await waitFor(() => expect(cell).toHaveTextContent('96,760 B · first deployed into multisig'));
    expect(cell.lastElementChild!.textContent).toMatch(/^0x[0-9a-f]{64}$/);
    expect(first.querySelectorAll('td')[3]).toHaveTextContent('consumed by v2');
  });
});
