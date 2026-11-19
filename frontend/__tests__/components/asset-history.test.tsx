import { lineRuns } from '@/__tests__/utils/line-runs';
import { JOYID_CODE_HASH, seededAddress } from '@/__tests__/utils/ckb-address';
import { render, screen, fireEvent, within, cleanup } from '@/__tests__/utils/test-utils';
import { AssetHistory } from '@/components/asset/asset-history';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { atViewportWidth } from '@/__tests__/utils/viewport';
import { yTickLabels, plotLeftOf } from '@/__tests__/utils/y-ticks';
import { factsOf } from '@/__tests__/utils/facts';
const daily = [
  {
    date: '2024-01-01',
    capacity: '10000000001',
    occupiedCapacity: '6100000000',
    freeCapacity: '3900000001',
    supply: '13',
  },
  {
    date: '2024-01-08',
    capacity: '20000000001',
    occupiedCapacity: '6100000000',
    freeCapacity: '13900000001',
    supply: '26',
  },
];
/** Three days and two complete weeks: a weekly series a chart draws (one week is said in words). */
const dailyOfTwoWeeks = [
  ...daily,
  {
    date: '2024-01-15',
    capacity: '20000000001',
    occupiedCapacity: '6100000000',
    freeCapacity: '13900000001',
    supply: '26',
  },
];
/** A supply event's party address in the fixture below: its seed spelled from its index. */
const eventAddress = (i: number, codeHash: string) => seededAddress(String(i).repeat(6), codeHash);

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
      // re-pinned on purpose (lane P2's third round, item 7): a series of one week is said in words and draws no chart, so the gutter's
      // three plots read a series of two weeks
      render(
        <AssetHistory
          assetClass="tokens"
          weeklyNoun="transactions"
          daily={dailyOfTwoWeeks}
          weekly={[
            { weekStart: '2024-01-01', count: 8000 },
            { weekStart: '2024-01-08', count: 10 },
          ]}
          completeThrough="2024-01-15"
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
      // re-pinned (readability R4): a tick at 12 px is 7.2 px a glyph (was 6 at 10 px)
      expect(left).toBe(Math.max(48, longest * 7.2 + 8));
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
      // re-pinned with readability R4 (lane B7): the reading is labelled facts, the detail pages' dl rows (was one line of facts joined by "·")
      expect(factsOf(readout)).toEqual([
        ['day', '2024-01-08'],
        ['held', '200.00000001 CKB'],
        // re-pinned on purpose (lane P2's follow-up, item 5): the readout's exact form, no padding (it was
        // "61.00000000 CKB")
        // re-pinned for lane exactness, part B2 (one percentage, cut): 61 of 200.00000001 CKB is 30.4999998…%, 30.49% (half up 30.50%)
        ['occupied', '61 CKB 30.49%'],
        ['free', '139.00000001 CKB'],
        ['week', 'Latest complete week, from 2024-01-01: 3 transfers'],
        ['peak', '200.00000001 CKB on 2024-01-08'],
      ]);
      // the name of a part says what it means on hover (it was the word's title in the line)
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
  // Re-pinned on purpose (readability R7, lane p-assets; renamed, it was "draws the main plot 280
  // px tall …"): the plot is 160 px under one sentence its points give
  // (__tests__/components/history-sentence.test.tsx), no longer the hero's 280 px picture.
  it('draws the main plot 160 px tall with its name inside it and its values in the gutter', () => {
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
        expect(Number(main.getAttribute('height'))).toBe(160);
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
    // re-pinned with readability R4 (lane B7): the reading is labelled facts, the detail pages' dl rows (was one line of facts joined by "·")
    expect(factsOf(readout).at(-1)).toEqual(['peak', '200.00000001 CKB on 2024-01-08']);
    expect(screen.queryByText(/CKB held peaked/)).toBeNull();
    // Lane P (Main.dc.html): the scope is the hover of the reading's day, not a line of its own.
    expect(screen.queryByText(/Complete UTC\+8 days through/)).toBeNull();
    // re-pinned (A4 round 2): the date is a unit (a span of its own) inside the titled reading of
    // the day, and the week's line names the same date as a unit too, so the reading is found by
    // its title (it was found by its text, then its title read)
    expect(
      within(readout).getByTitle('complete UTC+8 days through 2024-01-08 · indexed chain time')
    ).toHaveTextContent(/^2024-01-08$/);
    // re-pinned (lane c2-assets): the range is the controls board's RangeSelect, a radio group
    // whose spans are spelled "1 w · 1 m · 1 y · all" (was a button "1W")
    fireEvent.click(screen.getByRole('radio', { name: '1 w' }));
    expect(readout).toHaveTextContent('2024-01-08');
  });
  // Pinned before the controls lane (C2) put the range on RangeSelect: another span drops the day
  // the reader chose, and the reading returns to the newest complete day.
  it('reads the newest day again when another span is chosen', () => {
    stubChartWidth(358);
    try {
      render(
        <AssetHistory
          assetClass="tokens"
          weeklyNoun="transfers"
          daily={daily}
          weekly={[]}
          completeThrough="2024-01-08"
          token={{ symbol: 'T', decimals: 8 }}
        />
      );
      const readout = screen.getByLabelText('History readout');
      const day = () =>
        within(readout).getByTitle('complete UTC+8 days through 2024-01-08 · indexed chain time');
      expect(day()).toHaveTextContent(/^2024-01-08$/);
      fireEvent.keyDown(screen.getByRole('img', { name: 'Supply, T' }), { key: 'ArrowLeft' });
      expect(day()).toHaveTextContent(/^2024-01-01$/);
      // re-pinned (lane c2-assets): the span is a radio of RangeSelect, "1 m" (was a button "1M")
      fireEvent.click(screen.getByRole('radio', { name: '1 m' }));
      expect(day()).toHaveTextContent(/^2024-01-08$/);
    } finally {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    }
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
    atViewportWidth(390);
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
                lockCodeHash: JOYID_CODE_HASH,
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
        // a phone row is the summary form (readability R5, re-pinned on purpose: was padded to 8
        // decimals)
        ['change', 'Burn −1', '2024-01-02'],
        ['after', 'supply after 12', null],
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
    // B4 second fix round (re-pinned on purpose): the link opens that lock, by the hash type its
    // address spells, as a lock (the script page sends it to a family only where the family references
    // the code hash by that hash type)
    const scriptPage = expect.stringMatching(
      new RegExp(`/script/${unregistered}\\?hashType=type&kind=lock$`)
    );
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
      // B4 fix round: a real address on the lock the row names (a row reads its lock off it)
      address: lockCodeHash === null ? null : seededAddress(String(i).repeat(6), lockCodeHash),
      lockCodeHash,
      lockKind,
    });
    const rows = [
      event(1, 'mint', unregistered, null),
      event(2, 'burn', JOYID_CODE_HASH, 'JoyID'),
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

    // Readability F3-e: every column budgeted from what it holds, at 0.6 em a glyph (the cells
    // 13 px, heads and notes 12) and the 16 px gap after every column but the last. The amounts
    // hold one line under a billion whole units at the token's own decimals (none here); a larger
    // one breaks after a thousands separator.
    // B4 second fix round (review-rules-fixes-logic NOTE 5, the coordinator's item 11): a lock on
    // the default code hash by data is written "lock 0x9bd7e06f…a3cce8 by data": the column holds
    // it, "lock 0x…" one unit and "by data" another, every run within the column's budget.
    it('holds a by-data lock’s words within the To / from column', () => {
      const byData = {
        ...event(
          4,
          'mint',
          '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
          'Default Lock'
        ),
        address:
          'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq9nnw7qkdnnclfkg59uzn8umtfd2kwxceqvguktl',
      };
      render(
        <AssetHistory
          assetClass="tokens"
          weeklyNoun="transfers"
          daily={daily}
          weekly={[]}
          completeThrough="2024-01-08"
          token={{ symbol: 'T', decimals: 0 }}
          supplyEvents={{ total: 1, rows: [byData] }}
        />
      );
      fireEvent.click(screen.getByText('Every change to the supply (1)'));
      const table = screen.getByRole('table', { name: 'Mints and burns' });
      const cols = table.querySelectorAll('col');
      const column = Number.parseFloat((cols[cols.length - 1] as HTMLElement).style.minWidth);
      const cell = within(within(table).getAllByRole('row')[1]).getAllByRole('cell').at(-1)!;
      expect(cell).toHaveTextContent('lock 0x9bd7e06f…a3cce8 by data');
      const past = lineRuns(cell, 1280).filter((r) => r.px > column + 1e-9);
      expect(past.map((r) => `${r.text} ${r.px.toFixed(1)}`)).toEqual([]);
    });

    it('budgets every column from what it holds and changes form at their sum (F3-e)', () => {
      const at = (text: string, px: number) => (text.length * px * 3) / 5;
      const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
      const budgets = [
        // a date, "2024-01-02", wider than the head's words
        ceil(at('2024-01-02', 13) + 16),
        // the head "Change", wider than "Mint" or "Burn"
        ceil(Math.max(at('Change', 12), at('Burn', 13)) + 16),
        // "#99,999,999" over "tx 9999 · sequence 9999" at 12 px, the wider
        ceil(Math.max(at('#99,999,999', 13), at('tx 9999 · sequence 9999', 12)) + 16),
        // its sign and a billion whole units less one, at the token's 0 decimals
        ceil(at('+999,999,999', 13) + 16),
        ceil(at('999,999,999', 13) + 16),
        // the party's lock hash at the row size over "lock 0x43434343…434343" at 12 px; the last
        ceil(Math.max(at('0x11111111…111111', 13), at('lock 0x43434343…434343', 12))),
      ];
      expect(budgets).toEqual([94, 60, 182, 110, 102, 159]);
      const sum = budgets.reduce((s, w) => s + w, 0);
      expect(sum).toBe(707);
      mount();
      const table = screen.getByRole('table', { name: 'Mints and burns' });
      const cols = [...table.querySelectorAll('col')].map((c) =>
        Number.parseFloat((c as HTMLElement).style.minWidth)
      );
      expect(cols).toEqual(budgets);
      expect(Number.parseFloat(table.style.minWidth)).toBe(sum);
      cleanup();
      try {
        for (const [width, form] of [
          [1232, 'table'],
          [707, 'table'],
          [706, 'rows'],
          [608, 'rows'],
          [358, 'rows'],
        ] as const) {
          atContainerWidth(width);
          mount();
          const list = screen
            .getAllByRole(form === 'table' ? 'table' : 'list')
            .find((l) => l.getAttribute('aria-label')?.startsWith('Mints and burns'))!;
          expect(list.closest('[data-list-form]')).toHaveAttribute('data-list-form', form);
          cleanup();
          vi.unstubAllGlobals();
          vi.restoreAllMocks();
        }
      } finally {
        vi.unstubAllGlobals();
        vi.restoreAllMocks();
      }
    });

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

    // R9: the events fall to their phone rows at 1024 too (the token page's 676 px body, under the
    // table's 815 px), beside the holders' table that writes the same addresses at the row size:
    // tight only below sm, so from it a phone row writes the row size.
    it('in a phone row drawn from sm, at the row size', () => {
      atContainerWidth(676);
      atViewportWidth(1024);
      try {
        mount();
        const [unnamed, named] = screen.getAllByTestId('supply-event-row-mobile');
        expect(phoneLines(unnamed).map((l) => l.key)).toEqual(['change', 'evidence']);
        expect(phoneLines(unnamed)[1].right).toBe(
          `to ckb1…${eventAddress(1, unregistered).slice(-8)} lock 0x43434343…434343`
        );
        expect(phoneLines(named)[1].right).toBe(
          `from ckb1…${eventAddress(2, JOYID_CODE_HASH).slice(-8)} JoyID`
        );
      } finally {
        vi.restoreAllMocks();
        vi.unstubAllGlobals();
      }
    });

    it('in the phone row at 358 px', () => {
      atContainerWidth(358);
      atViewportWidth(390);
      try {
        mount();
        const [unnamed, named, notIndexed] = screen.getAllByTestId('supply-event-row-mobile');
        expect(phoneLines(unnamed)[2].right).toBe(
          `to ckb1…${eventAddress(1, unregistered).slice(-6)} lock 0x4343…4343`
        );
        expect(within(unnamed).getByRole('link', { name: '0x4343…4343' })).toHaveAttribute(
          'href',
          scriptPage
        );
        expect(phoneLines(named)[2].right).toBe(
          `from ckb1…${eventAddress(2, JOYID_CODE_HASH).slice(-6)} JoyID`
        );
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
                lockCodeHash: JOYID_CODE_HASH,
                lockKind: 'JoyID',
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
    expect(row.querySelector('[data-lock-kind]')).toHaveTextContent('JoyID');
    cleanup();
    atContainerWidth(358);
    try {
      mount();
      const [phone] = screen.getAllByTestId('supply-event-row-mobile');
      expect(within(phone).getByRole('link', { name: /^0x8888/ })).toHaveAttribute(
        'href',
        addressPage
      );
      expect(phoneLines(phone)[2].right).toMatch(/^to 0x8888.* JoyID$/);
    } finally {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    }
  });
  it('names what the weekly totals count and says when only the newest supply events are listed', () => {
    stubChartWidth(358);
    // re-pinned on purpose (lane P2's third round, item 7): a series of one week is said in words and draws no chart, so the weekly chart
    // it names reads a series of two weeks
    render(
      <AssetHistory
        assetClass="tokens"
        weeklyNoun="transactions"
        daily={dailyOfTwoWeeks}
        weekly={[
          { weekStart: '2024-01-01', count: 3 },
          { weekStart: '2024-01-08', count: 5 },
        ]}
        completeThrough="2024-01-15"
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
  // Lane P2's third round (item 7): a series of one complete week is said in words, the
  // readout's line, and no chart is drawn of one bar (one mark says nothing a figure does not,
  // and its axis invites a wrong reading); two weeks and more are a chart
  it('says a single complete week in words and draws no chart of one bar', () => {
    stubChartWidth(1248);
    try {
      const { unmount } = render(
        <AssetHistory
          assetClass="objects"
          weeklyNoun="transfers"
          daily={daily}
          weekly={[{ weekStart: '2024-01-01', count: 3 }]}
          completeThrough="2024-01-08"
        />
      );
      expect(screen.queryByRole('img', { name: /per week/ })).toBeNull();
      expect(screen.getByLabelText('History readout')).toHaveTextContent(
        'Latest complete week, from 2024-01-01: 3 transfers'
      );
      unmount();
      render(
        <AssetHistory
          assetClass="objects"
          weeklyNoun="transfers"
          daily={[
            ...daily,
            {
              date: '2024-01-15',
              capacity: '20000000001',
              occupiedCapacity: '6100000000',
              freeCapacity: '13900000001',
              supply: '26',
            },
          ]}
          weekly={[
            { weekStart: '2024-01-01', count: 3 },
            { weekStart: '2024-01-08', count: 5 },
          ]}
          completeThrough="2024-01-15"
        />
      );
      expect(screen.getByRole('img', { name: /Transfers per week/ })).toBeInTheDocument();
    } finally {
      vi.unstubAllGlobals();
    }
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
    // Lane P2's third round (item 13): the free part is the route's, read, never capacity −
    // occupied here; parts that do not make the whole are refused, never repaired
    expect(() =>
      render(
        <AssetHistory
          assetClass="objects"
          weeklyNoun="transfers"
          daily={[{ ...daily[0], freeCapacity: '3900000000' }]}
          weekly={[]}
          completeThrough="2024-01-08"
        />
      )
    ).toThrow(
      '2024-01-01: occupied 6100000000 and free 3900000000 do not make the capacity 10000000001'
    );
    error.mockRestore();
  });
});
