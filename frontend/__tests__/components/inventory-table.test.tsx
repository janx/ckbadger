import { afterEach, vi } from 'vitest';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { InventoryRows, AssetAmount } from '@/components/asset/inventory-table';
import type { Asset } from '@/lib/api';
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
describe('Inventory rows', () => {
  it('preserves a long H-multiplier with a column that may wrap instead of overflow', () => {
    render(<InventoryRows assetType="token" rows={[asset]} />);
    const value = within(screen.getByRole('table', { name: 'Tokens' })).getByText('×1.6393442625');
    expect(value.closest('td')).toHaveClass('!whitespace-normal', 'break-all');
  });
  it('keeps every token digit and uses the canonical standard name', () => {
    render(<InventoryRows assetType="token" rows={[asset]} />);
    const table = screen.getByRole('table', { name: 'Tokens' });
    expect(table).toHaveTextContent('1.000000000000000000000001');
    expect(table).toHaveTextContent('100.00000001');
    // F2 objects (IV-04): the standard sits under the full name, after the symbol.
    expect(within(table).getByText(/xUDT$/)).toBeInTheDocument();
    expect(table.querySelector('.truncate')).toBeNull();
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
    expect(table).toHaveTextContent('of 7 minted');
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
