import { afterEach, vi } from 'vitest';
import {
  atContainerWidth,
  dottedItems,
  phoneHeadPx,
  phoneLines,
  statedHeadPx,
} from '@/__tests__/utils/phone-row';
import { inkOf } from '@/__tests__/utils/ink-role';
import { PHONE_LINE_PX } from '@/lib/type-metrics';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { InventoryRows } from '@/components/asset/inventory-table';
import { CkbAmount, TokenAmount } from '@/components/ui/ckb-amount';
import type { Asset } from '@/lib/api';
import { COMPOSITION_BADGES } from '@/lib/design-tokens';
const asset: Asset = {
  id: '0x' + 'ab'.repeat(32),
  assetType: 'token',
  standard: 'xudt',
  name: 'Token',
  symbol: 'T',
  iconUrl: null,
  published: false,
  famous: false,
  tags: null,
  holdersCount: 2,
  transfersCount: 4,
  liveCount: null,
  activitiesCount: null,
  transfers24h: 0,
  decimals: 24,
  totalSupply: '1000000000000000000000001',
  contentType: null,
  contentSize: null,
  clusterId: null,
  clusterName: null,
  ownedCapacity: '10000000001',
  ownedKnowledge: '6100000000',
  hMultiplier: '1.6393442625',
  compositionTier: null,
  onchainRatio: null,
  onchainCount: null,
};
/** A column head's name: its text, or the name over a bar column's scale ticks. */
const headText = (h: HTMLElement) => h.firstChild?.firstChild?.textContent ?? h.textContent;
/** The cell of `row` (1 = first body row) under the column head named `head`. */
function cellUnder(table: HTMLElement, row: number, head: string): HTMLElement {
  const heads = within(table).getAllByRole('columnheader');
  const i = heads.findIndex((h) => headText(h) === head);
  if (i < 0) throw new Error(`no column ${head}`);
  return within(table).getAllByRole('row')[row].children[i] as HTMLElement;
}

/** Every column head is its words in an inline box, never a bare text node filling the cell. */
function expectWordHeads(table: HTMLElement) {
  for (const th of table.querySelectorAll('thead th')) {
    expect(Array.from(th.childNodes).some((n) => n.nodeType === Node.TEXT_NODE)).toBe(false);
    expect(th.firstElementChild?.tagName).toBe('SPAN');
  }
}

describe('Inventory rows', () => {
  it('rounds a long H-multiplier to 2 dp on its one line and keeps it exact in the hover (L2-1)', () => {
    render(<InventoryRows assetType="token" rows={[asset]} />);
    const table = screen.getByRole('table', { name: 'Tokens' });
    const value = within(table).getByText('×1.64');
    expect(value.closest('td')).not.toHaveClass('!whitespace-normal');
    expect(within(table).getAllByRole('row')[1].getAttribute('title')).toContain(
      'H-mult. ×1.6393442625'
    );
  });
  it('keeps every token digit in the row title and uses the canonical standard name there', () => {
    render(<InventoryRows assetType="token" rows={[asset]} />);
    const table = screen.getByRole('table', { name: 'Tokens' });
    const row = within(table).getAllByRole('row')[1];
    // Table-rows L2-1: the row shows a shortened form; every digit and the standard are in its
    // title. Re-pinned on purpose (readability R5): the supply is the summary form, cut ("1",
    // was the two-place "1.00"), its exact value in its own title.
    expect(row).toHaveAttribute(
      'title',
      'Token · T · xUDT · 0 transfers in 24 h · 100.00000001 CKB held · 61 CKB occupied (61.00%) · supply 1.000000000000000000000001 T · H-mult. ×1.6393442625'
    );
    expect(within(table).getByTitle('1.000000000000000000000001')).toHaveTextContent(/^1$/);
    expect(table).not.toHaveTextContent('xUDT');
  });
  it('breaks a long amount only after a thousands separator and ghosts trailing zeros (IV-28/IV-13/IV-05)', () => {
    // Re-pinned on purpose (R5): AssetAmount's look is the token renderer's column form.
    const { container } = render(
      <TokenAmount form="column" amount="100000000000000000000000000000" decimals={20} />
    );
    const root = container.firstElementChild as HTMLElement;
    expect(root).toHaveTextContent('1,000,000,000.00000000000000000000');
    expect(root).not.toHaveClass('break-all');
    const breaks = Array.from(root.querySelectorAll('wbr'));
    expect(breaks.length).toBe(3);
    for (const wbr of breaks) expect(wbr.previousSibling?.textContent?.endsWith(',')).toBe(true);
    // the point joins its zeros when no decimal is significant, as CkbAmount's ghost does (R4,
    // re-pinned on purpose: the point stood alone in the amount's ink)
    expect(screen.getByText('.00000000000000000000')).toHaveClass('text-text-ghost');
    expect(container.querySelector('.text-text-muted')).toBeNull();
  });
  // UI review 2026-11-14 (DY-06, ruling R4) set a token amount's significant decimals in the
  // neutral fraction ink, never the amount's ink at 80 %. Readability R5 (re-pinned on purpose):
  // one ink per amount, the decimals in the integer's own ink, still with no opacity.
  it('sets the significant decimals in the amount’s one ink, with no opacity', () => {
    const { container } = render(<TokenAmount form="column" amount="123450000" decimals={8} />);
    const amount = container.querySelector('[data-token-amount]')!;
    expect(amount).toHaveTextContent(/^1\.23450000$/);
    expect(amount.querySelector('[data-fraction]')).toBeNull();
    expect(amount.querySelector('.text-text-dim')).toBeNull();
    expect(amount.querySelector('[data-ghost]')).toHaveTextContent(/^0000$/);
    expect(container.innerHTML).not.toMatch(/opacity-/);
  });

  it('labels unknown decimals as raw instead of guessing the unit', () => {
    render(<TokenAmount form="column" amount="12345678901234567890" decimals={null} />);
    expect(screen.getByText('12,345,678,901,234,567,890 (raw)')).toBeInTheDocument();
  });
  it('separates live, minted, activity and pending composition', () => {
    render(
      <InventoryRows
        assetType="object"
        rows={[
          {
            ...asset,
            assetType: 'object',
            standard: 'spore',
            symbol: null,
            liveCount: 3,
            totalSupply: '7',
            activitiesCount: 19,
            compositionTier: 'decode_pending',
          },
        ]}
      />
    );
    const table = screen.getByRole('table', { name: 'Object collections' });
    // Table-rows L2-2: minted ≠ live shows as a dim "/ 7" beside the live count (R4, re-pinned
    // on purpose: a count is never the ghost); the words are the row's title.
    expect(within(table).getByText('/ 7', { exact: false })).toHaveClass('text-text-dim');
    expect(within(table).getAllByRole('row')[1].getAttribute('title')).toContain(
      '3 live of 7 minted'
    );
    expect(within(table).getByText('3')).toBeInTheDocument();
    expect(within(table).getByText('19')).toBeInTheDocument();
    expect(within(table).getByText('Decode pending')).toHaveClass('text-text-dim');
  });
  it('leaves the Activity cell empty while a collection count is not derived yet (L2)', () => {
    render(
      <InventoryRows
        assetType="object"
        rows={[
          {
            ...asset,
            assetType: 'object',
            standard: 'spore',
            symbol: null,
            liveCount: 3,
            totalSupply: '7',
            activitiesCount: null,
            compositionTier: 'pure_ckb',
          },
        ]}
      />
    );
    const table = screen.getByRole('table', { name: 'Object collections' });
    const header = within(table).getByRole('columnheader', { name: 'Activity' });
    const column = Array.from(header.parentElement!.children).indexOf(header);
    const cell = within(table).getAllByRole('row')[1].children[column];
    // Unknown is said by nothing, never by 0 or a stand-in word.
    expect(cell).toBeEmptyDOMElement();
  });
  it('rejects an impossible occupied partition with the asset identifier', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <InventoryRows assetType="token" rows={[{ ...asset, ownedKnowledge: '10000000002' }]} />
      )
    ).toThrow(`${asset.id}: occupied capacity exceeds held capacity`);
    error.mockRestore();
  });
});

// Table-rows round, lane L2-1 (board Lists.dc.html · tokens): one 40 px line of six fields — mark +
// bright name + symbol tag (+ a set-back standard tag when not xUDT) · Holders · Transfers · Supply
// (dim, 2 dp) · CKB held, its 2-dp amount (its bar left with B5) · H-mult. "0 in
// 24 h", Occupied, the standard and the exact digits are the row's title (and the token page's).
describe('Tokens table on the row rules (table-rows L2-1)', () => {
  const seal: Asset = {
    ...asset,
    id: '0x' + 'cd'.repeat(32),
    standard: 'sudt',
    name: null,
    symbol: 'SEAL',
    decimals: 8,
    totalSupply: '1606360356572848',
    transfers24h: 1,
    ownedCapacity: '40000000000',
    ownedKnowledge: '18300000000',
    hMultiplier: '2.18',
  };
  const mount = () => {
    render(<InventoryRows assetType="token" rows={[asset, seal]} />);
    return screen.getByRole('table', { name: 'Tokens' });
  };

  it('draws one-line rows at the row density with the board’s six columns', () => {
    const table = mount();
    expect(within(table).getAllByRole('columnheader').map(headText)).toEqual([
      'Token',
      'Holders',
      'Transfers',
      'Supply',
      'CKB held in its cells',
      'H-mult.',
    ]);
    expect(within(table).queryByRole('columnheader', { name: 'Occupied' })).toBeNull();
    expectWordHeads(table);
    for (const td of table.querySelectorAll('tbody td'))
      expect(td).toHaveClass('py-[11px]', 'align-middle');
    expect(table).not.toHaveTextContent('in 24 h');
  });

  it('leads with the bright name, the symbol as a tag and sUDT as a set-back tag', () => {
    const table = mount();
    const [, token, sealRow] = within(table).getAllByRole('row');
    const name = within(token).getByRole('link', { name: 'Token' });
    expect(name).toHaveClass('text-body', 'text-text-bright', 'leading-row', 'truncate');
    expect(name).toHaveAttribute('href', `/mainnet/tokens/${asset.id}`);
    const tags = (row: HTMLElement) =>
      Array.from(row.querySelectorAll('[data-row-tag]')).map((t) => t.textContent);
    expect(tags(token)).toEqual(['T']);
    // A token with only a symbol shows it once, as its name; sUDT is set back in the dim ink (R4,
    // re-pinned on purpose: the standard is a word, never the ghost).
    expect(within(sealRow).getByRole('link', { name: 'SEAL' })).toBeInTheDocument();
    expect(tags(sealRow)).toEqual(['sUDT']);
    expect(within(sealRow).getByText('sUDT')).toHaveClass('text-text-dim', 'border-base-border');
    expect(token.querySelector('svg[data-class="tokens"]')).not.toBeNull();
  });

  // readability R5, re-pinned on purpose (and renamed: "prints supply dim to 2 dp, exact in its
  // hover"): a list column is the
  // summary form, whole from 1,000 up, cut, never rounded up (site precedent, lane D3).
  it('prints supply dim in its summary form, exact in its hover', () => {
    const table = mount();
    const supply = cellUnder(table, 2, 'Supply');
    expect(supply).toHaveClass('text-text-dim');
    expect(supply).toHaveTextContent(/^16,063,603$/);
    expect(within(supply).getByTitle('16,063,603.56572848')).toHaveTextContent(/^16,063,603$/);
  });

  // Re-pinned on purpose (readability B5, plan §7.6): the 8 px token-blue bar on the page's scale
  // left the list; CKB held is its amount alone, in the list's summary form (readability R5,
  // re-pinned on purpose: it was the two-place "100.00").
  it('prints CKB held as its summary amount', () => {
    const table = mount();
    const first = cellUnder(table, 1, 'CKB held in its cells');
    const second = cellUnder(table, 2, 'CKB held in its cells');
    expect(first.querySelector('[data-track]')).toBeNull();
    expect(first.querySelector('[data-ckb-amount]')).toHaveTextContent(/^100$/);
    expect(first.querySelector('[data-ckb-amount]')).toHaveAttribute('title', '100.00000001 CKB');
    expect(second.querySelector('[data-ckb-amount]')).toHaveTextContent(/^400$/);
    expect(cellUnder(table, 2, 'H-mult.')).toHaveTextContent('×2.18');
  });

  it('moves 24 h transfers, the standard, occupied and the exact digits into the row title', () => {
    const table = mount();
    expect(within(table).getAllByRole('row')[2]).toHaveAttribute(
      'title',
      'SEAL · sUDT · 1 transfer in 24 h · 400 CKB held · 183 CKB occupied (45.75%) · supply 16,063,603.56572848 SEAL · H-mult. ×2.18'
    );
  });
});

// Table-rows round, lane L2-2 (board Lists.dc.html · objects): 24 px framed mark + bright name +
// the standard and the composition as tags · Live items (a ghost "/ N" only when minted differs) ·
// Holders · Activity · CKB held (its violet bar left with B5) · H-mult. Occupied and the collection id go to
// the row's title (and the collection page).
describe('Objects table on the row rules (table-rows L2-2)', () => {
  const spore: Asset = {
    ...asset,
    id: '0x' + '5a'.repeat(32),
    assetType: 'object',
    standard: 'spore',
    name: 'Wyltek Founding Members',
    symbol: null,
    holdersCount: 32,
    liveCount: 93,
    totalSupply: '93',
    activitiesCount: 95,
    compositionTier: 'pure_ckb',
  };
  const mnft: Asset = {
    ...spore,
    id: '0x' + '6b'.repeat(32),
    standard: 'm-nft',
    name: 'Santa',
    liveCount: 2712,
    totalSupply: '2811',
    activitiesCount: 3014,
    compositionTier: 'centralized_mixture',
    ownedCapacity: '40000000000',
    ownedKnowledge: '40000000000',
    hMultiplier: '1.00',
  };
  const mount = () => {
    render(<InventoryRows assetType="object" rows={[spore, mnft]} />);
    return screen.getByRole('table', { name: 'Object collections' });
  };

  it('draws one-line rows at the row density with the board’s six columns', () => {
    const table = mount();
    expect(within(table).getAllByRole('columnheader').map(headText)).toEqual([
      'Collection',
      'Live items',
      'Holders',
      'Activity',
      'CKB held in its cells',
      'H-mult.',
    ]);
    expectWordHeads(table);
    for (const td of table.querySelectorAll('tbody td'))
      expect(td).toHaveClass('py-[11px]', 'align-middle');
    expect(table).not.toHaveTextContent(/minted|Spore ·|occupied/);
  });

  it('marks each collection with a 24 px framed thumb, its bright name and two tags', () => {
    const table = mount();
    const [, pure, central] = within(table).getAllByRole('row');
    const thumb = pure.querySelector('[data-thumb-frame="pure_ckb"]')!;
    expect(thumb).toHaveClass('h-6', 'w-6', '-my-[3px]');
    expect(thumb.querySelector('svg[data-class="objects"]')).not.toBeNull();
    expect(central.querySelector('[data-thumb-frame]')).toBeNull();
    const name = within(pure).getByRole('link', { name: 'Wyltek Founding Members' });
    expect(name).toHaveClass('text-body', 'text-text-bright', 'leading-row', 'truncate');
    const tags = (row: HTMLElement) =>
      Array.from(row.querySelectorAll('[data-row-tag]')).map((t) => [
        t.textContent,
        t.getAttribute('data-row-tag'),
      ]);
    // Re-pinned (readability 2026-11-18, one class one hue): a fully on-chain tier is the site's
    // composition badge, 18 px as a tag is, in its class's frame and ink (was the accent tag, in
    // jade for both on-chain tiers); a centralized mix is still a plain tag.
    expect(tags(pure)).toEqual([['Spore', 'default']]);
    const badge = pure.querySelector('[data-composition="pure_ckb"]')!;
    expect(badge).toHaveTextContent('Pure CKB');
    expect(badge).toHaveClass('h-[18px]', 'text-meta');
    expect(tags(central)).toEqual([
      ['m-NFT', 'default'],
      ['Centralized mix', 'default'],
    ]);
  });

  // Readability review 2026-11-18 (lane A): a "BTC+CKB" row chip was drawn in the "Pure CKB" hue
  // (both an accent row tag, jade; sampled 149,194,183 on mainnet), while the filter chip of the
  // same class above the list is in the copper ink. One class, one hue: the row draws the
  // composition from the table that owns the classes' inks (COMPOSITION_BADGES), as the filter
  // and every other page do.
  it("draws each on-chain row's composition in its own class's ink, the filter's", () => {
    const btc: Asset = {
      ...spore,
      id: '0x' + '7c'.repeat(32),
      name: 'Nervape',
      compositionTier: 'btc_ckb',
    };
    render(<InventoryRows assetType="object" rows={[spore, btc]} />);
    const table = screen.getByRole('table', { name: 'Object collections' });
    for (const [tier, label] of [
      ['pure_ckb', 'Pure CKB'],
      ['btc_ckb', 'BTC+CKB'],
    ] as const) {
      const words = within(table).getByText(label);
      expect(words).toHaveStyle({ backgroundImage: COMPOSITION_BADGES[tier].ink });
      expect(words).not.toHaveClass('text-jade');
    }
  });

  // Re-pinned on purpose (readability B5, plan §7.6): the violet CKB held bar left the list.
  it('shows "/ minted" only when it differs from live, and prints CKB held as its amount', () => {
    const table = mount();
    expect(cellUnder(table, 1, 'Live items')).toHaveTextContent(/^93$/);
    expect(cellUnder(table, 2, 'Live items')).toHaveTextContent('2,712 / 2,811');
    expect(cellUnder(table, 1, 'CKB held in its cells').querySelector('[data-segment]')).toBeNull();
    // the list's summary form (readability R5, re-pinned on purpose: was "400.00")
    expect(
      cellUnder(table, 2, 'CKB held in its cells').querySelector('[data-ckb-amount]')
    ).toHaveTextContent(/^400$/);
    expect(cellUnder(table, 2, 'Activity')).toHaveTextContent('3,014');
    expect(cellUnder(table, 1, 'H-mult.')).toHaveTextContent('×1.64');
  });

  it('moves the id, live of minted, occupied and the exact digits into the row title', () => {
    const table = mount();
    expect(within(table).getAllByRole('row')[2]).toHaveAttribute(
      'title',
      `Santa · m-NFT · ${mnft.id} · 2,712 live of 2,811 minted · 400 CKB held · 400 CKB occupied (100.00%) · H-mult. ×1.00`
    );
  });
});

// Table-rows round, lane L2-3: the identity systems on the same rules — the name bright on one
// line (the standard a tag when the name differs), counts, CKB held as its 2-dp amount (its rose bar
// left with B5), occupied in whole CKB; every digit in the row title.
describe('Identity systems table on the row rules (table-rows L2-3)', () => {
  const dotbit: Asset = {
    ...asset,
    id: 'dotbit',
    assetType: 'identity',
    standard: 'dotbit',
    name: '.bit',
    symbol: null,
    liveCount: 6082,
    totalSupply: '250043',
    holdersCount: 4269,
  };
  const bitCell: Asset = {
    ...dotbit,
    id: 'bit_cell',
    standard: 'bit_cell',
    name: 'Cellula .bit',
    liveCount: 541,
    totalSupply: '656',
    holdersCount: 366,
    ownedCapacity: '40000000000',
    ownedKnowledge: '13300000050',
  };
  const mount = () => {
    render(<InventoryRows assetType="identity" rows={[dotbit, bitCell]} />);
    return screen.getByRole('table', { name: 'Identity systems' });
  };

  it('draws one-line rows at the row density, the name bright and the standard a tag', () => {
    const table = mount();
    expect(within(table).getAllByRole('columnheader').map(headText)).toEqual([
      'System',
      'Live now',
      'Ever registered',
      'Holders',
      'CKB held in its cells',
      'Occupied',
    ]);
    expectWordHeads(table);
    for (const td of table.querySelectorAll('tbody td'))
      expect(td).toHaveClass('py-[11px]', 'align-middle');
    const [, first, second] = within(table).getAllByRole('row');
    expect(within(first).getByRole('link', { name: '.bit' })).toHaveClass(
      'text-body',
      'text-text-bright',
      'leading-row',
      'truncate'
    );
    expect(first.querySelector('[data-row-tag]')).toBeNull();
    expect(within(second).getByText('.bit Cell')).toHaveAttribute('data-row-tag', 'default');
    expect(first.querySelector('svg[data-class="identities"]')).not.toBeNull();
  });

  // Re-pinned on purpose (readability B5, plan §7.6): the rose CKB held bar left the list. CKB
  // held and occupied are the list's summary form (readability R5, re-pinned on purpose: "100.00",
  // "133.0000005"), occupied cut to 133 and titled.
  it('prints CKB held as its summary amount and keeps occupied on one line', () => {
    const table = mount();
    expect(cellUnder(table, 2, 'CKB held in its cells').querySelector('[data-segment]')).toBeNull();
    expect(
      cellUnder(table, 1, 'CKB held in its cells').querySelector('[data-ckb-amount]')
    ).toHaveTextContent(/^100$/);
    expect(cellUnder(table, 1, 'Occupied')).toHaveTextContent(/^61$/);
    const occupied = cellUnder(table, 2, 'Occupied');
    expect(occupied).toHaveTextContent(/^133$/);
    expect(occupied.querySelector('[data-ckb-amount]')).toHaveAttribute('title', '133.0000005 CKB');
    expect(occupied.querySelector('[data-ckb-amount]')).toHaveClass('whitespace-nowrap');
    expect(cellUnder(table, 2, 'Ever registered')).toHaveTextContent('656');
  });

  it('keeps every digit in the row title', () => {
    const table = mount();
    expect(within(table).getAllByRole('row')[2]).toHaveAttribute(
      'title',
      'Cellula .bit · .bit Cell · 541 live · 656 ever registered · 366 holders · 400 CKB held · 133.0000005 CKB occupied (33.25%)'
    );
  });
});

// Readability 2026-11-18, lane B5 (§7.6, approved): on a linear scale to the page's largest
// holding, the "CKB held" bar was a 1–3 px stub from about the twelfth row on, and its head stood
// over the bar while the sorted figure had none. The bar leaves the three lists; the figure, right
// aligned under its own head, shows the magnitude.
// Readability F3-e: every inventory column budgeted from the widest value it writes (lib/
// list-bounds.ts), at 0.6 em a glyph (names 14 px, cells 13, tags, heads and notes 12) and the 16 px
// gap after every column but the last. The name column is the flexible one: its marks and tags at
// their widest and 12 characters of the name at its minimum, the rest of the container beyond it;
// a longer name ends in the one ellipsis (the row's title holds it whole).
describe('inventory columns budgeted from what they hold (readability F3-e)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const at = (text: string, px: number) => (text.length * px * 3) / 5; // 0.6 em, as textPx
  const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
  /** A row tag: its words at 12 px, 5 px sides and a 1 px border. */
  const tag = (text: string) => at(text, 12) + 12;
  const name = at('x'.repeat(12), 14);
  /** A count, or "Unavailable" where the API has not derived it, whichever is wider. */
  const count = ceil(Math.max(at('99,999,999', 13), at('Unavailable', 13)) + 16);
  const held = ceil(Math.max(at('CKB held in its cells', 12), at('99,999,999,999', 13)) + 16);
  const cases = [
    [
      'token',
      'Tokens',
      [
        // the 16 px mark, a name, the symbol's tag (8 characters) and "sUDT", 8 px apart
        ceil(16 + 8 + name + 8 + tag('x'.repeat(8)) + 8 + tag('sUDT') + 16),
        count,
        count,
        // a supply to 19 characters ("100,000,000,000,000"); a wider one ends in the ellipsis
        ceil(at('100,000,000,000,000', 13) + 16),
        held,
        // "×1.64" or "Unavailable", the last column
        ceil(at('Unavailable', 13)),
      ],
      [268, 102, 102, 165, 168, 86],
    ],
    [
      'object',
      'Object collections',
      [
        // the 24 px framed thumb, 10 characters of the name (12 would make the table 984 px,
        // past the 976 of a 1024 px window), the standard's tag and the widest composition tag
        ceil(
          24 +
            8 +
            at('x'.repeat(10), 14) +
            8 +
            tag('m-NFT') +
            8 +
            tag('Composition unavailable') +
            16
        ),
        // live items and the quiet " / minted" beside them, six digits each
        ceil(at('999,999', 13) + at(' / 999,999', 12) + 16),
        count,
        // an activity count (never "Unavailable": a count not derived says nothing)
        ceil(at('99,999,999', 13) + 16),
        held,
        ceil(at('Unavailable', 13)),
      ],
      [374, 143, 102, 94, 168, 86],
    ],
    [
      'identity',
      'Identity systems',
      [
        // the 16 px mark, a name and the widest standard's tag, ".bit Cell"
        ceil(16 + 8 + name + 8 + tag('.bit Cell') + 16),
        count,
        count,
        count,
        held,
        // occupied CKB, whole from 1,000 up, the last column
        ceil(Math.max(at('99,999,999,999', 13), at('Unavailable', 13))),
      ],
      [226, 102, 102, 102, 168, 110],
    ],
  ] as const;

  it.each(cases)(
    'pins every %s column and the form at its sum',
    (assetType, label, budgets, numbers) => {
      expect(budgets).toEqual(numbers);
      const sum = budgets.reduce((s: number, w: number) => s + w, 0);
      const rows = [{ ...asset, assetType }];
      const { unmount } = render(<InventoryRows assetType={assetType} rows={rows} />);
      const table = screen.getByRole('table', { name: label });
      const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
      expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual([...budgets]);
      expect(cols[0].width).toBe('auto');
      expect(Number.parseFloat(table.style.minWidth)).toBe(sum);
      unmount();
      // every inventory table keeps its form at a 1024 px window (976 px of content)
      expect(sum).toBeLessThanOrEqual(976);
      for (const [width, form] of [
        [1232, 'table'],
        [976, 'table'],
        [sum, 'table'],
        [sum - 1, 'rows'],
        [608, 'rows'],
        [358, 'rows'],
      ] as const) {
        atContainerWidth(width);
        const { container, unmount: drop } = render(
          <InventoryRows assetType={assetType} rows={rows} />
        );
        expect(container.querySelector('[data-list-form]')).toHaveAttribute('data-list-form', form);
        drop();
        vi.unstubAllGlobals();
        vi.restoreAllMocks();
      }
    }
  );
});

describe('CKB held is a right-aligned figure under its head, with no bar (readability B5)', () => {
  const rich: Asset = {
    ...asset,
    id: '0x' + 'ef'.repeat(32),
    ownedCapacity: '40000000000',
    ownedKnowledge: '18300000000',
  };
  const cases = [
    ['token', 'Tokens'],
    ['object', 'Object collections'],
    ['identity', 'Identity systems'],
  ] as const;
  it.each(cases)('draws the %s list’s CKB held as its figure alone', (assetType, label) => {
    render(
      <InventoryRows
        assetType={assetType}
        rows={[
          { ...asset, assetType },
          { ...rich, assetType },
        ]}
      />
    );
    const table = screen.getByRole('table', { name: label });
    const head = within(table).getByRole('columnheader', { name: 'CKB held in its cells' });
    expect(head).toHaveClass('text-right');
    expect(head.querySelector('[data-scale-ticks]')).toBeNull();
    for (const row of [1, 2]) {
      const cell = cellUnder(table, row, 'CKB held in its cells');
      expect(cell).toHaveClass('text-right', 'tabular-nums', 'whitespace-nowrap');
      expect(cell.querySelector('[data-aligned-bar], [data-track], [data-segment]')).toBeNull();
    }
    // the figure in the list's summary form (lane B2, readability R5; it was cut to 2 dp)
    expect(cellUnder(table, 1, 'CKB held in its cells')).toHaveTextContent(/^100$/);
    expect(cellUnder(table, 2, 'CKB held in its cells')).toHaveTextContent(/^400$/);
  });
});

// Table-rows L2 follow-up: a row's shortened amount is the exact amount CUT, never rounded (a
// rounded figure can show a digit that is not on chain); the exact value is in the title.
// Readability R5 (re-pinned on purpose): the row's form is the summary form, which keeps that rule
// — whole from 1,000 up (was two places: "1,234.56"), two places at most from 1 to 1,000, below 1
// the exact CKB or a token's four significant digits (was "0.99").
describe('row amounts truncate, never round (table-rows L2, R5)', () => {
  it('cuts 1,234.56789 to 1,234 for CKB and token amounts, the exact value in the title', () => {
    const { container, rerender } = render(
      <CkbAmount
        form="summary"
        shannons={BigInt('123456789000')}
        signed={false}
        tone="inherit"
        unit={false}
      />
    );
    expect(container).toHaveTextContent(/^1,234$/);
    expect(container.firstElementChild).toHaveAttribute('title', '1,234.56789 CKB');
    rerender(<TokenAmount form="summary" amount="123456789" decimals={5} />);
    expect(container).toHaveTextContent(/^1,234$/);
    expect(container.firstElementChild).toHaveAttribute('title', '1,234.56789');
  });
  it('never carries .999 into the next whole unit', () => {
    const { container, rerender } = render(
      <CkbAmount
        form="summary"
        shannons={BigInt('99999900000')}
        signed={false}
        tone="inherit"
        unit={false}
      />
    );
    expect(container).toHaveTextContent(/^999\.99$/);
    rerender(<TokenAmount form="summary" amount="99999999" decimals={8} />);
    expect(container).toHaveTextContent(/^0\.9999$/);
    rerender(<TokenAmount form="summary" amount="1999" decimals={3} />);
    expect(container).toHaveTextContent(/^1\.99$/);
    // A unit with fewer than 2 decimals prints what it has.
    rerender(<TokenAmount form="summary" amount="9999" decimals={1} />);
    expect(container).toHaveTextContent(/^999\.9$/);
  });
});

describe('Inventory phone rows (boards L3, L4)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const spore: Asset = {
    ...asset,
    assetType: 'object',
    standard: 'spore',
    name: 'Wyltek Founding Members',
    symbol: null,
    holdersCount: 32,
    liveCount: 93,
    totalSupply: '93',
    activitiesCount: 95,
    compositionTier: 'pure_ckb',
  };

  // Re-pinned on purpose (lane D4, the phone-rows board; the describe below pins the rows): line 1's
  // figure is the CKB the cells hold, the list's sort (it was the holders); the tags and the named
  // figures are line 2, the H-multiplier named and grouped (it was a bare "×1.6393442625" at the
  // right); line 3 the supply and the occupied CKB; the transfers of the last 24 h left the row for
  // its title (it was "xUDT · 4 transfers · 0 in 24 h"; xUDT, the default standard, is no tag, as in
  // the table).
  it('writes a token in three lines at 358 px: name and CKB · named figures · supply and occupied', () => {
    atContainerWidth(358);
    render(<InventoryRows assetType="token" rows={[asset]} />);
    expect(screen.queryByRole('table')).toBeNull();
    const [row] = screen.getAllByTestId('inventory-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.tone, l.left, l.right])).toEqual([
      ['identity', 'head', 'Token', '100 CKB'],
      ['what', 'body', 'T · 2 holders · 4 transfers · H-mult. ×1.64', null],
      // the supply and the occupied CKB with its exact share, in a phone row's summary form
      // (readability R5; each cut figure has its exact value in its title)
      ['figures', 'quiet', 'supply 1 T · 61 CKB occupied (61.00%)', null],
    ]);
    expect(row.querySelector('.truncate')).toBeNull();
  });

  // Re-pinned on purpose (lane D4): the card at 608 px is the phone row's three parts (it was two
  // lines, the standard beside the name and the counts at the right of the supply).
  it('writes a token as the phone row’s three parts at 608 px', () => {
    atContainerWidth(608);
    render(<InventoryRows assetType="token" rows={[asset]} />);
    const [row] = screen.getAllByTestId('inventory-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', 'Token', '100 CKB'],
      ['what', 'T · 2 holders · 4 transfers · H-mult. ×1.64', null],
      ['figures', 'supply 1 T · 61 CKB occupied (61.00%)', null],
    ]);
  });

  // Re-pinned on purpose (lane D4): the composition chip keeps line 2's right edge (held), the
  // collection's live objects are named there with its holders and activities, and the id left the
  // quiet line (PA-09: it ended the row as a bare hash; the row's title and the collection's page
  // keep it). Line 1's figure is the CKB its cells hold (it was "93 objects").
  it('writes a collection in three lines with the composition mark (board L4)', () => {
    atContainerWidth(358);
    render(<InventoryRows assetType="object" rows={[spore]} />);
    const [row] = screen.getAllByTestId('inventory-row-mobile');
    const lines = phoneLines(row);
    expect(lines.map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', 'Wyltek Founding Members', '100 CKB'],
      ['what', 'Spore · 93 objects · 32 holders · 95 activities', 'Pure CKB'],
      // a phone row's summary form (R5: was "100.00000001")
      ['figures', '93 minted · 61 CKB occupied (61.00%) · H-mult. ×1.64', null],
    ]);
    expect(row.querySelector('[data-composition="pure_ckb"]')).not.toBeNull();
  });

  // Re-pinned on purpose (lane D4): line 1's figure is the CKB the cells hold (it was "5 live"), the
  // live, registered and holder counts named on line 2.
  it('writes an identity system with its live, registered and holder counts', () => {
    atContainerWidth(358);
    render(
      <InventoryRows
        assetType="identity"
        rows={[
          {
            ...asset,
            assetType: 'identity',
            standard: 'dotbit',
            name: '.bit',
            liveCount: 5,
            totalSupply: '9',
          },
        ]}
      />
    );
    const [row] = screen.getAllByTestId('inventory-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '.bit', '100 CKB'],
      ['what', '5 live · 9 ever registered · 2 holders', null],
      // a phone row's summary form (readability R5: was "100.00000001")
      ['figures', '61 CKB occupied (61.00%)', null],
    ]);
  });
});

// Lane D4 (the phone-rows board, its tokens pane; objects and identities by its rules): line 1 the
// class's mark, the name and the CKB its cells hold, the figure the list sorts by ("sorted by ckb
// held in its cells"), at the key's step in the bright ink; line 2 what it is and who uses it, a
// separated list of named figures that wraps between them — its tags, its holders, its transfers or
// objects or names, the H-multiplier named and grouped (PA-09: it was a bare "×15019.09") — and, on
// an object collection's row, the composition chip at its right, where it stays whatever the
// list's length (the owner's rule: on every row of an object list); line 3 quiet, the rest. The
// collection's id leaves the row (PA-09: a bare hash at its end); the transfers of the last 24 h
// leave it for the row's title.
describe('the inventory phone rows on the phone-rows board (lane D4)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const spore: Asset = {
    ...asset,
    id: '0x' + 'cd'.repeat(32),
    assetType: 'object',
    standard: 'spore',
    name: 'Wyltek Founding Members',
    symbol: null,
    holdersCount: 32,
    liveCount: 93,
    totalSupply: '93',
    activitiesCount: 95,
    compositionTier: 'pure_ckb',
  };
  const lines = (row: HTMLElement) => phoneLines(row).map((l) => [l.key, l.tone, l.left, l.right]);

  it('writes a token as name and CKB held · tags and named figures · supply and occupied', () => {
    atContainerWidth(358);
    render(
      <InventoryRows
        assetType="token"
        rows={[
          asset,
          { ...asset, id: '0x' + 'ef'.repeat(32), standard: 'sudt', hMultiplier: '15019.0912' },
        ]}
      />
    );
    const [token, sudt] = screen.getAllByTestId('inventory-row-mobile');
    expect(lines(token)).toEqual([
      ['identity', 'head', 'Token', '100 CKB'],
      ['what', 'body', 'T · 2 holders · 4 transfers · H-mult. ×1.64', null],
      ['figures', 'quiet', 'supply 1 T · 61 CKB occupied (61.00%)', null],
    ]);
    expect(phoneLines(sudt)[1].left).toBe(
      'T · sUDT · 2 holders · 4 transfers · H-mult. ×15,019.09'
    );
    const labels = Array.from(token.querySelectorAll('[data-role="figure-label"]'));
    expect(labels.map((l) => l.textContent)).toEqual([
      'holders',
      'transfers',
      'H-mult.',
      'supply',
      'occupied',
    ]);
    for (const label of labels) expect(inkOf(label)).toBe('text-text-dim');
    expect(token.querySelector('[data-line="identity"] > [data-part="right"] > *')).toHaveClass(
      'text-body',
      'text-text-bright'
    );
  });

  it('writes an object collection with its composition chip on line 2, and no bare id', () => {
    atContainerWidth(358);
    render(<InventoryRows assetType="object" rows={[spore]} />);
    const [row] = screen.getAllByTestId('inventory-row-mobile');
    expect(lines(row)).toEqual([
      ['identity', 'head', 'Wyltek Founding Members', '100 CKB'],
      ['what', 'body', 'Spore · 93 objects · 32 holders · 95 activities', 'Pure CKB'],
      ['figures', 'quiet', '93 minted · 61 CKB occupied (61.00%) · H-mult. ×1.64', null],
    ]);
    // the chip holds line 2's right edge: the line does not wrap it under its figures
    const what = row.querySelector('[data-line="what"]')!;
    expect(what).toHaveClass('flex-nowrap');
    expect(what.querySelector('[data-part="right"] [data-composition="pure_ckb"]')).not.toBeNull();
    expect(row.textContent).not.toMatch(/0xcdcd/);
  });

  it('writes an identity system with its live, registered and holder counts named', () => {
    atContainerWidth(358);
    render(
      <InventoryRows
        assetType="identity"
        rows={[
          {
            ...asset,
            assetType: 'identity',
            standard: 'dotbit',
            name: '.bit',
            liveCount: 5,
            totalSupply: '9',
          },
        ]}
      />
    );
    const [row] = screen.getAllByTestId('inventory-row-mobile');
    expect(lines(row)).toEqual([
      ['identity', 'head', '.bit', '100 CKB'],
      ['what', 'body', '5 live · 9 ever registered · 2 holders', null],
      ['figures', 'quiet', '61 CKB occupied (61.00%)', null],
    ]);
    expect(dottedItems(row.querySelector('[data-line="what"]')!)).toEqual([
      '5 live',
      '9 ever registered',
      '2 holders',
    ]);
  });

  // The board's head(): the class's mark and its 6 px (18), an 18-character name at the key's
  // 14 px (151.2), the 12 px between the parts and the widest CKB a row's cells hold at the key's
  // step, "99,999,999,999" (117.6) and " CKB" at the unit's 12 px (28.8): 327.6 of 328 px. A longer
  // name drops the widest figure under it, whole.
  it('states its first line, 327.6 of 328 px, and draws an 18-character name with the widest figure', () => {
    atContainerWidth(PHONE_LINE_PX[360]);
    render(
      <InventoryRows
        assetType="token"
        rows={[
          {
            ...asset,
            name: 'Eighteen chars nam',
            ownedCapacity: '9999999999999999999',
            ownedKnowledge: '100000000',
          },
        ]}
      />
    );
    const [row] = screen.getAllByTestId('inventory-row-mobile');
    expect(phoneLines(row)[0].right).toBe('99,999,999,999 CKB');
    expect(statedHeadPx(row)).toBe(327.6);
    expect(phoneHeadPx(row, 360)).toBe(statedHeadPx(row));
  });
});

// Lane D4 follow-up (the coordinator's item 3): the inventory's H-multiplier, in the table's cell
// and on the phone row, is cut at two decimals, never rounded: the API's 1.0059 reads ×1.00 (it read
// ×1.01) and 15019.0999 reads ×15,019.09 (×15,019.10).
describe('the inventory’s H-multiplier is cut, never rounded', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const rows = [
    { ...asset, id: '0x' + 'a1'.repeat(32), hMultiplier: '1.0059' },
    { ...asset, id: '0x' + 'a2'.repeat(32), hMultiplier: '15019.0999' },
  ];

  it('in the table', () => {
    atContainerWidth(1248);
    render(<InventoryRows assetType="token" rows={rows} />);
    const table = screen.getByRole('table');
    expect(cellUnder(table, 1, 'H-mult.')).toHaveTextContent(/^×1\.00$/);
    expect(cellUnder(table, 2, 'H-mult.')).toHaveTextContent(/^×15,019\.09$/);
  });

  it('on the phone row', () => {
    atContainerWidth(358);
    render(<InventoryRows assetType="token" rows={rows} />);
    const [first, second] = screen.getAllByTestId('inventory-row-mobile');
    expect(phoneLines(first)[1].left).toMatch(/ · H-mult\. ×1\.00$/);
    expect(phoneLines(second)[1].left).toMatch(/ · H-mult\. ×15,019\.09$/);
  });
});
