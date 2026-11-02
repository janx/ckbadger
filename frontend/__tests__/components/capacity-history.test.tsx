import { render, screen, fireEvent, within } from '@/__tests__/utils/test-utils';
import { CapacityHistory } from '@/components/asset/capacity-history';
import type { AssetCapacityHistory } from '@/lib/api';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { clusterCapacityHistoryProducer } from '@/__tests__/msw/fixtures/item-producer';
const history: AssetCapacityHistory = {
  source: 'indexedChainTimeUtc8',
  completeThrough: '2023-11-14',
  anchor: { blockNumber: 999, blockHash: '0x' + '11'.repeat(32), chainTime: '1700000000000' },
  title: 'Capacity',
  series: [],
  data: Array.from({ length: 14 }, (_, i) => ({
    date: `2023-11-${String(i + 1).padStart(2, '0')}`,
    values: { used: '6100000000', unused: String(BigInt(3900000000) + BigInt(i)) },
  })),
};
it('preserves single shannons, complete-day cutoff and shared capacity/transfer range', () => {
  render(
    <CapacityHistory
      subject="Collection"
      history={history}
      weekly={[
        {
          weekStart: '2023-11-06',
          mintTransactions: 2,
          transferTransactions: 7,
          burnTransactions: 1,
        },
      ]}
    />
  );
  const readout = screen.getByLabelText('History readout');
  expect(readout).toHaveTextContent('100.00000013');
  expect(readout).toHaveTextContent('39.00000013');
  // Lane P: the complete-day scope is the hover of the reading's day.
  expect(readout.querySelector('[title^="complete UTC+8 days through 2023-11-14"]')).not.toBeNull();
  const range = screen.getByRole('group', { name: 'History time range' });
  fireEvent.click(within(range).getByRole('button', { name: '1W' }));
  expect(within(range).getByRole('button', { name: '1W' })).toHaveAttribute('aria-pressed', 'true');
  expect(screen.getByText('No data in this range.')).toBeInTheDocument();
  expect(readout).toHaveTextContent('100.00000013');
});
it('rejects an incomplete calendar day and missing exact occupied state', () => {
  const log = vi.spyOn(console, 'error').mockImplementation(() => {});
  expect(() =>
    render(
      <CapacityHistory
        subject="Broken"
        history={{ ...history, completeThrough: '2023-11-13' }}
        weekly={[]}
      />
    )
  ).toThrow('incomplete day');
  expect(() =>
    render(
      <CapacityHistory
        subject="Broken"
        history={{ ...history, data: [{ date: '2023-11-01', values: { unused: '1' } }] }}
        weekly={[]}
      />
    )
  ).toThrow('occupied');
  log.mockRestore();
});

it.each([928, 358])(
  'draws real producer micro-CKB capacity at %spx without duplicate zero ticks or invalid geometry',
  (width) => {
    stubChartWidth(width);
    const errors = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      render(
        <CapacityHistory
          subject="Producer collection"
          history={clusterCapacityHistoryProducer}
          weekly={[]}
        />
      );
      const plot = screen.getByRole('img', { name: 'CKB held in its cells, CKB' });
      // Lane P (R13): the round values are ghost labels inside the plot; the baseline and the
      // top line under the plot's name are not written. No value is written twice.
      const ticks = [...plot.querySelectorAll('[data-role="y-tick"]')].map((t) => t.textContent);
      expect(ticks).toContain('0.0000005 CKB');
      expect(ticks).not.toContain('0 CKB');
      expect(new Set(ticks).size).toBe(ticks.length);
      expect(plot.outerHTML).not.toMatch(/NaN|Infinity/);
      expect(errors).not.toHaveBeenCalled();
      expect(screen.getByLabelText('History readout')).toHaveTextContent('0.00000120');
      fireEvent.keyDown(plot, { key: 'ArrowLeft' });
      expect(screen.getByLabelText('History readout')).toHaveTextContent('0.00000100');
    } finally {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    }
  }
);

it('says why a collection has no weekly transfers instead of drawing an empty chart (OB-K2)', () => {
  render(
    <CapacityHistory
      subject="Identity collection"
      history={history}
      weekly={{ unavailable: 'Weekly transfer totals are not indexed for identity collections' }}
    />
  );
  expect(
    screen.getByText('Weekly transfer totals are not indexed for identity collections')
  ).toBeInTheDocument();
  expect(screen.queryByText('No completed weekly total for this day')).not.toBeInTheDocument();
  expect(screen.queryByRole('img', { name: 'Transfers per week, transfers' })).toBeNull();
});
