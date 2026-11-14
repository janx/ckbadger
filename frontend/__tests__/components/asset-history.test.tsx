import { render, screen, fireEvent, within } from '@/__tests__/utils/test-utils';
import { AssetHistory } from '@/components/asset/asset-history';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { yTickLabels, plotLeftOf } from '@/__tests__/utils/y-ticks';
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
          assetClass="tokens"
          weeklyNoun="transfers"
          daily={daily.map((day, i) => ({ ...day, supply: String(i + 1) }))}
          weekly={[]}
          completeThrough="2024-01-08"
          token={{ symbol: 'T', decimals: 255 }}
        />
      );
      const plot = screen.getByRole('img', { name: 'Supply, T' });
      // Re-pinned with the lane A5 follow-up (U-19): the values are bare in the gutter, still
      // scientific, and the plot's name says the unit once.
      expect(plot).toHaveTextContent(/\de-255/);
      expect(plot).not.toHaveTextContent(/e-255 T/);
      expect(plot.querySelector('[data-role="chart-name"]')).toHaveTextContent('supply in T');
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
          assetClass="tokens"
          weeklyNoun="transactions"
          daily={daily}
          weekly={[{ weekStart: '2024-01-01', count: 8000 }]}
          completeThrough="2024-01-08"
          token={{ symbol: 'RGB++', decimals: 8 }}
        />
      );
      const plots = screen.getAllByRole('img');
      expect(plots).toHaveLength(3);
      // Re-pinned with U-19 (UI review 2026-11-14) and its lane A5 follow-up: the ticks are bare
      // values in one gutter sized from the longest of the three plots' labels (the unit is said
      // once, in each plot's name), never under the 48 px least.
      const ticks = yTickLabels(plots[2]);
      expect(ticks.map((t) => t.textContent)).toContain('5k');
      const left = plotLeftOf(plots[2]);
      const longest = Math.max(
        ...plots.flatMap((plot) => yTickLabels(plot).map((t) => t.textContent!.length))
      );
      expect(left).toBe(Math.max(48, longest * 6 + 8));
      for (const tick of ticks) {
        expect(tick.getAttribute('text-anchor')).toBe('end');
        expect(Number(tick.getAttribute('x'))).toBe(left - 8);
      }
      // The three plots still share one left edge, so their days line up.
      expect(new Set(plots.map(plotLeftOf)).size).toBe(1);
    } finally {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    }
  });

  it('uses jade for occupied capacity and neutral ink for free capacity', () => {
    stubChartWidth(1248);
    try {
      render(
        <AssetHistory
          assetClass="objects"
          weeklyNoun="transfers"
          daily={daily}
          weekly={[]}
          completeThrough="2024-01-08"
        />
      );
      // Lane P: the plot names its inks inside it (Main.dc.html); the readout keys them.
      // Re-pinned with U-19: a shorter name, so it fits beside the value gutter at 360 px.
      expect(screen.getByRole('img', { name: 'CKB held in its cells, CKB' })).toHaveTextContent(
        'CKB held · occupied, free'
      );
      const swatches = [
        ...screen.getByLabelText('History readout').querySelectorAll<HTMLElement>('[aria-hidden]'),
      ];
      expect(swatches.map((s) => s.style.backgroundColor)).toEqual([
        'var(--c-jade)',
        'var(--c-slate-ckb)',
      ]);
    } finally {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    }
  });

  // Lane P (Main.dc.html "the readout row"): one row under the plots — the day, held, occupied
  // with its share, free, then the week and the peak — and what occupied and free mean is their
  // hover, not two sentences under the chart.
  it('reads the day in one row under the plots, as the board words it', () => {
    stubChartWidth(1248);
    try {
      const { container } = render(
        <AssetHistory
          assetClass="objects"
          weeklyNoun="transfers"
          daily={daily}
          weekly={[{ weekStart: '2024-01-01', count: 3 }]}
          completeThrough="2024-01-08"
        />
      );
      const readout = screen.getByLabelText('History readout');
      const plots = [...container.querySelectorAll('svg[role="img"]')];
      expect(
        plots.at(-1)!.compareDocumentPosition(readout) & Node.DOCUMENT_POSITION_FOLLOWING
      ).toBeTruthy();
      expect(readout).toHaveClass('border-t');
      expect(readout).toHaveTextContent(
        /^2024-01-08 · held 200\.00000001 CKB · occupied 61\.00000000 CKB 30\.50 % · free 139\.00000001 CKB · Latest complete week, from 2024-01-01: 3 transfers · peak 200\.00000001 CKB on 2024-01-08$/
      );
      expect(within(readout).getByText('occupied')).toHaveAttribute(
        'title',
        'Occupied pays for the state kept on CKB.'
      );
      expect(within(readout).getByText('free')).toHaveAttribute(
        'title',
        'Free is CKB held in the same cells beyond that byte cost.'
      );
      expect(screen.queryByText(/Occupied pays for the state/)).toBeNull();
    } finally {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    }
  });

  // Lane P (R13, Main.dc.html, measured ~150 px by lane A): the main history plot is the page's
  // picture, at least 280 px tall, its name and round values written inside it in ghost ink.
  // Re-pinned with U-19 (UI review 2026-11-14): the values leave the plot for a dim gutter (they
  // were ghost values written inside it, under the line and behind the bars); the name stays
  // inside the plot, after the gutter, short enough to fit a 360 px phone.
  it('draws the main plot 280 px tall with its name inside it and its values in the gutter', () => {
    stubChartWidth(1248);
    try {
      for (const token of [undefined, { symbol: 'T', decimals: 0 }]) {
        const { unmount } = render(
          <AssetHistory
            assetClass={token ? 'tokens' : 'objects'}
            weeklyNoun="transfers"
            daily={daily}
            weekly={[{ weekStart: '2024-01-01', count: 400 }]}
            completeThrough="2024-01-08"
            token={token}
          />
        );
        const main = screen.getByRole('img', { name: 'CKB held in its cells, CKB' });
        expect(Number(main.getAttribute('height'))).toBeGreaterThanOrEqual(280);
        const name = main.querySelector('[data-role="chart-name"]')!;
        expect(name).toHaveTextContent('CKB held · occupied, free');
        const left = plotLeftOf(main);
        expect(Number(name.getAttribute('x'))).toBe(left + 8);
        for (const plot of screen.getAllByRole('img')) {
          expect(plot.querySelectorAll('[data-role="y-tick"]')).toHaveLength(0);
          const ticks = yTickLabels(plot);
          expect(ticks.length).toBeGreaterThan(0);
          for (const tick of ticks) expect(tick).toHaveAttribute('fill', 'var(--c-text-dim)');
          expect(plotLeftOf(plot)).toBe(left);
        }
        unmount();
      }
    } finally {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    }
  });
  it('keeps exact tier-one readouts, complete-day scope and the actual peak date', () => {
    render(
      <AssetHistory
        assetClass="tokens"
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
    // Lane P (Main.dc.html): the peak is the readout row's last item, not a sentence of its own.
    expect(readout).toHaveTextContent(/· peak 200\.00000001 CKB on 2024-01-08$/);
    expect(screen.queryByText(/CKB held peaked/)).toBeNull();
    // Lane P (Main.dc.html): the scope is the hover of the reading's day, not a line of its own.
    expect(screen.queryByText(/Complete UTC\+8 days through/)).toBeNull();
    expect(within(readout).getByText('2024-01-08')).toHaveAttribute(
      'title',
      'complete UTC+8 days through 2024-01-08 · indexed chain time'
    );
    fireEvent.click(screen.getByRole('button', { name: '1W' }));
    expect(readout).toHaveTextContent('2024-01-08');
  });
  it('labels supply events in UTC+8 and distinguishes raw block dates from indexed chart days', () => {
    render(
      <AssetHistory
        assetClass="tokens"
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
          assetClass="tokens"
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
        assetClass="tokens"
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
        <AssetHistory
          assetClass="objects"
          weeklyNoun="transfers"
          daily={[]}
          weekly={[]}
          completeThrough="2024-02-31"
        />
      )
    ).toThrow('Invalid indexed history date');
    expect(() =>
      render(
        <AssetHistory
          assetClass="objects"
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
          assetClass="objects"
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
          assetClass="objects"
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
