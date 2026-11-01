import { render, screen, fireEvent } from '@/__tests__/utils/test-utils';
import { AssetHistory } from '@/components/asset/asset-history';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
const daily = [
  { date: '2024-01-01', capacity: '10000000001', occupiedCapacity: '6100000000', supply: '13' },
  { date: '2024-01-08', capacity: '20000000001', occupiedCapacity: '6100000000', supply: '26' },
];
describe('Asset history', () => {
  it('plots 255-decimal token amounts with scientific ticks and exact keyboard readouts', () => {
    stubChartWidth(358);
    const errors = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      render(
        <AssetHistory
          weeklyNoun="transfers"
          daily={daily.map((day, i) => ({ ...day, supply: String(i + 1) }))}
          weekly={[]}
          completeThrough="2024-01-08"
          token={{ symbol: 'T', decimals: 255 }}
        />
      );
      const plot = screen.getByRole('img', { name: 'Supply, T' });
      expect(plot).toHaveTextContent('1e-255 T');
      expect(plot.outerHTML).not.toMatch(/NaN|Infinity/);
      const readout = screen.getByLabelText('History readout');
      expect(readout).toHaveTextContent(`0.${'0'.repeat(254)}2`);
      fireEvent.keyDown(plot, { key: 'ArrowLeft' });
      expect(readout).toHaveTextContent(`0.${'0'.repeat(254)}1`);
      expect(errors).not.toHaveBeenCalled();
    } finally {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    }
  });

  it('sizes the shared gutter of its aligned charts from their longest tick (responsive B#17)', () => {
    // The token page printed ".0k transactions": a fixed 100 px gutter cut "10k transactions".
    stubChartWidth(358);
    try {
      render(
        <AssetHistory
          weeklyNoun="transactions"
          daily={daily}
          weekly={[{ weekStart: '2024-01-01', count: 8000 }]}
          completeThrough="2024-01-08"
          token={{ symbol: 'RGB++', decimals: 8 }}
        />
      );
      const plots = screen.getAllByRole('img');
      expect(plots).toHaveLength(3);
      // Every tick label starts inside its chart.
      const top = [...plots[2].querySelectorAll('text')].find(
        (n) => n.textContent === '10k transactions'
      )!;
      expect(top.getAttribute('text-anchor')).toBe('end');
      expect(Number(top.getAttribute('x')) - '10k transactions'.length * 6).toBeGreaterThanOrEqual(
        0
      );
      // The three plots still share one left edge, so their days line up.
      const lefts = plots.map((plot) => plot.querySelector('line')!.getAttribute('x1'));
      expect(new Set(lefts).size).toBe(1);
    } finally {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    }
  });

  it('uses jade for occupied capacity and neutral ink for free capacity', () => {
    render(
      <AssetHistory weeklyNoun="transfers" daily={daily} weekly={[]} completeThrough="2024-01-08" />
    );
    expect(screen.getByText('Occupied', { exact: true })).toHaveStyle({ color: '#2edba3' });
    expect(screen.getByText('Free', { exact: true })).toHaveStyle({ color: '#3a4258' });
  });
  it('keeps exact tier-one readouts, complete-day scope and the actual peak date', () => {
    render(
      <AssetHistory
        weeklyNoun="transfers"
        daily={daily}
        weekly={[{ weekStart: '2024-01-01', count: 3 }]}
        completeThrough="2024-01-08"
        token={{ symbol: 'T', decimals: 8 }}
        supplyEvents={{ rows: [], total: 0 }}
      />
    );
    const readout = screen.getByLabelText('History readout');
    expect(readout).toHaveTextContent('200.00000001');
    expect(readout).toHaveTextContent('0.00000026');
    // F2 objects (OB-C6, visual pass): before any hover, today's week is incomplete, so the
    // reading names the latest complete week instead of "No completed weekly total".
    expect(readout).toHaveTextContent('Latest complete week, from 2024-01-01: 3 transfers');
    expect(screen.getByText(/CKB held peaked/)).toHaveTextContent('on 2024-01-08');
    expect(screen.getByText(/Complete UTC\+8 days through 2024-01-08/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '1W' }));
    expect(readout).toHaveTextContent('2024-01-08');
  });
  it('labels supply events in UTC+8 and distinguishes raw block dates from indexed chart days', () => {
    render(
      <AssetHistory
        weeklyNoun="transfers"
        daily={daily}
        weekly={[]}
        completeThrough="2024-01-08"
        token={{ symbol: 'T', decimals: 8 }}
        supplyEvents={{
          total: 1,
          rows: [
            {
              txHash: '0xaa',
              blockNumber: 10,
              txIndex: 0,
              transferSequence: 0,
              timestamp: String(Date.parse('2024-01-01T16:30:00Z')),
              kind: 'mint',
              amount: '1',
              supplyAfter: '13',
              lockHash: '0xbb',
              address: null,
              lockKind: null,
            },
          ],
        }}
      />
    );
    fireEvent.click(screen.getByText('Every change to the supply (1)'));
    expect(screen.getByRole('table', { name: 'Mints and burns' })).toHaveTextContent('2024-01-02');
    expect(screen.getByText(/Event dates use raw block timestamps/)).toHaveTextContent('UTC+8');
  });
  it('writes a supply change as a phone row at 358 px: change and date · supply after · where and to whom', () => {
    atContainerWidth(358);
    try {
      render(
        <AssetHistory
          weeklyNoun="transfers"
          daily={daily}
          weekly={[]}
          completeThrough="2024-01-08"
          token={{ symbol: 'T', decimals: 8 }}
          supplyEvents={{
            total: 1,
            rows: [
              {
                txHash: '0xaa',
                blockNumber: 10,
                txIndex: 0,
                transferSequence: 2,
                timestamp: String(Date.parse('2024-01-01T16:30:00Z')),
                kind: 'burn',
                amount: '100000000',
                supplyAfter: '1200000000',
                lockHash: `0x${'bb'.repeat(32)}`,
                address: null,
                lockKind: 'JoyID',
              },
            ],
          }}
        />
      );
      fireEvent.click(screen.getByText('Every change to the supply (1)'));
      expect(screen.queryByRole('table', { name: 'Mints and burns' })).toBeNull();
      const [row] = screen.getAllByTestId('supply-event-row-mobile');
      expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
        ['change', 'Burn −1.00000000', '2024-01-02'],
        ['after', 'supply after 12.00000000', null],
        ['evidence', '#10 · tx 0 · sequence 2', 'from 0xbbbbbb...bbbb JoyID'],
      ]);
    } finally {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    }
  });
  it('names what the weekly totals count and says when only the newest supply events are listed', () => {
    stubChartWidth(358);
    render(
      <AssetHistory
        weeklyNoun="transactions"
        daily={daily}
        weekly={[{ weekStart: '2024-01-01', count: 3 }]}
        completeThrough="2024-01-14"
        token={{ symbol: 'T', decimals: 8 }}
        supplyEvents={{
          total: 1234,
          rows: [
            {
              txHash: '0xaa',
              blockNumber: 10,
              txIndex: 0,
              transferSequence: 0,
              timestamp: String(Date.parse('2024-01-01T16:30:00Z')),
              kind: 'mint',
              amount: '1',
              supplyAfter: '13',
              lockHash: '0xbb',
              address: null,
              lockKind: null,
            },
          ],
        }}
      />
    );
    expect(
      screen.getByRole('img', { name: 'Transactions per week, transactions' })
    ).toBeInTheDocument();
    expect(screen.queryByRole('img', { name: /Transfers/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('Every change to the supply (1,234)'));
    expect(screen.getByText('The newest 1 are listed.')).toBeInTheDocument();
    vi.unstubAllGlobals();
  });
  it('rejects invalid day labels and incomplete weekly buckets', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <AssetHistory weeklyNoun="transfers" daily={[]} weekly={[]} completeThrough="2024-02-31" />
      )
    ).toThrow('Invalid indexed history date');
    expect(() =>
      render(
        <AssetHistory
          weeklyNoun="transfers"
          daily={daily}
          weekly={[{ weekStart: '2024-01-08', count: 0 }]}
          completeThrough="2024-01-08"
        />
      )
    ).toThrow('incomplete weekly transfer');
    error.mockRestore();
  });
  it('rejects incomplete-day values and mismatched partitions instead of repairing them', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <AssetHistory
          weeklyNoun="transfers"
          daily={daily}
          weekly={[]}
          completeThrough="2024-01-07"
        />
      )
    ).toThrow('incomplete day');
    expect(() =>
      render(
        <AssetHistory
          weeklyNoun="transfers"
          daily={[{ ...daily[0], occupiedCapacity: '10000000002' }]}
          weekly={[]}
          completeThrough="2024-01-08"
        />
      )
    ).toThrow('occupied exceeds held capacity');
    error.mockRestore();
  });
});
