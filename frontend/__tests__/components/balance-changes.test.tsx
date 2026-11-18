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
import { BalanceChanges } from '@/components/transaction/balance-changes';
import { seededAddress } from '@/__tests__/utils/ckb-address';
import type { GlobalActivity } from '@/lib/api';

const UNNAMED_LOCK = `0x43b2924e${'00'.repeat(26)}895d`;
/** A lock the registry names (made up: the fixture names it "Nervape Shadow Lock"). */
const NERVAPE_SHADOW_LOCK = `0x${'ab'.repeat(32)}`;

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
      // B4 fix round (re-pinned on purpose): each party's lock is read off its address, so each
      // address spells the lock its fixture names (the sample's are all the default lock's)
      { ...from, lockKind: 'Default Lock', itemDeltas: [object(-1)] },
      {
        ...to,
        address: seededAddress('shp3hr', NERVAPE_SHADOW_LOCK),
        lockKind: 'Nervape Shadow Lock',
        itemDeltas: [object(1)],
      },
      { ...third, address: seededAddress('204m4g', UNNAMED_LOCK), lockKind: null },
    ],
  };
}

/** A sample party's address at the tight size's tail: its last 6 characters (R9). */
const tight = (i: number) => activity().participants[i].address!.slice(-6);

function renderAt(width: number) {
  stubChartWidth(width);
  const a = activity();
  return render(<BalanceChanges activity={a} fee="64119" compensation={null} />);
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
    // First line: the address and its lock, the lock never split. U-09 (re-pinned): the list
    // forms are phone rows, their ids at the tight size. B4 (re-pinned): the sample's real
    // address, and no lock beside it, the default lock (R9); the second party's lock is whole.
    expect(lines[0]).toHaveTextContent(new RegExp(`^ckb1…${tight(0)}$`));
    expect(lines[0].querySelector('[data-role="lock"]')).toBeNull();
    // B4 fix round (re-pinned): the lock's words are LockWordsView's, inside the card's lock slot
    expect(
      cards[1].querySelector(':scope > [data-line="party"] [data-role="lock"] [data-lock-kind]')
    ).toHaveClass('whitespace-nowrap');
    // Second line: the CKB change and the occupied change. The list forms are phone rows: the
    // summary form (readability R5, re-pinned on purpose: was "−528.00064119 CKB"), exact in its
    // title.
    expect(lines[1]).toHaveTextContent('−528 CKB');
    expect(lines[1].querySelector('[data-ckb-amount]')).toHaveAttribute(
      'title',
      '−528.00064119 CKB'
    );
    expect(lines[1]).toHaveTextContent('occupied −502');
    // Third line: the items sentence, in the feed's words.
    expect(lines[2]).toHaveTextContent('Nervape 3x 0x51c7…96f2 departed');
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
    // R9 (re-pinned): the two-line card is drawn at 1024 too, beside the tiles and the summary:
    // the row size, `ckb1…` and the last 8 (it was the tight size)
    // B4 (re-pinned): the sample's real address
    expect(card.querySelector('[data-role="party"]')).toHaveTextContent(
      `ckb1…${activity().participants[1].address!.slice(-8)}Nervape Shadow Lock`
    );
    // a phone row's summary form (R5, re-pinned on purpose: was "+320.00000000 CKB")
    expect(card.querySelector('[data-role="ckb"]')).toHaveTextContent('+320 CKB');
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
      // U-09 (re-pinned): tight in the phone forms (358, 608), row in the table (912); R9
      // (re-pinned): tight only in the three-line card (358), the two-line card (608) at the row
      // size
      // B4 (re-pinned): the default lock is named on no row (R9)
      expect(locks.map((l) => l.textContent)).toEqual([
        'Nervape Shadow Lock',
        width === 358 ? 'lock 0x43b2…895d' : 'lock 0x43b2924e…00895d',
      ]);
      expect(locks[1].querySelector('[data-lock-kind]')).toHaveAttribute('title', UNNAMED_LOCK);
    }
  );
});

// Readability B4 (R9): a shortened address no longer says its lock, so the lock is named beside it
// only when it is not the default lock; no name means the default lock.
describe('BalanceChanges names no lock for a party on the default lock (B4 item 1)', () => {
  it.each([358, 608, 912])('leaves the default-lock party unnamed at %i px', (width) => {
    renderAt(width);
    const locks = [...document.querySelectorAll('[data-role="lock"]')];
    expect(locks.map((l) => l.textContent)).not.toContain('Default Lock');
    expect(document.body).not.toHaveTextContent('Default Lock');
    expect(locks).toHaveLength(2);
  });
});
