import { render, screen, within } from '@/__tests__/utils/test-utils';
import { TokenDistributionView, TokenHolders } from '@/components/asset/token-holders';
import { api } from '@/lib/api';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
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

describe('Token holders (board L5)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const holders = {
    data: [
      {
        lockScriptHash: `0x${'11'.repeat(32)}`,
        address: `ckb1qrgqep8s${'q'.repeat(40)}tllakf`,
        balance: '56299700000000',
        lockKind: 'JoyID',
      },
      {
        lockScriptHash: `0x${'22'.repeat(32)}`,
        address: null,
        balance: '1000000000000000',
        lockKind: null,
      },
    ],
    total: 2,
    limit: 10,
    hasMore: false,
    nextCursor: null,
    // 562,997 of 24,255,256.49 is 2.32 %.
    totalSupply: '2425525649000000',
    anchor: null,
  };
  const mount = () => {
    vi.spyOn(api, 'getTokenHolders').mockResolvedValue(holders);
    vi.spyOn(api, 'getTokenDistribution').mockRejectedValue(new Error('not needed'));
    render(<TokenHolders typeHash="0xabc" symbol="RGB++" decimals={8} />);
  };

  it('draws the five-column table in the token page’s 554 px column at 1280 and the 608 px band', async () => {
    atContainerWidth(554);
    mount();
    const table = await screen.findByRole('table', { name: 'Top 10 holders' });
    expect(
      within(table)
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual(['#', 'Address', 'Lock', 'RGB++', 'Share']);
    expect(Number.parseFloat(table.style.minWidth)).toBeLessThanOrEqual(608);
    const first = within(table).getAllByRole('row')[1];
    expect(first).toHaveTextContent('JoyID');
    expect(first).toHaveTextContent('2.32%');
    // 10 % fills the bar: 2.32 % is 23.2 % of it; 41.23 % is a full bar.
    expect(first.querySelector('[data-share-bar]')).toHaveAttribute('data-share-bar', '23.21%');
    const second = within(table).getAllByRole('row')[2];
    expect(second.querySelector('[data-share-bar]')).toHaveAttribute('data-share-bar', '100%');
  });

  it('gives each holder two lines at 358 px: rank inline with address, lock and amount · the bar', async () => {
    atContainerWidth(358);
    mount();
    const [first] = await screen.findAllByTestId('holder-row-mobile');
    expect(screen.queryByRole('table', { name: 'Top 10 holders' })).toBeNull();
    expect(phoneLines(first)).toEqual([
      {
        key: 'holder',
        tone: 'head',
        left: '1ckb1qrgqep8s...tllakf JoyID',
        right: '562,997.00000000',
      },
      { key: 'share', tone: 'quiet', left: '', right: '2.32%' },
    ]);
    expect(first.querySelector('[data-role="rank"]')).toHaveClass('w-[22px]');
  });
});

describe('Token holders where the table does not fit (L5)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  it('falls back to the two-line row just below the 548 px table', async () => {
    atContainerWidth(546);
    vi.spyOn(api, 'getTokenHolders').mockResolvedValue({
      data: [
        {
          lockScriptHash: `0x${'11'.repeat(32)}`,
          address: null,
          balance: '100',
          lockKind: 'JoyID',
        },
      ],
      total: 1,
      limit: 10,
      hasMore: false,
      nextCursor: null,
      totalSupply: '1000',
      anchor: null,
    });
    vi.spyOn(api, 'getTokenDistribution').mockRejectedValue(new Error('not needed'));
    render(<TokenHolders typeHash="0xabc" symbol="T" decimals={0} />);
    const [row] = await screen.findAllByTestId('holder-row-mobile');
    expect(screen.queryByRole('table', { name: 'Top 10 holders' })).toBeNull();
    expect(phoneLines(row)).toEqual([
      { key: 'holder', tone: 'head', left: '10x1111111111...111111 JoyID', right: '100' },
      { key: 'share', tone: 'quiet', left: '', right: '10.00%' },
    ]);
    expect(row.querySelector('[data-share-bar]')).toHaveAttribute('data-share-bar', '100%');
  });
});

describe('Holders by balance phone row', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  it('folds a range into two lines where its three columns do not fit', () => {
    atContainerWidth(300);
    render(<TokenDistributionView distribution={distribution} symbol="T" />);
    const rows = screen.getAllByTestId('holder-bucket-row-mobile');
    expect(phoneLines(rows[1]).map((l) => [l.key, l.left, l.right])).toEqual([
      ['range', '1.00000000 and above', '1 holder'],
      ['share', '', '100.00% of supply'],
    ]);
  });
});
