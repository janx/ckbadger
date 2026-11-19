import { afterEach, onTestFinished, vi } from 'vitest';
import { fireEvent } from '@testing-library/react';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { AddressBalance, addressStripFacts } from '@/components/address/address-holdings';
import { StatusStrip } from '@/components/ui/status-strip';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import {
  AddressActivity,
  AddressLiveCells,
  AddressTransactions,
} from '@/components/address/address-history';
import {
  addressFixture,
  holdingsFixture,
  addressActivitiesFixture,
  addressTransactionsFixture,
} from '@/__tests__/msw/fixtures/address';
import { addressProducerCells } from '@/__tests__/msw/fixtures/address-producer';
import { expectBefore, expectOneRowSeal } from '@/__tests__/utils/seals';
import { ROW_SEAL } from '@/components/ui/row-seal';
import { ROW_LINE_PX } from '@/lib/type-metrics';
import { addressCellSealId } from '@/lib/address-cell-seal';
import type { Activity, ItemDelta } from '@/lib/api';
import { formatUtc8Date } from '@/lib/utils';
import { factsOf } from '@/__tests__/utils/facts';

describe('Address v3 components', () => {
  it('keeps the balance hero once and preserves a single shannon in the partition', () => {
    render(
      <AddressBalance
        holdings={{
          ...holdingsFixture,
          balance: '10000000001',
          occupiedCapacity: '6100000000',
          freeCapacity: '3900000001',
          daoFreeCapacity: '0',
        }}
      />
    );
    // Readability R5 (re-pinned on purpose): the hero is the summary form, its single shannon in
    // its title (it showed every decimal: "100.00000001").
    const figure = screen.getByLabelText('Balance').querySelector('[data-ckb-amount]')!;
    expect(figure).toHaveTextContent(/^100$/);
    expect(figure).toHaveAttribute('title', '100.00000001 CKB');
    expect(screen.queryByText('CKB held')).not.toBeInTheDocument();
    // Lane P (Address.dc.html): the exact parts are the quiet line under the bar. Re-pinned on
    // purpose (lane P1): what it can spend stands beside the balance, its single shannon there
    expect(
      screen.getByLabelText('Balance').querySelector('[data-role="balance-beside"]')
    ).toHaveTextContent('39.00000001');
  });

  // Lane P, the Address board's balance hero: the 40 px figure (its fraction 22 px, dim), the
  // live cells at its right, the 18 px partition with the shares inside, one quiet line of exact
  // parts, and no DAO segment when nothing is in the Nervos DAO.
  describe('the balance hero (Address.dc.html)', () => {
    const board = {
      ...holdingsFixture,
      balance: '31974266879443',
      occupiedCapacity: '29148600000000',
      freeCapacity: '2825666879443',
      daoFreeCapacity: '0',
      depositCellsCount: 0,
      withdrawalRequestCellsCount: 0,
      liveCellsCount: 828,
    };
    afterEach(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });

    // Renamed and re-pinned on purpose (lane P1, the plan's P1 paragraph: spendable CKB and
    // unclaimed compensation stand beside the balance; said once: "in 828 live cells" repeated
    // the status strip's live cells). It was "… with its live cells beside it".
    it('sets the balance as the display figure with what it can spend beside it', () => {
      stubChartWidth(1248);
      render(<AddressBalance holdings={board} />);
      const hero = screen.getByRole('region', { name: 'Balance' });
      // material pass v2 (move 3): the hero label is an eyebrow (`.eyebrow`: upper case, .12 em, 500)
      expect(within(hero).getByText('balance')).toHaveClass('text-meta', 'eyebrow');
      const figure = hero.querySelector('[data-role="balance-figure"]')!;
      // Readability R5 (re-pinned on purpose): the hero figure is the summary form, whole CKB,
      // one run in one ink, its exact value in its title — was "319,742.66879443", the fraction
      // 22 px (17 px) dim after a <wbr>. "33,600,000,000 CKB" (14 characters) is 336 px at 40 px
      // and 252 px at 30 px on a phone, plus " CKB" (31.2 px): no break is needed.
      expect(figure).toHaveTextContent(/^319,742 CKB$/);
      const amount = figure.querySelector('[data-ckb-amount]')!;
      expect(amount).toHaveAttribute('title', '319,742.66879443 CKB');
      expect(amount.closest('.text-entity')).toHaveClass('sm:text-display', 'text-text-bright');
      expect(within(figure as HTMLElement).queryByText(/^\.66879443$/)).toBeNull();
      expect(figure.querySelector('wbr')).toBeNull();
      expect(hero).not.toHaveTextContent(/live cells?/);
      // beside the balance, exact: what it can spend; no compensation where it holds no DAO cell
      expect(factsOf(hero.querySelector('[data-role="balance-beside"]')!)).toEqual([
        ['spendable', '28,256.66879443 CKB'],
      ]);
    });

    it('draws the 18 px partition with each share inside and the exact parts in one line', () => {
      stubChartWidth(1248);
      render(<AddressBalance holdings={board} />);
      const bar = screen.getByRole('img', { name: /^Balance partition:/ });
      // re-pinned on purpose (lane P1; review chain-b address-dao-depositor: three "free"s for
      // three things): the parts are "occupied", "spendable" and "in the DAO"
      expect(bar).toHaveAccessibleName(
        'Balance partition: occupied 291,486.00000000 CKB (91.16%), spendable 28,256.66879443 CKB (8.84%), nothing in the Nervos DAO'
      );
      expect(bar).toHaveClass('h-[18px]');
      const parts = [...bar.querySelectorAll('[data-part]')];
      expect(parts.map((p) => p.getAttribute('data-part'))).toEqual(['occupied', 'spendable']);
      // "spendable 8.84%" (15 glyphs, 108 px and 16 of padding) is wider than its 8.84 % of the
      // 1,246 px bar (110 px): its share stands in the part's title, never cut (the next test)
      expect(parts.map((p) => p.textContent)).toEqual(['occupied 91.16%', '']);
      expect(parts[1]).toHaveAttribute('title', 'spendable 8.84% · 28,256.66879443 CKB');
      // the exact parts in their exact form (R5, re-pinned on purpose: was padded to 8 decimals);
      // re-pinned with readability R4 (lane B7): labelled facts, each named as its part of the bar
      // (was one line joined by "·")
      // re-pinned on purpose (lane P1): the parts under the bar are the two the reader cannot
      // spend, each with its unit (the bar is by CKB); what it can spend stands beside the
      // balance, said once
      expect(factsOf(screen.getByTestId('balance-parts'))).toEqual([
        ['occupied', '291,486 CKB pays for the bytes its cells keep'],
        ['in the DAO', 'no CKB in the Nervos DAO'],
      ]);
    });

    it('keeps a share that cannot fit inside its part in the part title, never cut', () => {
      stubChartWidth(358);
      render(<AddressBalance holdings={board} />);
      const free = screen
        .getByRole('img', { name: /^Balance partition:/ })
        .querySelector('[data-part="spendable"]')!;
      expect(free).toHaveTextContent(/^$/);
      // re-pinned on purpose (lane P1): the part is "spendable"
      expect(free).toHaveAttribute('title', 'spendable 8.84% · 28,256.66879443 CKB');
    });

    it('adds the DAO part only when the address has CKB in the Nervos DAO', () => {
      stubChartWidth(1248);
      render(<AddressBalance holdings={holdingsFixture} />);
      const bar = screen.getByRole('img', { name: /^Balance partition:/ });
      expect(
        [...bar.querySelectorAll('[data-part]')].map((p) => p.getAttribute('data-part'))
      ).toEqual(['occupied', 'spendable', 'in the DAO']);
      // the exact form (R5, re-pinned on purpose: was "9,589,480.00000000"); re-pinned with
      // readability R4 (lane B7): a labelled fact (was the line's last item after "·"); re-pinned
      // on purpose (lane P1): "in the DAO", with its unit (it was "DAO free")
      expect(factsOf(screen.getByTestId('balance-parts')).at(-1)).toEqual([
        'in the DAO',
        '9,589,480 CKB, free part of 5 deposits',
      ]);
      // lane P1: its unclaimed compensation stands beside the balance, exact, with what it can
      // spend
      expect(
        factsOf(
          screen
            .getByRole('region', { name: 'Balance' })
            .querySelector('[data-role="balance-beside"]')!
        )
      ).toEqual([
        ['spendable', '54,571.35338057 CKB'],
        ['unclaimed compensation', '259,870.84255174 CKB'],
      ]);
    });

    it('states a zero balance as a ghost 0 CKB with no bar', () => {
      render(
        <AddressBalance
          holdings={{
            ...board,
            balance: '0',
            occupiedCapacity: '0',
            freeCapacity: '0',
            liveCellsCount: 0,
          }}
        />
      );
      const figure = document.querySelector('[data-role="balance-figure"]')!;
      expect(figure).toHaveTextContent(/^0 CKB$/);
      // R4 (re-pinned on purpose): a zero balance is a datum, in the dim ink, not the ghost
      expect(within(figure as HTMLElement).getByText('0')).toHaveClass('text-text-dim');
      expect(screen.queryByRole('img', { name: /^Balance partition:/ })).toBeNull();
    });
  });

  // Lane P: the four metrics under the balance — Holdings · Live cells · Transactions · Lock.
  // Re-pinned on purpose (lane D2, PA-13; renamed, the describe was "the metrics row"): the four
  // are the status strip's facts under the id line, said once (the kind line said three of them
  // again), in the strip's order and lowercase labels: lock, live cells, transactions, holdings.
  // The live cells' note by class went: the holdings section states each class's cells under its
  // heading, so the strip states the count alone.
  describe('the status strip (lane D2; the metrics row before)', () => {
    it('names the lock, the live cells, the transactions and the holdings', () => {
      render(
        <StatusStrip
          label="Address status"
          facts={addressStripFacts(addressFixture, holdingsFixture)}
        />
      );
      const strip = screen.getByRole('group', { name: 'Address status' });
      expect(factsOf(strip)).toEqual([
        ['lock', 'JoyID · type · 22-byte args'],
        ['live cells', '40'],
        ['transactions', '1,021'],
        // re-pinned (A4 round 2): each count a unit, number and noun joined by a plain space.
        // Re-pinned on purpose (lane P1; review of phases C and D, NOTE 12): no "29 holdings", a
        // sum of unlike counts (DAO cells, token kinds, objects, identities) made in the page; the
        // strip states each class's count, the API's, the first its value and the rest its notes
        ['holdings', '5 DAO cells · 9 tokens · 14 objects · 1 identity'],
      ]);
      expect(within(strip).getByRole('link', { name: 'JoyID' })).toHaveAttribute(
        'href',
        expect.stringContaining('/scripts/')
      );
    });

    it('reads one held class as the board does: "827 objects · in 14 collections"', () => {
      const objectsOnly = {
        ...holdingsFixture,
        holdings: { dao: 0, token: 0, object: 827, identity: 0 },
        objectCollections: Array.from({ length: 14 }, (_, i) => ({
          ...holdingsFixture.objectCollections[0],
          id: `0x${String(i).padStart(2, '0').repeat(32)}`,
        })),
      };
      render(
        <StatusStrip
          label="Address status"
          facts={addressStripFacts(addressFixture, objectsOnly)}
        />
      );
      const holdings = factsOf(screen.getByRole('group', { name: 'Address status' }))[3];
      expect(holdings).toEqual(['holdings', '827 objects · in 14 collections']);
    });

    // Lane D2: a fact the page does not know is not passed: before the holdings read answers, the
    // strip states the lock and the transactions alone; the class counts must add up to the live
    // cells it states, or it refuses them by name.
    it('states what it knows before the holdings answer, and refuses class counts that do not add up', () => {
      const { unmount } = render(
        <StatusStrip label="Address status" facts={addressStripFacts(addressFixture, undefined)} />
      );
      expect(factsOf(screen.getByRole('group', { name: 'Address status' }))).toEqual([
        ['lock', 'JoyID · type · 22-byte args'],
        ['transactions', '1,021'],
      ]);
      unmount();
      expect(() =>
        addressStripFacts(addressFixture, { ...holdingsFixture, liveCellsCount: 41 })
      ).toThrow('class cell counts add up to 40, not its 41 live cells');
    });
  });
  it('shows exact producer cell amounts and the data1 script reference', () => {
    render(<AddressLiveCells rows={addressProducerCells.data} />);
    const table = screen.getByRole('table', { name: 'Live cells' });
    expect(table).toHaveTextContent('1,000.00000000');
    expect(table.querySelector('a[href*="hashType=data1&kind=type"]')).not.toBeNull();
    const cell = addressProducerCells.data[0];
    expect(
      within(table)
        .getAllByRole('link')
        .some(
          (link) => link.getAttribute('href') === `/mainnet/cell/${cell.txHash}-${cell.outputIndex}`
        )
    ).toBe(true);
  });
  it('folds a live cell into a phone row at 358 px: outpoint and capacity · contents · occupied and created', () => {
    atContainerWidth(358);
    onTestFinished(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });
    render(<AddressLiveCells rows={addressProducerCells.data} />);
    expect(screen.queryByRole('table', { name: 'Live cells' })).toBeNull();
    const [row] = screen.getAllByTestId('address-cell-row-mobile');
    const lines = phoneLines(row);
    const cell = addressProducerCells.data[0];
    expect(lines.map((l) => [l.key, l.tone])).toEqual([
      ['cell', 'head'],
      ['contains', 'body'],
      ['figures', 'quiet'],
    ]);
    expect(lines[0].left).toMatch(new RegExp(`:${cell.outputIndex}$`));
    // a phone row's summary form (R5, re-pinned on purpose: was "1,000.00000000 CKB")
    expect(lines[0].right).toBe('1,000 CKB');
    expect(lines[2].left).toMatch(/^occupied .+ CKB · created #[\d,]+$/);
    expect(
      screen
        .getByRole('list', { name: 'Live cells, compact' })
        .querySelector('a[href*="hashType=data1&kind=type"]')
    ).not.toBeNull();
  });
  // The seals by kind (C3 for the address's cells, lane C4): every live cell leads with its own
  // 30 px row seal, in force (pressed: a live cell); no owner's stone (every cell here is the page
  // address's own). The producer's eleven cells cover every class the seal draws.
  describe('the cell’s row seal', () => {
    const cells = addressProducerCells.data;
    /** What each seal says, in the producer's order: the class `kindOf()` gives each cell. */
    const CLASSES = [
      'ckb',
      'other',
      'dao',
      'dao',
      'tokens',
      'tokens',
      'objects',
      'objects',
      'identities',
      'identities',
      'identities',
    ];
    afterEach(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });

    // re-pinned on purpose (readability R6): the cell's 18 px stone stands centred on the 20 px row
    // line, 1 px clear above and below, where the 30 px stone overhung it by 5 px (`-my-[5px]`);
    // renamed: "overhanging the 18 px line"
    it('leads every table row before its out point, centred on its line', () => {
      render(<AddressLiveCells rows={cells} />);
      const table = screen.getByRole('table', { name: 'Live cells' });
      const rows = within(table).getAllByRole('row').slice(1);
      expect(rows).toHaveLength(cells.length);
      rows.forEach((row, i) => {
        const cell = cells[i];
        const seal = expectOneRowSeal(row, addressCellSealId(cell));
        expect(seal.querySelector('svg')).toHaveAttribute(
          'aria-label',
          `Cell seal ${cell.txHash.slice(0, 10)}:${cell.outputIndex}, ${CLASSES[i]}, 1,000.00000000 CKB`
        );
        // first in the first column, before the out point (still the row's link), centred in a
        // box of the 20 px line: (20 − 18) / 2 = 1 px clear above and below
        const first = row.querySelector('td')!;
        expect(first.firstElementChild).toHaveClass('flex', 'items-start');
        expect(first.firstElementChild?.firstElementChild).toBe(seal);
        expect((ROW_LINE_PX - ROW_SEAL.cell.height) / 2).toBe(1);
        expect(seal).toHaveClass('h-row', 'items-center');
        expect(seal.className).not.toMatch(/-my-/);
        const outPoint = within(first).getByRole('link');
        expect(outPoint).toHaveAttribute(
          'href',
          `/mainnet/cell/${cell.txHash}-${cell.outputIndex}`
        );
        // U-09 (re-pinned): the table's out point at the row size
        expect(outPoint).toHaveTextContent(`0xdce40476…3993e1:${cell.outputIndex}`);
        expectBefore(seal, outPoint);
        expect(row.querySelector('[data-owner-seal]')).toBeNull();
      });
    });

    it.each([358, 608])(
      'leads every phone row at %i px as its mark, beside every line, with no overhang',
      (width) => {
        atContainerWidth(width);
        render(<AddressLiveCells rows={cells} />);
        expect(screen.queryByRole('table', { name: 'Live cells' })).toBeNull();
        const rows = screen.getAllByTestId('address-cell-row-mobile');
        expect(rows).toHaveLength(cells.length);
        rows.forEach((row, i) => {
          const cell = cells[i];
          const seal = expectOneRowSeal(row, addressCellSealId(cell));
          expect(row.firstElementChild).toHaveAttribute('data-part', 'mark');
          expect(row.firstElementChild).toContainElement(seal);
          expect(seal.className).not.toMatch(/-my-/);
          // the head line is the out point alone, the seal in its own column
          const lines = phoneLines(row);
          // U-09 (re-pinned): the phone row's out point at the tight size
          expect(lines[0].left).toBe(`0xdce4…93e1:${cell.outputIndex}`);
          expectBefore(seal, within(row).getAllByRole('link')[0]);
          expect(row.querySelector('[data-owner-seal]')).toBeNull();
        });
      }
    );

    // The seal's 38 px (30 + the 8 px gap) are budgeted in the Cell column and taken from
    // Occupied's and Created at's measured slack (see cellColumns): the table keeps its 992 px.
    // Re-pinned on purpose (readability F3-e): every column is its content (Cell 238 → 226, the
    // out point at 13 px and its 12.4 px of spare gone; Contains the flexible column, 280 → 282
    // minimum; Capacity 196 → 201 and Occupied 178 → 170 with their units at 12 px; Created at
    // 100 → 86), so the table is 965 px (renamed: it said "keeps the table at 992 px").
    // Re-pinned on purpose (readability R6): the cell's row stone 30 → 18 px, its footprint 38 →
    // 26, Cell 226 → 214, the table 953 px (renamed: "965 px").
    it('budgets the seal in the Cell column and keeps the table at 953 px', () => {
      render(<AddressLiveCells rows={cells} />);
      const table = screen.getByRole('table', { name: 'Live cells' });
      const widths = Array.from(table.querySelectorAll('col')).map(
        (c) => (c as HTMLElement).style.minWidth
      );
      expect(widths).toEqual(['214px', '282px', '201px', '170px', '86px']);
      // the out point and its gap, 187.6 rounded up with the stone's footprint
      expect(214 - 188).toBe(ROW_SEAL.cell.footprint);
      expect(Number.parseFloat(table.style.minWidth)).toBe(953);
    });

    // Re-pinned on purpose (readability F3-e): the table changes form at its 965 px (was 992).
    // Re-pinned on purpose (readability R6): at its 953 px (was 965).
    it.each([
      [953, 'table'],
      [952, 'rows'],
    ])('changes form where it did: at %i px it is %s', (width, form) => {
      atContainerWidth(width);
      const { container } = render(<AddressLiveCells rows={cells} />);
      expect(container.querySelector('[data-list-form]')).toHaveAttribute('data-list-form', form);
    });
  });

  // Readability F3-e: every column of both address tables budgeted from what it holds, at 0.6 em a
  // glyph (13 px cells, 12 px notes and units) and the 16 px gap after every column but the last.
  // Contains (what a cell holds) and Scripts (the scripts a transaction calls) are the flexible
  // columns.
  describe('budgets every column from what it holds (F3-e)', () => {
    const at = (text: string, px: number) => (text.length * px * 3) / 5;
    const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
    afterEach(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });

    // re-pinned on purpose (readability R6): the cell's stone footprint 38 → 26, Cell 226 → 214,
    // 953 px (renamed: "965 px")
    it('the live cells: 953 px, a table from it', () => {
      const budgets = [
        // the stone and its gap, the out point at the row size with a four-digit index
        ceil(ROW_SEAL.cell.footprint + at('0xdce40476…3993e1:9999', 13) + 16),
        // the class form whole: the 16 px mark, "Other scripts", the type script's hash
        ceil(16 + 8 + at('Other scripts', 13) + 8 + at('0x00000000…000000', 13) + 16),
        // under a billion CKB on one line with its unit at 12 px
        ceil(at('999,999,999.99999999', 13) + at(' CKB', 12) + 16),
        // a cell is at most a block's 597,000 bytes
        ceil(at('597,000.00000000', 13) + at(' CKB', 12) + 16),
        // "#99,999,999", the last column
        ceil(at('#99,999,999', 13)),
      ];
      expect(budgets).toEqual([214, 282, 201, 170, 86]);
      expect(budgets.reduce((s, w) => s + w, 0)).toBe(953);
      render(<AddressLiveCells rows={addressProducerCells.data} />);
      const table = screen.getByRole('table', { name: 'Live cells' });
      const cols = [...table.querySelectorAll('col')].map((c) => (c as HTMLElement).style);
      expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual(budgets);
      expect(cols[1].width).toBe('auto');
      expect(Number.parseFloat(table.style.minWidth)).toBe(953);
    });

    it('the transactions: 1,006 px, a table from it', () => {
      const budgets = [
        // the hash at the row size
        ceil(at('0x00000000…000000', 13) + 16),
        // a pool transaction's wait on one line, wider than "#99,999,999", a date or its badge
        ceil(at('in pool for 99d', 13) + 16),
        // four digits a side (the inputs as written, the outputs grouped)
        ceil(at('9999 → 9,999', 13) + 16),
        // a change under a billion CKB with its sign
        ceil(at('+999,999,999.99999999', 13) + 16),
        // a fee under 100 CKB
        ceil(at('99.99999999', 13) + 16),
        // "Unavailable", wider than a size of "597,000"
        ceil(at('Unavailable', 13) + 16),
        // the cycle limit, "3,500,000,000"
        ceil(at('3,500,000,000', 13) + 16),
        // a script: its 12 px mark, its 6 px gap and 12 characters of its name; the last column
        ceil(12 + 6 + at('x'.repeat(12), 13)),
      ];
      expect(budgets).toEqual([149, 133, 110, 180, 102, 102, 118, 112]);
      expect(budgets.reduce((s, w) => s + w, 0)).toBe(1006);
      render(<AddressTransactions rows={addressTransactionsFixture} />);
      const table = screen.getByRole('table');
      const cols = [...table.querySelectorAll('col')].map((c) => (c as HTMLElement).style);
      expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual(budgets);
      expect(cols[7].width).toBe('auto');
      expect(Number.parseFloat(table.style.minWidth)).toBe(1006);
      for (const [width, form] of [
        [1232, 'table'],
        [1006, 'table'],
        [1005, 'rows'],
        [608, 'rows'],
        [358, 'rows'],
      ] as const) {
        vi.unstubAllGlobals();
        vi.restoreAllMocks();
        atContainerWidth(width);
        const { container, unmount } = render(
          <AddressTransactions rows={addressTransactionsFixture} />
        );
        expect(container.querySelector('[data-list-form]')).toHaveAttribute('data-list-form', form);
        unmount();
      }
    });
  });

  it('retains null cycles as unavailable and never a zero measurement', () => {
    render(
      <AddressTransactions
        rows={[{ ...addressTransactionsFixture[0], cycles: null, cyclesStatus: 'pending' }]}
      />
    );
    expect(screen.getByRole('table', { name: 'Address transactions' })).toHaveTextContent(
      'Not computed'
    );
  });
  it('counts a cellbase as 0 inputs, not its placeholder input (BA-11)', () => {
    render(
      <AddressTransactions
        rows={[
          { ...addressTransactionsFixture[0], isCellbase: true, inputsCount: 1, outputsCount: 1 },
        ]}
      />
    );
    const table = screen.getByRole('table', { name: 'Address transactions' });
    expect(table).toHaveTextContent('0 → 1');
    expect(table).not.toHaveTextContent('1 → 1');
  });
  it('renders the participant position and a sent headline without claiming a burn', () => {
    render(
      <AddressActivity
        address={addressFixture}
        activity={{
          ...addressActivitiesFixture[0],
          tags: 1,
          itemDeltas: [
            { kind: 'token', typeScriptHash: '0x' + 'ab'.repeat(32), delta: '-1', decimals: 8 },
          ],
        }}
      />
    );
    expect(screen.getByText('Token sent')).toBeInTheDocument();
    expect(screen.queryByText('Token burn')).not.toBeInTheDocument();
    expect(screen.getByText(/0.00000001/)).toBeInTheDocument();
  });
  it('colours its token amounts by sign, jade received and rouge sent (Address board, BA-35)', () => {
    const tokenRow = (delta: string) => ({
      ...addressActivitiesFixture[0],
      tags: 1,
      itemDeltas: [
        {
          kind: 'token' as const,
          typeScriptHash: '0x' + 'ab'.repeat(32),
          delta,
          symbol: 'iCKB',
          decimals: 8,
        },
      ],
    });
    const { rerender } = render(
      <AddressActivity address={addressFixture} activity={tokenRow('100000000')} />
    );
    expect(screen.getByRole('link', { name: /iCKB/ })).toHaveClass('text-positive');
    rerender(<AddressActivity address={addressFixture} activity={tokenRow('-100000000')} />);
    expect(screen.getByRole('link', { name: /iCKB/ })).toHaveClass('text-negative');
  });
});

// Consistency C3: the address feed is the site's one activity row (ActivityRow), with this
// address's line first, as text and tagged "this address" (Address.dc.html → History).
describe('Address activity is the shared activity row (C3)', () => {
  const nervape = (n: number): ItemDelta => ({
    kind: 'object',
    objectId: `0x${n.toString(16).padStart(2, '0').repeat(32)}`,
    delta: 1,
    collection: { id: '0x' + '55'.repeat(32), name: 'Nervape Special' },
    class: 'object',
    composition: 'btc_ckb',
    role: 'spore',
  });
  const received = (count: number): Activity => ({
    ...addressActivitiesFixture[0],
    tags: 2,
    ckbDelta: '326700000000',
    usedDelta: '323100000000',
    itemDeltas: Array.from({ length: count }, (_, i) => nervape(i + 1)),
  });
  const article = (c: HTMLElement) => c.querySelector('article[data-feed-row]') as HTMLElement;
  const lines = (c: HTMLElement) => [...c.querySelectorAll('[data-role="participant"]')];

  it('is one ActivityRow with one participant line — this address, as text', () => {
    const { container } = render(
      <AddressActivity address={addressFixture} activity={received(9)} />
    );
    const row = article(container);
    expect(row).toHaveAttribute('data-feed-row', addressActivitiesFixture[0].txHash);
    // The endpoint gives this participant's deltas only: one line, the fixture's one participant.
    expect(lines(container)).toHaveLength(1);
    expect(container.querySelectorAll('[data-party]')).toHaveLength(1);
    const [line] = lines(container);
    expect(within(line as HTMLElement).getAllByText('this address')).toHaveLength(1);
    // B4 (re-pinned): LockKind's words inside the line's lock-kind part (JoyID is not the
    // default lock, so it is named)
    expect(
      within(line as HTMLElement)
        .getByText('JoyID')
        .closest('[data-role="lock-kind"]')
    ).not.toBeNull();
    // Never a link to the page it is on; the full address is its hover.
    const party = line.querySelector('[data-role="party"]') as HTMLElement;
    expect(within(party).queryByRole('link')).toBeNull();
    expect(within(party).getByTitle(addressFixture.address!)).toBeInTheDocument();
  });

  it('heads the row as the board: the headline links to the tx, block · date on the right, hash and age in its hover', () => {
    const { container } = render(
      <AddressActivity address={addressFixture} activity={received(9)} />
    );
    const row = article(container);
    const a = addressActivitiesFixture[0];
    expect(within(row).getByRole('link', { name: /Received 9\sobjects/ })).toHaveAttribute(
      'href',
      `/mainnet/tx/${a.txHash}`
    );
    expect(row.querySelector('[data-role="headline"]')).toHaveTextContent(
      'Received 9 objects · Nervape Special'
    );
    const evidence = row.querySelector('[data-role="evidence"]') as HTMLElement;
    expect(evidence).toHaveTextContent(`#18,672,923 · ${formatUtc8Date(a.timestamp!)}`);
    expect(evidence).toHaveTextContent(/^#18,672,923 · \d{4}-\d\d-\d\d$/);
    expect(row).toHaveAttribute(
      'title',
      expect.stringMatching(new RegExp(`^${a.txHash} · \\d+d ago$`))
    );
    expect(row).not.toHaveTextContent(a.txHash.slice(0, 10));
  });

  it('folds the batch behind "+N more objects" and says the shared tier once', () => {
    const { container } = render(
      <AddressActivity address={addressFixture} activity={received(9)} />
    );
    const [line] = lines(container) as HTMLElement[];
    expect(line.querySelectorAll('a[href*="/objects/"]')).toHaveLength(1);
    const more = within(line).getByRole('button', { name: '+8 more objects' });
    expect(line.querySelectorAll('[data-composition="btc_ckb"]')).toHaveLength(1);
    fireEvent.click(more);
    expect(line.querySelectorAll('a[href*="/objects/"]')).toHaveLength(9);
    expect(line.querySelectorAll('[data-composition="btc_ckb"]')).toHaveLength(1);
  });

  it('says one object, a departure, and a batch across collections in the same grammar', () => {
    const one = render(<AddressActivity address={addressFixture} activity={received(1)} />);
    expect(one.container.querySelector('[data-role="headline"]')).toHaveTextContent(
      /^Received 1 object · Nervape Special$/
    );
    one.unmount();
    const sent = render(
      <AddressActivity
        address={addressFixture}
        activity={{
          ...received(2),
          ckbDelta: '-100',
          itemDeltas: received(2).itemDeltas.map((d) =>
            d.kind === 'object' ? { ...d, delta: -1 } : d
          ),
        }}
      />
    );
    expect(sent.container.querySelector('[data-role="headline"]')).toHaveTextContent(
      /^Sent 2 objects · Nervape Special$/
    );
    sent.unmount();
    const mixed = received(2);
    const other = {
      ...nervape(3),
      collection: { id: '0x' + '66'.repeat(32), name: 'Nervape Suit' },
    };
    const across = render(
      <AddressActivity
        address={addressFixture}
        activity={{ ...mixed, itemDeltas: [...mixed.itemDeltas, other] }}
      />
    );
    expect(across.container.querySelector('[data-role="headline"]')).toHaveTextContent(
      /^Received 3 objects$/
    );
  });

  it('keeps its roles on the participant line, not in a separate "Roles:" line', () => {
    const { container } = render(
      <AddressActivity
        address={addressFixture}
        activity={{ ...addressActivitiesFixture[0], roles: ['owner'] }}
      />
    );
    const party = container.querySelector('[data-role="party"]') as HTMLElement;
    expect(party).toHaveTextContent('owner');
    expect(container).not.toHaveTextContent('Roles:');
  });
});
