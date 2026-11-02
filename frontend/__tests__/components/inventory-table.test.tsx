import { afterEach, vi } from 'vitest';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import {
  InventoryRows,
  AssetAmount,
  CkbTwoPlaces,
  TokenTwoPlaces,
} from '@/components/asset/inventory-table';
import type { Asset } from '@/lib/api';
import { resourceScale } from '@/components/blocks/block-visual';
import { formatTick } from '@/lib/chart-ticks';
import { CLASS_COLORS } from '@/lib/design-tokens';
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
const TRACK = 120;

/** Every column head is its words in an inline box, never a bare text node filling the cell. */
function expectWordHeads(table: HTMLElement) {
  for (const th of table.querySelectorAll('thead th')) {
    expect(Array.from(th.childNodes).some((n) => n.nodeType === Node.TEXT_NODE)).toBe(false);
    expect(th.firstElementChild?.tagName).toBe('SPAN');
  }
}
const barWidth = (value: number, scale: number) =>
  `${Math.max(Math.round((value / scale) * TRACK * 100) / 100, value > 0 ? 2 : 0)}px`;

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
    // Table-rows L2-1: the row shows 2 dp; every digit and the standard are in its title.
    expect(row).toHaveAttribute(
      'title',
      'Token · T · xUDT · 0 transfers in 24 h · 100.00000001 CKB held · 61 CKB occupied (61.0%) · supply 1.000000000000000000000001 T · H-mult. ×1.6393442625'
    );
    expect(within(table).getByTitle('1.000000000000000000000001')).toHaveTextContent('1.00');
    expect(table).not.toHaveTextContent('xUDT');
  });
  it('breaks a long amount only after a thousands separator and ghosts trailing zeros (IV-28/IV-13/IV-05)', () => {
    const { container } = render(
      <AssetAmount amount="100000000000000000000000000000" decimals={20} />
    );
    const root = container.firstElementChild as HTMLElement;
    expect(root).toHaveTextContent('1,000,000,000.00000000000000000000');
    expect(root).not.toHaveClass('break-all');
    const breaks = Array.from(root.querySelectorAll('wbr'));
    expect(breaks.length).toBe(3);
    for (const wbr of breaks) expect(wbr.previousSibling?.textContent?.endsWith(',')).toBe(true);
    expect(screen.getByText('00000000000000000000')).toHaveClass('text-text-ghost');
    expect(container.querySelector('.text-text-muted')).toBeNull();
  });
  it('labels unknown decimals as raw instead of guessing the unit', () => {
    render(<AssetAmount amount="12345678901234567890" decimals={null} />);
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
    // Table-rows L2-2: minted ≠ live shows as a ghost "/ 7" beside the live count; the words
    // are the row's title.
    expect(within(table).getByText('/ 7', { exact: false })).toHaveClass('text-text-ghost');
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
// (dim, 2 dp) · CKB held as an 8 px bar on the page's scale with its 2-dp amount · H-mult. "0 in
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
    expect(name).toHaveClass('text-body', 'text-text-bright', 'leading-[18px]', 'truncate');
    expect(name).toHaveAttribute('href', `/mainnet/tokens/${asset.id}`);
    const tags = (row: HTMLElement) =>
      Array.from(row.querySelectorAll('[data-row-tag]')).map((t) => t.textContent);
    expect(tags(token)).toEqual(['T']);
    // A token with only a symbol shows it once, as its name; sUDT is set back in ghost ink.
    expect(within(sealRow).getByRole('link', { name: 'SEAL' })).toBeInTheDocument();
    expect(tags(sealRow)).toEqual(['sUDT']);
    expect(within(sealRow).getByText('sUDT')).toHaveClass('text-text-ghost', 'border-base-border');
    expect(token.querySelector('svg[data-class="tokens"]')).not.toBeNull();
  });

  it('prints supply dim to 2 dp, exact in its hover', () => {
    const table = mount();
    const supply = cellUnder(table, 2, 'Supply');
    expect(supply).toHaveClass('text-text-dim');
    // Cut at 2 dp, never rounded up (site precedent, lane D3): .56572848 shows as .56.
    expect(supply).toHaveTextContent('16,063,603.56');
    expect(supply).not.toHaveTextContent('16,063,603.57');
    expect(within(supply).getByTitle('16,063,603.56572848')).toBeInTheDocument();
  });

  it('draws CKB held as an 8 px token-blue bar on the page’s scale with a 2-dp amount', () => {
    const table = mount();
    const scale = resourceScale([100, 400]);
    const head = within(table).getByRole('columnheader', { name: 'CKB held in its cells' });
    expect(
      Array.from(head.querySelector('[data-scale-ticks]')!.children).map((t) => t.textContent)
    ).toEqual(['0', formatTick(scale / 2, scale / 2), formatTick(scale, scale / 2)]);
    const first = cellUnder(table, 1, 'CKB held in its cells');
    const second = cellUnder(table, 2, 'CKB held in its cells');
    expect(first.querySelector('[data-track]')).toHaveStyle({ height: '8px', width: '120px' });
    expect(first.querySelector('[data-segment]')).toHaveStyle({
      width: barWidth(100, scale),
      backgroundColor: CLASS_COLORS.tokens,
    });
    expect(second.querySelector('[data-segment]')).toHaveStyle({ width: barWidth(400, scale) });
    expect(first).toHaveTextContent('100.00');
    expect(second).toHaveTextContent('400.00');
    expect(cellUnder(table, 2, 'H-mult.')).toHaveTextContent('×2.18');
  });

  it('moves 24 h transfers, the standard, occupied and the exact digits into the row title', () => {
    const table = mount();
    expect(within(table).getAllByRole('row')[2]).toHaveAttribute(
      'title',
      'SEAL · sUDT · 1 transfer in 24 h · 400 CKB held · 183 CKB occupied (45.8%) · supply 16,063,603.56572848 SEAL · H-mult. ×2.18'
    );
  });
});

// Table-rows round, lane L2-2 (board Lists.dc.html · objects): 24 px framed mark + bright name +
// the standard and the composition as tags · Live items (a ghost "/ N" only when minted differs) ·
// Holders · Activity · CKB held as a violet 8 px bar · H-mult. Occupied and the collection id go to
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
    expect(name).toHaveClass('text-body', 'text-text-bright', 'leading-[18px]', 'truncate');
    const tags = (row: HTMLElement) =>
      Array.from(row.querySelectorAll('[data-row-tag]')).map((t) => [
        t.textContent,
        t.getAttribute('data-row-tag'),
      ]);
    // Fully on-chain tiers are the accent tag; a centralized mix is a plain one.
    expect(tags(pure)).toEqual([
      ['Spore', 'default'],
      ['Pure CKB', 'accent'],
    ]);
    expect(tags(central)).toEqual([
      ['m-NFT', 'default'],
      ['Centralized mix', 'default'],
    ]);
  });

  it('shows "/ minted" only when it differs from live, and draws CKB held in violet', () => {
    const table = mount();
    expect(cellUnder(table, 1, 'Live items')).toHaveTextContent(/^93$/);
    expect(cellUnder(table, 2, 'Live items')).toHaveTextContent('2,712 / 2,811');
    const scale = resourceScale([100, 400]);
    expect(
      cellUnder(table, 1, 'CKB held in its cells').querySelector('[data-segment]')
    ).toHaveStyle({ width: barWidth(100, scale), backgroundColor: CLASS_COLORS.objects });
    expect(cellUnder(table, 2, 'CKB held in its cells')).toHaveTextContent('400.00');
    expect(cellUnder(table, 2, 'Activity')).toHaveTextContent('3,014');
    expect(cellUnder(table, 1, 'H-mult.')).toHaveTextContent('×1.64');
  });

  it('moves the id, live of minted, occupied and the exact digits into the row title', () => {
    const table = mount();
    expect(within(table).getAllByRole('row')[2]).toHaveAttribute(
      'title',
      `Santa · m-NFT · ${mnft.id} · 2,712 live of 2,811 minted · 400 CKB held · 400 CKB occupied (100.0%) · H-mult. ×1.00`
    );
  });
});

// Table-rows round, lane L2-3: the identity systems on the same rules — the name bright on one
// line (the standard a tag when the name differs), counts, CKB held as a rose 8 px bar with its 2-dp
// amount, occupied in whole CKB; every digit in the row title.
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
      'leading-[18px]',
      'truncate'
    );
    expect(first.querySelector('[data-row-tag]')).toBeNull();
    expect(within(second).getByText('.bit Cell')).toHaveAttribute('data-row-tag', 'default');
    expect(first.querySelector('svg[data-class="identities"]')).not.toBeNull();
  });

  it('draws CKB held as a rose bar with a 2-dp amount and keeps occupied on one line', () => {
    const table = mount();
    const scale = resourceScale([100, 400]);
    expect(
      cellUnder(table, 2, 'CKB held in its cells').querySelector('[data-segment]')
    ).toHaveStyle({ width: barWidth(400, scale), backgroundColor: CLASS_COLORS.identities });
    expect(cellUnder(table, 1, 'CKB held in its cells')).toHaveTextContent('100.00');
    expect(cellUnder(table, 1, 'Occupied')).toHaveTextContent(/^61$/);
    const occupied = cellUnder(table, 2, 'Occupied');
    expect(occupied).toHaveTextContent('133.0000005');
    expect(occupied.querySelector('[data-ckb-amount]')).toHaveClass('whitespace-nowrap');
    expect(cellUnder(table, 2, 'Ever registered')).toHaveTextContent('656');
  });

  it('keeps every digit in the row title', () => {
    const table = mount();
    expect(within(table).getAllByRole('row')[2]).toHaveAttribute(
      'title',
      'Cellula .bit · .bit Cell · 541 live · 656 ever registered · 366 holders · 400 CKB held · 133.0000005 CKB occupied (33.3%)'
    );
  });
});

// Table-rows L2 follow-up: a row's 2-dp amount is the exact amount CUT at 2 places, never rounded
// (a rounded figure can show a digit that is not on chain); the exact value is in the title.
describe('2-dp row amounts truncate, never round (table-rows L2)', () => {
  it('cuts 1,234.56789 to 1,234.56 for CKB and token amounts, the exact value in the title', () => {
    const { container, rerender } = render(<CkbTwoPlaces shannons={BigInt('123456789000')} />);
    expect(container).toHaveTextContent(/^1,234\.56$/);
    rerender(<TokenTwoPlaces amount="123456789" decimals={5} />);
    expect(container).toHaveTextContent(/^1,234\.56$/);
    expect(container.firstElementChild).toHaveAttribute('title', '1,234.56789');
  });
  it('never carries .999 into the next whole unit', () => {
    const { container, rerender } = render(<CkbTwoPlaces shannons={BigInt('123499900000')} />);
    expect(container).toHaveTextContent(/^1,234\.99$/);
    rerender(<TokenTwoPlaces amount="99999999" decimals={8} />);
    expect(container).toHaveTextContent(/^0\.99$/);
    rerender(<TokenTwoPlaces amount="1999" decimals={3} />);
    expect(container).toHaveTextContent(/^1\.99$/);
    // A unit with fewer than 2 decimals prints what it has.
    rerender(<TokenTwoPlaces amount="12349" decimals={1} />);
    expect(container).toHaveTextContent(/^1,234\.9$/);
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

  it('writes a token in three lines at 358 px: name and holders · standard, transfers, ×H · supply and CKB', () => {
    atContainerWidth(358);
    render(<InventoryRows assetType="token" rows={[asset]} />);
    expect(screen.queryByRole('table')).toBeNull();
    const [row] = screen.getAllByTestId('inventory-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.tone, l.left, l.right])).toEqual([
      ['identity', 'head', 'T Token', '2 holders'],
      ['activity', 'body', 'xUDT · 4 transfers · 0 in 24 h', '×1.6393442625'],
      [
        'figures',
        'quiet',
        // Every digit of the supply, the CKB held and occupied with its exact share.
        'supply 1.000000000000000000000001 T · 100.00000001 CKB held · 61 occupied (61.0%)',
        null,
      ],
    ]);
    expect(row.querySelector('.truncate')).toBeNull();
  });

  it('writes a token in two lines at 608 px, the standard beside the name', () => {
    atContainerWidth(608);
    render(<InventoryRows assetType="token" rows={[asset]} />);
    const [row] = screen.getAllByTestId('inventory-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', 'T Token · xUDT', '2 holders'],
      [
        'figures',
        'supply 1.000000000000000000000001 T · 100.00000001 CKB held · 61 occupied (61.0%)',
        '4 transfers · 0 in 24 h · ×1.6393442625',
      ],
    ]);
  });

  it('writes a collection in three lines with the composition mark (board L4)', () => {
    atContainerWidth(358);
    render(<InventoryRows assetType="object" rows={[spore]} />);
    const [row] = screen.getAllByTestId('inventory-row-mobile');
    const lines = phoneLines(row);
    expect(lines.map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', 'Wyltek Founding Members', '93 objects'],
      ['activity', 'Spore · 32 holders · 95 activities', 'Pure CKB'],
      [
        'figures',
        expect.stringMatching(
          /^of 93 minted · 100\.00000001 CKB held · 61 occupied \(61\.0%\) · ×1\.6393442625 · 0xabab/
        ),
        null,
      ],
    ]);
    expect(row.querySelector('[data-composition="pure_ckb"]')).not.toBeNull();
  });

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
      ['identity', '.bit', '5 live'],
      ['activity', '9 ever registered · 2 holders', null],
      ['figures', '100.00000001 CKB held · 61 occupied (61.0%)', null],
    ]);
  });
});
