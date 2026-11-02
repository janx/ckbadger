import { onTestFinished, vi } from 'vitest';
import { fireEvent } from '@testing-library/react';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { AddressBalance } from '@/components/address/address-holdings';
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
    expect(screen.getByLabelText('Address capacity partition')).toHaveTextContent('39.00000001');
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
