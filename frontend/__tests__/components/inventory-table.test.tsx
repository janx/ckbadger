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
