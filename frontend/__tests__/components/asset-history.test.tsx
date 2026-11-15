import { render, screen, fireEvent, within, cleanup } from '@/__tests__/utils/test-utils';
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
      // Re-pinned with the plot-name fit: the fixed 'supply in tokens' (a symbol has no length
      // bound and SVG text does not wrap).
      expect(plot.querySelector('[data-role="chart-name"]')).toHaveTextContent('supply in tokens');
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
        // PB-23: the readout is one separated list; its dots are not swatches
        ...screen
          .getByLabelText('History readout')
          .querySelectorAll<HTMLElement>('[aria-hidden]:not([data-dotted-separator])'),
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
        /^2024-01-08 · held 200\.00000001 CKB · occupied 61\.00000000 CKB 30\.50% · free 139\.00000001 CKB · Latest complete week, from 2024-01-01: 3 transfers · peak 200\.00000001 CKB on 2024-01-08$/
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
    // re-pinned (A4 round 2): the date is a unit (a span of its own) inside the titled reading of
    // the day, and the week's line names the same date as a unit too, so the reading is found by
    // its title (it was found by its text, then its title read)
    expect(
      within(readout).getByTitle('complete UTC+8 days through 2024-01-08 · indexed chain time')
    ).toHaveTextContent(/^2024-01-08$/);
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
              lockCodeHash: null,
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
                lockCodeHash: `0x${'9b'.repeat(32)}`,
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
        // U-09 (re-pinned): the phone row's lock hash at the tight size
        ['evidence', '#10 · tx 0 · sequence 2', 'from 0xbbbb…bbbb JoyID'],
      ]);
    } finally {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    }
  });
  // U-15 / lane A4b (UI review 2026-11-14): a mint's or burn's party whose lock the scripts registry
  // does not name read "Lock kind unavailable"; the event now carries the lock's code hash, and the
  // party is named by it, "lock 0x43434343…434343", linked to that code hash's script page. A
  // registered lock reads as before; a lock not indexed shows no kind. U-09 (re-pinned): the code
  // hash takes the size of the form it is drawn in, row in the table and tight in the phone row.
  describe('names a supply-event party’s unregistered lock by its code hash (U-15)', () => {
    const unregistered = `0x${'43'.repeat(32)}`;
    const scriptPage = expect.stringMatching(new RegExp(`/script/${unregistered}$`));
    const event = (
      i: number,
      kind: 'mint' | 'burn',
      lockCodeHash: string | null,
      lockKind: string | null
    ) => ({
      txHash: `0x${String(i).repeat(64)}`,
      blockNumber: 10 + i,
      txIndex: 0,
      transferSequence: 0,
      timestamp: String(Date.parse('2024-01-01T16:30:00Z')),
      kind,
      amount: '1',
      supplyAfter: '13',
      lockHash: `0x${String(i).repeat(64)}`,
      address:
        lockCodeHash === null
          ? null
          : `ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq${String(i).repeat(8)}`,
      lockCodeHash,
      lockKind,
    });
    const rows = [
      event(1, 'mint', unregistered, null),
      event(2, 'burn', `0x${'9b'.repeat(32)}`, 'JoyID'),
      event(3, 'mint', null, null),
    ];
    const mount = () => {
      render(
        <AssetHistory
          assetClass="tokens"
          weeklyNoun="transfers"
          daily={daily}
          weekly={[]}
          completeThrough="2024-01-08"
          token={{ symbol: 'T', decimals: 0 }}
          supplyEvents={{ total: 3, rows }}
        />
      );
      fireEvent.click(screen.getByText('Every change to the supply (3)'));
    };

    it('in the table', () => {
      mount();
      const table = screen.getByRole('table', { name: 'Mints and burns' });
      const [, unnamed, named, notIndexed] = within(table).getAllByRole('row');
      expect(unnamed.querySelector('[data-lock-kind]')).toHaveTextContent(
        /^lock 0x43434343…434343$/
      );
      expect(within(unnamed).getByRole('link', { name: '0x43434343…434343' })).toHaveAttribute(
        'href',
        scriptPage
      );
      expect(within(named).getByText('JoyID')).toHaveClass('text-text-dim', 'text-meta', 'block');
      expect(notIndexed.querySelector('[data-lock-kind]')).toBeNull();
      expect(table).not.toHaveTextContent(/lock kind unavailable/i);
    });

    it('in the phone row at 358 px', () => {
      atContainerWidth(358);
      try {
        mount();
        const [unnamed, named, notIndexed] = screen.getAllByTestId('supply-event-row-mobile');
        expect(phoneLines(unnamed)[2].right).toMatch(/^to ckb1…111111 lock 0x4343…4343$/);
        expect(within(unnamed).getByRole('link', { name: '0x4343…4343' })).toHaveAttribute(
          'href',
          scriptPage
        );
        expect(phoneLines(named)[2].right).toMatch(/^from ckb1…222222 JoyID$/);
        // A4 Chrome sweep (c): the party is not one nowrap run around its lock name (words); its
        // address is the unit, the name may break
        const party = within(named).getByText('JoyID').closest('span:not([data-lock-kind])');
        expect(party).not.toHaveClass('whitespace-nowrap');
        expect(within(named).getByText('JoyID').closest('.whitespace-nowrap')).toBeNull();
        expect(notIndexed.querySelector('[data-lock-kind]')).toBeNull();
        for (const row of [unnamed, named, notIndexed])
          expect(row).not.toHaveTextContent(/lock kind unavailable/i);
      } finally {
        vi.restoreAllMocks();
        vi.unstubAllGlobals();
      }
    });
  });
  // A4b review: a party whose lock has no address form (args longer than an address can carry)
  // comes with a null address and its lock's code hash and kind: it is named by its lock hash,
  // linked to the address page that takes a lock hash, and by its lock kind.
  it('names a supply-event party whose lock has no address form by its lock hash and kind (A4b review)', () => {
    const lockHash = `0x${'88'.repeat(32)}`;
    const addressPage = expect.stringMatching(new RegExp(`/address/${lockHash}$`));
    const mount = () => {
      render(
        <AssetHistory
          assetClass="tokens"
          weeklyNoun="transfers"
          daily={daily}
          weekly={[]}
          completeThrough="2024-01-08"
          token={{ symbol: 'T', decimals: 0 }}
          supplyEvents={{
            total: 1,
            rows: [
              {
                txHash: `0x${'aa'.repeat(32)}`,
                blockNumber: 10,
                txIndex: 0,
                transferSequence: 0,
                timestamp: String(Date.parse('2024-01-01T16:30:00Z')),
                kind: 'mint',
                amount: '1',
                supplyAfter: '13',
                lockHash,
                address: null,
                lockCodeHash: `0x${'9b'.repeat(32)}`,
                lockKind: 'SECP256K1/blake160',
              },
            ],
          }}
        />
      );
      fireEvent.click(screen.getByText('Every change to the supply (1)'));
    };
    mount();
    const [, row] = within(screen.getByRole('table', { name: 'Mints and burns' })).getAllByRole(
      'row'
    );
    expect(within(row).getByRole('link', { name: /^0x8888/ })).toHaveAttribute('href', addressPage);
    expect(row.querySelector('[data-lock-kind]')).toHaveTextContent('SECP256K1/blake160');
    cleanup();
    atContainerWidth(358);
    try {
      mount();
      const [phone] = screen.getAllByTestId('supply-event-row-mobile');
      expect(within(phone).getByRole('link', { name: /^0x8888/ })).toHaveAttribute(
        'href',
        addressPage
      );
      expect(phoneLines(phone)[2].right).toMatch(/^to 0x8888.* SECP256K1\/blake160$/);
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
              lockCodeHash: null,
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
