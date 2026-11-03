import { afterEach, onTestFinished, vi } from 'vitest';
import { fireEvent } from '@testing-library/react';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { AddressBalance, AddressMetrics } from '@/components/address/address-holdings';
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
import type { Activity, ItemDelta } from '@/lib/api';
import { formatUtc8Date } from '@/lib/utils';

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
    expect(screen.getByLabelText('Balance')).toHaveTextContent('100.00000001');
    expect(screen.queryByText('CKB held')).not.toBeInTheDocument();
    // Lane P (Address.dc.html): the exact parts are the quiet line under the bar.
    expect(screen.getByLabelText('Address capacity partition')).toHaveTextContent('39.00000001');
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

    it('sets the balance as the display figure with its live cells beside it', () => {
      stubChartWidth(1248);
      render(<AddressBalance holdings={board} />);
      const hero = screen.getByRole('region', { name: 'Balance' });
      // material pass v2 (move 3): the hero label is an eyebrow (`.eyebrow`: upper case, .12 em, 500)
      expect(within(hero).getByText('balance')).toHaveClass('text-meta', 'eyebrow');
      const figure = hero.querySelector('[data-role="balance-figure"]')!;
      expect(figure).toHaveTextContent(/^319,742\.66879443 CKB$/);
      // 40 px from sm (30 px, one step smaller, on a phone); the fraction 22 px (17 px), dim.
      expect(within(figure as HTMLElement).getByText('319,742')).toHaveClass(
        'sm:text-display',
        'text-entity',
        'text-text-bright'
      );
      expect(within(figure as HTMLElement).getByText('.66879443')).toHaveClass(
        'sm:text-hero',
        'text-section',
        'text-text-dim'
      );
      expect(figure.querySelector('wbr')).not.toBeNull();
      expect(hero).toHaveTextContent('in 828 live cells');
    });

    it('draws the 18 px partition with each share inside and the exact parts in one line', () => {
      stubChartWidth(1248);
      render(<AddressBalance holdings={board} />);
      const bar = screen.getByRole('img', { name: /^Balance partition:/ });
      expect(bar).toHaveAccessibleName(
        'Balance partition: occupied 291,486.00000000 CKB (91.16 %), free 28,256.66879443 CKB (8.84 %), nothing in the Nervos DAO'
      );
      expect(bar).toHaveClass('h-[18px]');
      const parts = [...bar.querySelectorAll('[data-part]')];
      expect(parts.map((p) => p.getAttribute('data-part'))).toEqual(['occupied', 'free']);
      expect(parts.map((p) => p.textContent)).toEqual(['occupied 91.16 %', 'free 8.84 %']);
      expect(screen.getByTestId('balance-parts')).toHaveTextContent(
        /^occupied 291,486\.00000000 pays for the bytes its cells keep · free 28,256\.66879443 · no CKB in the Nervos DAO$/
      );
    });

    it('keeps a share that cannot fit inside its part in the part title, never cut', () => {
      stubChartWidth(358);
      render(<AddressBalance holdings={board} />);
      const free = screen
        .getByRole('img', { name: /^Balance partition:/ })
        .querySelector('[data-part="free"]')!;
      expect(free).toHaveTextContent(/^$/);
      expect(free).toHaveAttribute('title', 'free 8.84 % · 28,256.66879443 CKB');
    });

    it('adds the DAO part only when the address has CKB in the Nervos DAO', () => {
      stubChartWidth(1248);
      render(<AddressBalance holdings={holdingsFixture} />);
      const bar = screen.getByRole('img', { name: /^Balance partition:/ });
      expect(
        [...bar.querySelectorAll('[data-part]')].map((p) => p.getAttribute('data-part'))
      ).toEqual(['occupied', 'free', 'DAO free']);
      expect(screen.getByTestId('balance-parts')).toHaveTextContent(
        /· DAO free 9,589,480\.00000000, free part of 5 deposits$/
      );
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
      expect(within(figure as HTMLElement).getByText('0')).toHaveClass('text-text-ghost');
      expect(screen.queryByRole('img', { name: /^Balance partition:/ })).toBeNull();
    });
  });

  // Lane P: the four metrics under the balance — Holdings · Live cells · Transactions · Lock.
  describe('the metrics row (Address.dc.html)', () => {
    it('names the holdings, the live cells by class, the transactions and the lock', () => {
      render(<AddressMetrics holdings={holdingsFixture} address={addressFixture} />);
      const row = screen.getByRole('region', { name: 'Metrics' });
      const items = [...row.querySelectorAll('dl > div')];
      expect(items.map((i) => i.querySelector('dt')!.textContent)).toEqual([
        'Holdings',
        'Live cells',
        'Transactions',
        'Lock',
      ]);
      expect(items.map((i) => i.querySelector('dd')!.textContent)).toEqual([
        '29 holdings · 5 DAO cells · 9 tokens · 14 objects · 1 identity',
        '40 · 5 DAO cells + 15 token cells + 14 objects + 1 identity + 3 other-script cells + 2 plain CKB',
        '1,021',
        'JoyID · type · 22-byte args',
      ]);
      expect(within(items[3] as HTMLElement).getByRole('link', { name: 'JoyID' })).toHaveAttribute(
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
      render(<AddressMetrics holdings={objectsOnly} address={addressFixture} />);
      const holdings = screen.getByRole('region', { name: 'Metrics' }).querySelector('dd')!;
      expect(holdings).toHaveTextContent(/^827 objects · in 14 collections$/);
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
    expect(lines[0].right).toBe('1,000.00000000 CKB');
    expect(lines[2].left).toMatch(/^occupied .+ CKB · created #[\d,]+$/);
    expect(
      screen
        .getByRole('list', { name: 'Live cells, compact' })
        .querySelector('a[href*="hashType=data1&kind=type"]')
    ).not.toBeNull();
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
    expect(within(line as HTMLElement).getByText('JoyID')).toHaveAttribute(
      'data-role',
      'lock-kind'
    );
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
    expect(within(row).getByRole('link', { name: /Received 9 objects/ })).toHaveAttribute(
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
