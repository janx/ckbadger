import { render, screen, fireEvent } from '@/__tests__/utils/test-utils';
import { AssetHistory } from '@/components/asset/asset-history';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
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

  it('uses jade for occupied capacity and neutral ink for free capacity', () => {
    render(<AssetHistory daily={daily} weekly={[]} completeThrough="2024-01-08" />);
    expect(screen.getByText('Occupied', { exact: true })).toHaveStyle({ color: '#2edba3' });
    expect(screen.getByText('Free', { exact: true })).toHaveStyle({ color: '#3a4258' });
  });
  it('keeps exact tier-one readouts, complete-day scope and the actual peak date', () => {
    render(
      <AssetHistory
        daily={daily}
        weekly={[{ weekStart: '2024-01-01', transfersCount: 3 }]}
        completeThrough="2024-01-08"
        token={{ symbol: 'T', decimals: 8 }}
        supplyEvents={[]}
      />
    );
    const readout = screen.getByLabelText('History readout');
    expect(readout).toHaveTextContent('200.00000001');
    expect(readout).toHaveTextContent('0.00000026');
    expect(readout).toHaveTextContent('No completed weekly total for this day');
    expect(screen.getByText(/CKB held peaked/)).toHaveTextContent('on 2024-01-08');
    expect(screen.getByText(/Complete UTC\+8 days through 2024-01-08/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '1W' }));
    expect(readout).toHaveTextContent('2024-01-08');
  });
  it('labels supply events in UTC+8 and distinguishes raw block dates from indexed chart days', () => {
    render(
      <AssetHistory
        daily={daily}
        weekly={[]}
        completeThrough="2024-01-08"
        token={{ symbol: 'T', decimals: 8 }}
        supplyEvents={[
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
        ]}
      />
    );
    fireEvent.click(screen.getByText('Every change to the supply (1)'));
    expect(screen.getByRole('table', { name: 'Mints and burns' })).toHaveTextContent('2024-01-02');
    expect(screen.getByText(/Event dates use raw block timestamps/)).toHaveTextContent('UTC+8');
  });
  it('rejects invalid day labels and incomplete weekly buckets', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(<AssetHistory daily={[]} weekly={[]} completeThrough="2024-02-31" />)
    ).toThrow('Invalid indexed history date');
    expect(() =>
      render(
        <AssetHistory
          daily={daily}
          weekly={[{ weekStart: '2024-01-08', transfersCount: 0 }]}
          completeThrough="2024-01-08"
        />
      )
    ).toThrow('incomplete weekly transfer');
    error.mockRestore();
  });
  it('rejects incomplete-day values and mismatched partitions instead of repairing them', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(<AssetHistory daily={daily} weekly={[]} completeThrough="2024-01-07" />)
    ).toThrow('incomplete day');
    expect(() =>
      render(
        <AssetHistory
          daily={[{ ...daily[0], occupiedCapacity: '10000000002' }]}
          weekly={[]}
          completeThrough="2024-01-08"
        />
      )
    ).toThrow('occupied exceeds held capacity');
    error.mockRestore();
  });
});
