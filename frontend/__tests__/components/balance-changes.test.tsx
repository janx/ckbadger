/**
 * tx "Balance changes" (responsive plan T1, board T1-balance-changes.dc.html): the four-column
 * table only when the measured container holds its 700 px; below that one card per participant
 * (three lines on a phone, two from 480 px), the Net row the last card, never a sideways scroller.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import { BalanceChanges, lockCodeHashesByAddress } from '@/components/transaction/balance-changes';
import type { GlobalActivity } from '@/lib/api';

const UNNAMED_LOCK = `0x43b2924e${'00'.repeat(26)}895d`;
const DEFAULT_LOCK = '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8';

/** The board's object transfer: two named locks, the third one unnamed in the registry. */
function activity(): GlobalActivity {
  const object = (delta: number) => ({
    kind: 'object' as const,
    objectId: `0x51c75a${'00'.repeat(26)}96f2`,
    delta,
    collection: { id: `0x${'cc'.repeat(32)}`, name: 'Nervape 3x' },
    class: 'object' as const,
    composition: 'btc_ckb' as const,
  });
  const [from, to, third] = feedSamples.objectTransfer.participants;
  return {
    ...feedSamples.objectTransfer,
    participants: [
      { ...from, lockKind: 'Default Lock', itemDeltas: [object(-1)] },
      { ...to, lockKind: 'Nervape Shadow Lock', itemDeltas: [object(1)] },
      { ...third, lockKind: null },
    ],
  };
}

/** The transaction's own cells: each party's lock script, by its address. */
function codeHashes(a: GlobalActivity) {
  const [from, to, third] = a.participants;
  return lockCodeHashesByAddress([
    { address: from.address!, lock: { codeHash: DEFAULT_LOCK, hashType: 'type', args: '0x01' } },
    {
      address: to.address!,
      lock: { codeHash: `0x${'ab'.repeat(32)}`, hashType: 'type', args: '0x' },
    },
    { address: third.address!, lock: { codeHash: UNNAMED_LOCK, hashType: 'type', args: '0x02' } },
  ]);
}

function renderAt(width: number) {
  stubChartWidth(width);
  const a = activity();
  return render(
    <BalanceChanges
      activity={a}
      fee="64119"
      compensation={null}
      genesis={false}
      lockCodeHashes={codeHashes(a)}
    />
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('BalanceChanges form by measured width', () => {
  it('keeps the four-column table when the container holds every column (912 px at 1280)', () => {
    renderAt(912);
    const table = screen.getByRole('table', { name: 'Balance changes by participant' });
    expect(
      within(table)
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual(['Participant', 'CKB', 'Occupied', 'Items']);
    expect(within(table).getAllByRole('row')).toHaveLength(5);
    // The table is drawn only where it fits, so nothing around it scrolls sideways.
    expect(document.querySelector('.overflow-x-auto')).toBeNull();
  });

  it('renders one three-line card per participant at 358 px, and no sideways scroller', () => {
    renderAt(358);
    expect(screen.queryByRole('table')).toBeNull();
    expect(document.querySelector('.overflow-x-auto')).toBeNull();
    expect(document.querySelector('.min-w-\\[700px\\]')).toBeNull();
    const list = screen.getByRole('list', { name: 'Balance changes by participant' });
    expect(list).toHaveAttribute('data-form', 'three-line');
    const cards = list.querySelectorAll('[data-balance-card]');
    expect(cards).toHaveLength(3);

    const lines = cards[0].querySelectorAll(':scope > [data-line]');
    expect([...lines].map((l) => l.getAttribute('data-line'))).toEqual([
      'party',
      'amounts',
      'items',
    ]);
    // First line: the address and its lock, the lock never split ("Default / Lock").
    expect(lines[0]).toHaveTextContent('ckb1qzda...cjk9lzDefault Lock');
    expect(lines[0].querySelector('[data-role="lock"]')).toHaveClass('whitespace-nowrap');
    // Second line: the CKB change and the occupied change.
    expect(lines[1]).toHaveTextContent('−528.00064119 CKB');
    expect(lines[1]).toHaveTextContent('occupied −502');
    // Third line: the items sentence, in the feed's words.
    expect(lines[2]).toHaveTextContent('Nervape 3x 0x51c75a...96f2 departed');
  });

  it('puts the amount on the first line and occupied on the second from 480 px (608 px at 640)', () => {
    renderAt(608);
    expect(screen.queryByRole('table')).toBeNull();
    const list = screen.getByRole('list', { name: 'Balance changes by participant' });
    expect(list).toHaveAttribute('data-form', 'two-line');
    const card = list.querySelectorAll('[data-balance-card]')[1];
    expect(
      [...card.querySelectorAll(':scope > [data-role]')].map((c) => c.getAttribute('data-role'))
    ).toEqual(['party', 'ckb', 'items', 'occupied']);
    expect(card).toHaveClass('grid', 'grid-cols-[minmax(0,1fr)_auto]');
    expect(card.querySelector('[data-role="party"]')).toHaveTextContent(
      'ckb1qzda...shp3hrNervape Shadow Lock'
    );
    expect(card.querySelector('[data-role="ckb"]')).toHaveTextContent('+320.00000000 CKB');
    expect(card.querySelector('[data-role="occupied"]')).toHaveTextContent('occupied +320');
  });

  it.each([358, 608])('keeps the Net row as the last card at %i px', (width) => {
    renderAt(width);
    const list = screen.getByRole('list', { name: 'Balance changes by participant' });
    const net = screen.getByTestId('tx-balance-net');
    expect(list.lastElementChild).toBe(net);
    expect(net).toHaveTextContent('Net −0.00064119 CKB · occupied +26');
    expect(net).toHaveTextContent('the fee, paid to the miner');
  });

  it.each([358, 608, 912])(
    'names a lock without a registry name by its code hash at %i px',
    (width) => {
      renderAt(width);
      const locks = [...document.querySelectorAll('[data-role="lock"]')];
      expect(locks.map((l) => l.textContent)).toEqual([
        'Default Lock',
        'Nervape Shadow Lock',
        'lock 0x43b2...895d',
      ]);
      expect(locks[2]).toHaveAttribute('title', UNNAMED_LOCK);
    }
  );
});

describe('lockCodeHashesByAddress', () => {
  it('reads each address its lock code hash from the transaction cells', () => {
    const map = lockCodeHashesByAddress([
      { address: 'ckb1a', lock: { codeHash: '0x01', hashType: 'type', args: '0x' } },
      { address: 'ckb1a', lock: { codeHash: '0x01', hashType: 'type', args: '0x' } },
      { address: 'ckb1b', lock: { codeHash: '0x02', hashType: 'data1', args: '0x' } },
    ]);
    expect([...map]).toEqual([
      ['ckb1a', '0x01'],
      ['ckb1b', '0x02'],
    ]);
  });

  it('refuses one address with two lock code hashes (an address encodes its lock)', () => {
    expect(() =>
      lockCodeHashesByAddress([
        { address: 'ckb1a', lock: { codeHash: '0x01', hashType: 'type', args: '0x' } },
        { address: 'ckb1a', lock: { codeHash: '0x02', hashType: 'type', args: '0x' } },
      ])
    ).toThrow('address ckb1a carries two lock code hashes: 0x01 and 0x02');
  });
});
