import { render, screen, fireEvent, within } from '@/__tests__/utils/test-utils';
import { CapacityHistory } from '@/components/asset/capacity-history';
import type { AssetCapacityHistory } from '@/lib/api';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { clusterCapacityHistoryProducer } from '@/__tests__/msw/fixtures/item-producer';
import { yTickLabels } from '@/__tests__/utils/y-ticks';
const history: AssetCapacityHistory = {
  source: 'indexedChainTimeUtc8',
  completeThrough: '2023-11-14',
  anchor: { blockNumber: 999, blockHash: '0x' + '11'.repeat(32), chainTime: '1700000000000' },
  title: 'Capacity',
  series: [],
  data: Array.from({ length: 14 }, (_, i) => ({
    date: `2023-11-${String(i + 1).padStart(2, '0')}`,
    values: {
      used: '6100000000',
      unused: String(BigInt(3900000000) + BigInt(i)),
      // the whole the day's cells hold, as the route states it (lane p-assets)
      capacity: String(BigInt(10000000000) + BigInt(i)),
    },
  })),
};
it('preserves single shannons, complete-day cutoff and shared capacity/transfer range', () => {
  render(
    <CapacityHistory
      assetClass="objects"
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
  // re-pinned (lane c2-assets): the range is the controls board's RangeSelect, a radio group whose
  // spans are spelled "1 w" (was a group of pressed buttons, "1W")
  const range = screen.getByRole('radiogroup', { name: 'History time range' });
  fireEvent.click(within(range).getByRole('radio', { name: '1 w' }));
  expect(within(range).getByRole('radio', { name: '1 w' })).toHaveAttribute('aria-checked', 'true');
  expect(screen.getByText('No data in this range.')).toBeInTheDocument();
  expect(readout).toHaveTextContent('100.00000013');
});
it('rejects an incomplete calendar day and missing exact occupied state', () => {
  const log = vi.spyOn(console, 'error').mockImplementation(() => {});
  expect(() =>
    render(
      <CapacityHistory
        assetClass="objects"
        subject="Broken"
        history={{ ...history, completeThrough: '2023-11-13' }}
        weekly={[]}
      />
    )
  ).toThrow('incomplete day');
  expect(() =>
    render(
      <CapacityHistory
        assetClass="objects"
        subject="Broken"
        // re-pinned on purpose (lane p-assets): the day states its capacity, so its missing
        // occupied part is the fault read (it read occupied first, before capacity was sent)
        history={{
          ...history,
          data: [{ date: '2023-11-01', values: { unused: '1', capacity: '1' } }],
        }}
        weekly={[]}
      />
    )
  ).toThrow('occupied');
  // a day without the whole its cells hold, which the route states, fails too: the page sums
  // nothing in its place
  expect(() =>
    render(
      <CapacityHistory
        assetClass="objects"
        subject="Broken"
        history={{ ...history, data: [{ date: '2023-11-01', values: { used: '1', unused: '1' } }] }}
        weekly={[]}
      />
    )
  ).toThrow('Broken 2023-11-01: capacity');
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
          assetClass="objects"
          subject="Producer collection"
          history={clusterCapacityHistoryProducer}
          weekly={[]}
        />
      );
      const plot = screen.getByRole('img', { name: 'CKB held in its cells, CKB' });
      // Re-pinned with U-19 (UI review 2026-11-14): the round values are dim labels in the gutter
      // left of the plot, the baseline's 0 among them (it was left out inside the plot, where it
      // would sit on the data). No value is written twice.
      expect(plot.querySelectorAll('[data-role="y-tick"]')).toHaveLength(0);
      const ticks = yTickLabels(plot).map((t) => t.textContent);
      // and, since the lane A5 follow-up, bare: the plot's name says CKB once
      expect(ticks).toContain('0.0000005');
      expect(ticks).toContain('0');
      expect(plot.querySelector('[data-role="chart-name"]')).toHaveTextContent('CKB');
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
      assetClass="objects"
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
