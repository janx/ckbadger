import { render, screen, within } from '@/__tests__/utils/test-utils';
import { TokenDistributionView } from '@/components/asset/token-holders';
import type { TokenDistribution } from '@/lib/api';
const distribution: TokenDistribution = {
  holdersCount: 2,
  totalSupply: '10000000001',
  largestHolderBalance: '10000000000',
  nextNineBalance: '1',
  remainingBalance: '0',
  unit: 'token',
  decimals: 8,
  buckets: [
    { lowerBound: '0', upperBound: '100000000', holdersCount: 1, balance: '1' },
    { lowerBound: '100000000', upperBound: null, holdersCount: 1, balance: '10000000000' },
  ],
};
describe('Token distribution', () => {
  it('uses exact same-response totals and keeps raw bucket boundaries honest', () => {
    const { rerender } = render(<TokenDistributionView distribution={distribution} symbol="T" />);
    expect(screen.getByRole('table', { name: 'Holders by balance' })).toHaveTextContent(
      '1.00000000 and above'
    );
    expect(screen.getByRole('img')).toHaveAccessibleName(/Largest holder 100.00%/);
    rerender(
      <TokenDistributionView
        distribution={{ ...distribution, unit: 'raw', decimals: null }}
        symbol="T"
      />
    );
    expect(screen.getByRole('columnheader', { name: 'Balance · raw' })).toBeInTheDocument();
    expect(screen.getByRole('table', { name: 'Holders by balance' })).toHaveTextContent(
      '100,000,000 (raw)'
    );
  });
  it('keeps each range bound on one line so a number never breaks mid-digits (IV-13)', () => {
    render(<TokenDistributionView distribution={distribution} symbol="T" />);
    const table = screen.getByRole('table', { name: 'Holders by balance' });
    const bound = within(table).getAllByText((_, el) => el?.textContent === '1.00000000')[0];
    expect(bound.closest('.whitespace-nowrap')).not.toBeNull();
  });
  it('rejects missing holders and mismatched aggregate supply', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <TokenDistributionView distribution={{ ...distribution, holdersCount: 3 }} symbol="T" />
      )
    ).toThrow('buckets do not equal totals');
    expect(() =>
      render(
        <TokenDistributionView
          distribution={{ ...distribution, remainingBalance: '1' }}
          symbol="T"
        />
      )
    ).toThrow('concentration does not equal supply');
    error.mockRestore();
  });
});
