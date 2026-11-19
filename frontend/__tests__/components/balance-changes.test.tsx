/**
 * tx "Balance changes" (responsive plan T1, board T1-balance-changes.dc.html): the four-column
 * table only when the measured container holds its 700 px; below that one card per participant
 * (three lines on a phone, two from 480 px), the Net row the last card, never a sideways scroller.
 */
import { containerContentPx, gapPx } from '@/__tests__/utils/tailwind-box';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import { BalanceChanges } from '@/components/transaction/balance-changes';
import { seededAddress } from '@/__tests__/utils/ckb-address';
import type { GlobalActivity } from '@/lib/api';
import { WIDEST_OCCUPIED_CHANGE } from '@/lib/list-bounds';
import { fontPx, textPx } from '@/lib/type-metrics';
import { activityRead } from '@/__tests__/utils/activity-read';
import { inkOf, QUIET, READING } from '@/__tests__/utils/ink-role';

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
  return activityRead({
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
  });
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

  // Lane B4 fix round (review-rules-logic NOTE 4, the coordinator's item 14): the Occupied
  // column holds the widest change a party can make in one transaction, with its unit
  // (WIDEST_OCCUPIED_CHANGE, "−8,101,332,271 B", 16 glyphs; re-pinned on purpose in the second
  // fix round, the exact bound: it was "−16,773,724,928 B"), at the step its cells inherit from
  // the transaction page, the body step (8.4 px a glyph): 134.4 px. It was a 100 px literal,
  // which "−1,234,567 B" (100.8 px) already passed. The column is a grid track, read from the
  // rows the table draws (each row is its own grid, so every row is read).
  it('budgets the Occupied column at the widest change, with its unit, in every row', () => {
    renderAt(912);
    const table = screen.getByRole('table', { name: 'Balance changes by participant' });
    const tracks = (row: HTMLElement) => {
      const style = row.style.gridTemplateColumns;
      if (style) return style.split(' ');
      const cls = [...row.classList].find((c) => c.startsWith('grid-cols-['));
      return cls!.slice('grid-cols-['.length, -1).split('_');
    };
    const widest = textPx(WIDEST_OCCUPIED_CHANGE.length, fontPx('body'));
    expect(widest).toBeCloseTo(134.4, 10);
    const rows = within(table).getAllByRole('row');
    expect(rows).toHaveLength(5);
    for (const row of rows) {
      const occupied = tracks(row)[2];
      expect(occupied).toMatch(/^\d+px$/);
      expect(parseFloat(occupied)).toBeGreaterThanOrEqual(widest);
      // the cells inherit the page's body step: no step of their own
      const cell = row.children[2] as HTMLElement;
      expect(cell.className).not.toMatch(/\btext-(meta|small|body|lede)\b/);
    }
  });

  // B4 second fix round (tests SHOULD-2, review-rules-fixes-logic SHOULD 1; the coordinator's
  // item 3): the table is drawn where its tracks fit, the width rebuilt from the tracks its rows
  // write (each a length or a floor, the Items track's minmax) and the three 16 px gaps; one pixel
  // narrower draws the two-line cards. The 768 px window's column, 736 px (one column below lg,
  // 768 less the container's 16 px sides), keeps its table; 912 (1280) draws it, 608 (1024 and
  // 640, beside the tiles) the two-line cards, 358 (390) the three-line cards.
  it('draws the table from the width its tracks need, the cards one px below (736 at 768)', () => {
    renderAt(912);
    const row = within(
      screen.getByRole('table', { name: 'Balance changes by participant' })
    ).getAllByRole('row')[0];
    const tracks = row.style.gridTemplateColumns.split(/ (?![^(]*\))/);
    expect(tracks).toHaveLength(4);
    const floor = (track: string) => {
      const m = /^(\d+)px$/.exec(track) ?? /^minmax\((\d+)px, ?1fr\)$/.exec(track);
      if (!m) throw new Error(`a track this does not read: ${track}`);
      return Number(m[1]);
    };
    const need = tracks.map(floor).reduce((a, b) => a + b, 0) + 3 * gapPx(row, 912);
    const form = (width: number) => {
      cleanup();
      renderAt(width);
      return screen.queryByRole('table')
        ? 'table'
        : screen.getByRole('list').getAttribute('data-form');
    };
    expect(form(need)).toBe('table');
    expect(form(need - 1)).toBe('two-line');
    expect(need).toBeLessThanOrEqual(containerContentPx(768));
    expect(containerContentPx(768)).toBe(736);
    expect(form(736)).toBe('table');
    expect(form(912)).toBe('table');
    expect(form(608)).toBe('two-line');
    expect(form(358)).toBe('three-line');
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

  // lane P1, the ink and size roles: the net row's figures and its note are values and a
  // sentence, the reading ink; "Net" names them, quiet (the whole row was quiet)
  it.each([358, 912])('reads the Net row in the reading ink, its name quiet, at %i px', (width) => {
    renderAt(width);
    const net = screen.getByTestId('tx-balance-net');
    expect(inkOf(net)).toBe(READING);
    expect(inkOf(within(net).getByText('Net'))).toBe(QUIET);
    expect(inkOf(within(net).getByText(/the fee, paid to the miner/))).toBe(READING);
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

// Lane P1 follow-up (review chain-b tx-dao-deposit: the table's rows stood in the API's order, the
// largest change last and under the fold of the page, while the figure above sorts by size): the
// order a reader needs, who gave, then who got, each the largest first (the feed's order,
// lib/participant-order.ts), and the Net row with the fee last, in the table and the cards alike.
describe('BalanceChanges, the order a reader needs', () => {
  it('lists who gave, then who got, each the largest first, the Net row last', () => {
    for (const width of [912, 358]) {
      stubChartWidth(width);
      const a = activity();
      const [from, to, third] = a.participants;
      const shuffled = activityRead({ ...a, participants: [third, from, to] });
      const { unmount } = render(
        <BalanceChanges activity={shuffled} fee="64119" compensation={null} />
      );
      const rows =
        width >= 700
          ? within(screen.getByRole('table')).getAllByRole('row').slice(1)
          : within(screen.getByRole('list')).getAllByRole('listitem');
      const tails = [from, to, third].map((p) => p.address!.slice(-6));
      expect(rows.map((row) => tails.findIndex((tail) => row.textContent!.includes(tail)))).toEqual(
        [0, 1, 2, -1]
      );
      expect(rows[rows.length - 1]).toHaveTextContent(/^Net/);
      unmount();
      vi.unstubAllGlobals();
    }
  });

  // Lane P1, third round (review p-logic-chain NOTE 4: the table ordered parties by CKB alone, so a
  // party that gave only a token stood last, after the receivers): a party gave if its CKB or any
  // item left it, and it stands among the givers.
  it('counts a party that gave only an item among the givers', () => {
    stubChartWidth(912);
    const a = activity();
    const [from, to, third] = a.participants;
    // the fee's payer gave CKB alone; the object's giver and receiver kept their CKB
    const payer = { ...third, ckbDelta: '-64119', itemDeltas: [] };
    const giver = { ...from, ckbDelta: '0' };
    const receiver = { ...to, ckbDelta: '0' };
    render(
      <BalanceChanges
        activity={activityRead({ ...a, participants: [receiver, payer, giver] })}
        fee="64119"
        compensation={null}
      />
    );
    const rows = within(screen.getByRole('table')).getAllByRole('row').slice(1);
    const tails = [payer, giver, receiver].map((p) => p.address!.slice(-6));
    expect(rows.map((row) => tails.findIndex((tail) => row.textContent!.includes(tail)))).toEqual([
      0, 1, 2, -1,
    ]);
  });
});
