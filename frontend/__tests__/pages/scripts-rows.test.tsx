import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ScriptsPage from '@/app/scripts/page';
import { api, type KnownScript } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: { getScripts: vi.fn() },
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div /> }));

/** The px between an AlignedBar's track and its figure (its `gap-2.5`). */
const BAR_FIGURE_GAP = 10;

// Table-rows plan L1-1 (board Scripts.dc.html, rules T1–T6, ruling D3): one 40 px line per script —
// name + role tag, code status, live cells, CKB with the page's one bar, occupied share. The
// description, the holder and date, "of N ever", the exact amount and the occupied CKB are the
// row's hover (its title) and the script page.
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

const defaultLock = family({
  codeHash: 'family:default-lock',
  name: 'Default Lock',
  description: 'SECP256K1/blake160 is the default lock script',
  ownedCapacitySum: '4674323611196022084',
  ownedKnowledgeSum: '6530970100000000',
  liveCellsCount: 886257,
  cellsCount: 78323235,
  versionsCount: 1,
  firstDeployedAt: {
    blockNumber: 0,
    timestampMs: 1573852190812,
    versionHash: '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649',
  },
  codeClasses: [
    { class: 'fixed', lockCellsCount: 1, lockOwnedCapacitySum: '1', referenceCount: 2 },
    { class: 'unspendable', lockCellsCount: 2, lockOwnedCapacitySum: '2', referenceCount: 1 },
  ],
});
const joyid = family({
  codeHash: 'family:joyid',
  name: 'JoyID',
  description: 'An universal Account Protocol for Web3 Mass-adoption.',
  ownedCapacitySum: '45435269486181731',
  ownedKnowledgeSum: '1909357700000000',
  liveCellsCount: 161008,
  cellsCount: 2707977,
  versionsCount: 5,
  firstDeployedAt: {
    blockNumber: 11132140,
    timestampMs: 1696664371829,
    versionHash: '0x0b6a8ef9dee12629aaa738266a56e6e0ec3330782af9ac29f50e04b0daa1845c',
  },
  codeClasses: [
    { class: 'multisig', lockCellsCount: 161008, lockOwnedCapacitySum: '1', referenceCount: 1 },
  ],
});
const bitLock = family({
  codeHash: 'family:bit-lock',
  name: '.bit Lock',
  scriptKind: 'lock+type',
  description: 'Unified DID, access from anywhere, use it everywhere.',
  ownedCapacitySum: '10125773016787966',
  ownedKnowledgeSum: '3083378800000000',
  liveCellsCount: 259686,
  cellsCount: 2805629,
  versionsCount: 34,
  firstDeployedAt: {
    blockNumber: 4849427,
    timestampMs: 1626669308352,
    versionHash: '0xe683b04139344768348499c23eb1326d5a52d6db006c0d2fece00a831f3660d7',
  },
  codeClasses: [
    { class: 'multisig', lockCellsCount: 1, lockOwnedCapacitySum: '1', referenceCount: 2 },
    { class: 'one_key', lockCellsCount: 1, lockOwnedCapacitySum: '1', referenceCount: 1 },
  ],
});
const pwLock = family({
  codeHash: 'family:pw-lock',
  name: 'PW Lock',
  deprecated: true,
  description: "Forked from CKB's system scripts",
  ownedCapacitySum: '28190026043787127',
  ownedKnowledgeSum: '331359000000000',
  liveCellsCount: 29229,
  cellsCount: 209529,
  versionsCount: 3,
  firstDeployedAt: {
    blockNumber: 2417685,
    timestampMs: 1596253756344,
    versionHash: '0x7d9a474b479e6881d738de11874f009dd88895713e34a6e8bc16b05220f881c6',
  },
  codeClasses: [
    { class: 'multisig', lockCellsCount: 1, lockOwnedCapacitySum: '1', referenceCount: 1 },
  ],
});
const empty = family({
  codeHash: 'family:always-success',
  name: 'AlwaysSuccess',
  scriptKind: 'lock+type',
  ownedCapacitySum: '0',
  ownedKnowledgeSum: '0',
  liveCellsCount: 0,
  cellsCount: 0,
  codeClasses: [],
});

const rows = [defaultLock, joyid, bitLock, pwLock, empty];
const response = {
  data: rows,
  total: 70,
  limit: 50,
  hasMore: true,
  nextCursor: 'c',
  roleCounts: { all: 70, locks: 41, types: 34 },
};

const table = () => screen.findByRole('table', { name: 'Scripts' });
const rowOf = (t: HTMLElement, name: string) =>
  within(t).getByRole('link', { name }).closest('tr') as HTMLElement;
const cells = (row: HTMLElement) => [...row.querySelectorAll('td')] as HTMLElement[];

describe('Scripts table rows (table-rows L1-1, board Scripts)', () => {
  beforeEach(() => {
    vi.mocked(api.getScripts).mockResolvedValue(response);
  });

  it('draws five columns: Script · Code · Live cells · CKB in its cells · Occupied', async () => {
    render(<ScriptsPage />);
    const t = await table();
    const heads = within(t)
      .getAllByRole('columnheader')
      .map((h) => h.firstElementChild?.firstElementChild?.textContent ?? h.textContent);
    expect(heads).toEqual(['Script', 'Code', 'Live cells', 'CKB in its cells', 'Occupied']);
  });

  it('heads its columns at 11 px (T5)', async () => {
    render(<ScriptsPage />);
    const t = await table();
    // Re-pinned on purpose (B5): the one bar column is now Occupied (index 4, was CKB at 3); its
    // name is the first line of its scale head.
    for (const [i, h] of within(t).getAllByRole('columnheader').entries())
      if (i !== 4) expect(h.firstElementChild).toHaveClass('text-meta');
      else expect(h.firstElementChild?.firstElementChild).toHaveClass('text-meta');
  });

  it('is one 40 px line a row: row density, every cell centred (T1, T2)', async () => {
    render(<ScriptsPage />);
    const t = await table();
    for (const td of cells(rowOf(t, 'JoyID'))) {
      expect(td).toHaveClass('py-[11px]', 'align-middle');
      expect(td.querySelector('.block, br')).toBeNull();
    }
  });

  it('leads each row with the name at 13 px bright, its role and a warn "deprecated" as tags (T5)', async () => {
    render(<ScriptsPage />);
    const t = await table();
    const name = within(t).getByRole('link', { name: 'JoyID' });
    expect(name).toHaveClass('text-body', 'text-text-bright', 'leading-[18px]');
    const [script] = cells(rowOf(t, 'JoyID'));
    expect(
      [...script.querySelectorAll<HTMLElement>('[data-row-tag]')].map((tag) => [
        tag.dataset.rowTag,
        tag.textContent,
      ])
    ).toEqual([['default', 'lock']]);
    expect(
      [...cells(rowOf(t, '.bit Lock'))[0].querySelectorAll('[data-row-tag]')].map(
        (tag) => tag.textContent
      )
    ).toEqual(['lock + type']);
    expect(
      [...cells(rowOf(t, 'PW Lock'))[0].querySelectorAll<HTMLElement>('[data-row-tag]')].map(
        (tag) => [tag.dataset.rowTag, tag.textContent]
      )
    ).toEqual([
      ['default', 'lock'],
      // Lane F follow-up 2: the warning (was ['ghost', 'deprecated']).
      ['warn', 'deprecated'],
    ]);
    // The description is the row's hover, not a second line.
    expect(script).not.toHaveTextContent('An universal Account Protocol');
    // One bright thing a row — the name; the rest in body ink or dim.
    expect(rowOf(t, 'JoyID')).toHaveClass('text-text');
    expect(rowOf(t, 'JoyID').querySelectorAll('.text-text-bright')).toHaveLength(1);
  });

  it('writes the code status on one dim line, "by whom · since" in its title (D3)', async () => {
    render(<ScriptsPage />);
    const t = await table();
    const code = (name: string) => within(rowOf(t, name)).getByTestId('script-code-class');
    expect(code('JoyID')).toHaveTextContent(/^upgradable · 5 versions$/);
    expect(code('JoyID')).toHaveClass('text-text-dim');
    expect(code('JoyID')).toHaveAttribute(
      'title',
      'upgradable · 5\u00a0versions · by a multisig · since 2023-10-07'
    );
    expect(code('.bit Lock')).toHaveTextContent(/^upgradable · 34 versions$/);
    expect(code('Default Lock')).toHaveTextContent(/^immutable$/);
    expect(code('Default Lock')).toHaveAttribute(
      'title',
      'immutable · genesis · code cell unspendable · pinned by data hash'
    );
    // Lane F follow-up: immutable code is the rarer, stronger fact — the cell's one accent, a jade
    // row tag (was dim text like "upgradable"); upgradable stays dim text with no tag.
    const immutable = within(code('Default Lock')).getByText('immutable');
    expect(immutable).toHaveAttribute('data-row-tag', 'accent');
    expect(immutable).toHaveClass('text-jade');
    expect(code('JoyID').querySelector('[data-row-tag]')).toBeNull();
    expect(code('.bit Lock').querySelector('[data-row-tag]')).toBeNull();
    // T5 still holds: the name is the row's one bright thing; the tag is an accent, not bright.
    expect(rowOf(t, 'Default Lock').querySelectorAll('.text-text-bright')).toHaveLength(1);
    expect(code('AlwaysSuccess')).toHaveTextContent(/^code evidence unavailable$/);
  });

  it('shows live cells alone — "of N ever" leaves the row for its title (T1)', async () => {
    render(<ScriptsPage />);
    const t = await table();
    const live = cells(rowOf(t, 'JoyID'))[2];
    expect(live).toHaveTextContent(/^161,008$/);
    expect(live.firstElementChild).toHaveAttribute(
      'title',
      '161,008 live of 2,707,977\u00a0cells ever'
    );
  });

  // Re-pinned on purpose (B5, plan §7.6): the CKB column's 8 px bar on the page scale (50B, "0 ·
  // 25B · 50B") left the list, a 2 px stub on most rows; the amount to 2 dp stays.
  it('prints CKB in its cells as the amount to 2 dp, with no bar (T4, B5)', async () => {
    render(<ScriptsPage />);
    const t = await table();
    const head = within(t).getAllByRole('columnheader')[3];
    expect(head.querySelector('[data-scale-ticks]')).toBeNull();
    for (const row of within(t).getAllByRole('row').slice(1))
      expect(cells(row)[3].querySelector('[data-track]')).toBeNull();
    const ckb = cells(rowOf(t, 'JoyID'))[3];
    expect(ckb).toHaveTextContent(/^454,352,694\.86$/);
    expect(cells(rowOf(t, 'AlwaysSuccess'))[3]).toHaveTextContent(/^0\.00$/);
    // 2 dp by truncation: .23594289 is .23, never rounded up to .24.
    expect(cells(rowOf(t, 'Default Lock'))[3].querySelector('[data-ckb-2dp]')!.textContent).toBe(
      '46,743,236,111.96'
    );
    expect(ckb.textContent).not.toMatch(/\d\.\d{8}/);
  });

  // Re-pinned on purpose (B5): the share now stands right of its bar, so the dim share is the bar's
  // figure, not the cell's first element.
  it('keeps the occupied share in the last column, dim', async () => {
    render(<ScriptsPage />);
    const t = await table();
    const occupied = cells(rowOf(t, 'JoyID'))[4];
    expect(occupied).toHaveTextContent(/^4\.20%$/);
    expect(within(occupied).getByText('4.20%')).toHaveClass('text-text-dim');
  });

  it('puts every field that left the row in its title (D3: nothing deleted)', async () => {
    render(<ScriptsPage />);
    const t = await table();
    expect(rowOf(t, 'JoyID')).toHaveAttribute(
      'title',
      'JoyID · lock · An universal Account Protocol for Web3 Mass-adoption. · upgradable · 5\u00a0versions · by a multisig · since 2023-10-07 · 161,008 live of 2,707,977\u00a0cells ever · 454,352,694.86181731 CKB · 19,093,577.00000000 occupied (4.20%)'
    );
    expect(rowOf(t, 'Default Lock')).toHaveAttribute(
      'title',
      'Default Lock · lock · SECP256K1/blake160 is the default lock script · immutable · genesis · code cell unspendable · pinned by data hash · 886,257 live of 78,323,235\u00a0cells ever · 46,743,236,111.96022084 CKB · 65,309,701.00000000 occupied (0.14%)'
    );
    expect(rowOf(t, 'PW Lock').getAttribute('title')).toContain('PW Lock · lock · deprecated · ');
    expect(rowOf(t, 'AlwaysSuccess')).toHaveAttribute(
      'title',
      'AlwaysSuccess · lock + type · code evidence unavailable · 0 live of 0\u00a0cells ever · 0.00000000 CKB · 0.00000000 occupied (0.00%)'
    );
  });

  // Readability 2026-11-18, lane B5. §7.6 (approved): the linear CKB bar, scaled to 50B, was a 1 px
  // stub on 46 of 50 rows; it leaves the list and the amount, right aligned under its own head,
  // shows the magnitude. The occupied share's bar (0–100 %) stays, beside the share it pictures,
  // and with one bar left the "bar:" switch has one state and goes. A head stands over what it
  // names: the column's name over its figure, the ticks over the track.
  it('prints CKB in its cells as its amount alone, right aligned under its head (B5)', async () => {
    render(<ScriptsPage />);
    const t = await table();
    const head = within(t).getAllByRole('columnheader')[3];
    expect(head).toHaveTextContent(/^CKB in its cells$/);
    expect(head).toHaveClass('text-right');
    expect(head.querySelector('[data-scale-ticks]')).toBeNull();
    const ckb = cells(rowOf(t, 'JoyID'))[3];
    expect(ckb).toHaveClass('text-right', 'tabular-nums');
    expect(ckb.querySelector('[data-aligned-bar], [data-track]')).toBeNull();
    expect(ckb).toHaveTextContent(/^454,352,694\.86$/);
  });

  it('draws the occupied share on 0 · 50 · 100 %, its name over the figure (B5)', async () => {
    render(<ScriptsPage />);
    const t = await table();
    const head = within(t).getAllByRole('columnheader')[4];
    const ticks = head.querySelector<HTMLElement>('[data-scale-ticks]')!;
    expect([...ticks.children].map((n) => n.textContent)).toEqual(['0', '50', '100%']);
    const share = cells(rowOf(t, 'JoyID'))[4];
    const track = share.querySelector<HTMLElement>('[data-track]')!;
    const figure = share.querySelector<HTMLElement>('[data-aligned-bar] > :last-child')!;
    expect(figure).toHaveTextContent(/^4\.20%$/);
    // 4.20 % of the track (the bar rounds up to the next ppm, so a share is never drawn empty)
    const fill = share.querySelector<HTMLElement>('[data-segment]')!;
    expect(Number.parseFloat(fill.style.width) / Number.parseFloat(track.style.width)).toBeCloseTo(
      0.042,
      3
    );
    // The name spans the track, the bar's gap and the figure, and ends where the figure ends.
    const name = within(head).getByText('Occupied');
    expect(name).toHaveClass('justify-end');
    expect(Number.parseFloat(name.style.width)).toBe(
      Number.parseFloat(track.style.width) + BAR_FIGURE_GAP + Number.parseFloat(figure.style.width)
    );
    expect(ticks.style.width).toBe(track.style.width);
  });

  it('offers no bar switch: the one bar left has one state (B5)', async () => {
    render(<ScriptsPage />);
    await table();
    expect(screen.queryByRole('group', { name: 'Bar shows' })).toBeNull();
    expect(screen.queryByText(/^bar:$/)).toBeNull();
  });

  it('keeps each row within 90 visible characters (T6)', async () => {
    render(<ScriptsPage />);
    const t = await table();
    for (const row of within(t).getAllByRole('row').slice(1))
      expect(row.textContent!.replace(/\s+/g, ' ').trim().length).toBeLessThanOrEqual(90);
  });
});
