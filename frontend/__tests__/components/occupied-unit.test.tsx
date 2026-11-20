// Readability 2026-11-18, lane B4 (pG), item 2: "occupied ±N" carries its unit, as a figure does by
// docs/RESPONSIVE_LAYOUT.md ("Formats": a figure writes the symbol, `B`). Occupied capacity is bytes
// (1 CKB of capacity holds 1 byte), so a change in it is a whole number of bytes; `formatOccupiedDelta`
// wrote "occupied −61" with no unit on the activity rows, the transaction's balance changes and its
// common-knowledge figure.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import { BalanceChanges } from '@/components/transaction/balance-changes';
import { ActivityRow } from '@/components/activity/activity-row';
import { formatOccupiedDelta } from '@/lib/occupied';
import type { ParticipantInfo } from '@/lib/api';
import { seededAddress } from '@/__tests__/utils/ckb-address';
import { activityRead } from '@/__tests__/utils/activity-read';

const MINUS = '−';

// Lane B4 second fix round (review-rules-fixes-logic NOTE 4, the coordinator's item 10): a change
// that is not whole bytes is refused naming the transaction it stands in, so the upstream fault is
// found from the message alone.
describe('an occupied change that is not whole bytes names its transaction', () => {
  it('in an activity row', () => {
    const a = feedSamples.objectTransfer;
    const bad = activityRead({
      ...a,
      participants: [{ ...a.participants[0], usedDelta: '150000001' }, ...a.participants.slice(1)],
    });
    expect(() => render(<ActivityRow activity={bad} />)).toThrow(
      new RegExp(`${bad.txHash}.*occupied change 150000001 shannons is not a whole number of bytes`)
    );
  });
});

describe('formatOccupiedDelta writes bytes with their unit (B4 item 2)', () => {
  it('writes a signed, grouped whole number of bytes and "B"', () => {
    expect(formatOccupiedDelta('-6100000000', 'tx 0xab')).toBe(`${MINUS}61\u00a0B`);
    expect(formatOccupiedDelta('32000000000', 'tx 0xab')).toBe('+320\u00a0B');
    expect(formatOccupiedDelta('123456700000000', 'tx 0xab')).toBe('+1,234,567\u00a0B');
    expect(formatOccupiedDelta('0', 'tx 0xab')).toBe('0\u00a0B');
  });

  it('refuses a change that is not whole bytes, naming it (occupied capacity is bytes)', () => {
    expect(() => formatOccupiedDelta('150000001', 'tx 0xab')).toThrow(
      /150000001.*not a whole number of bytes/
    );
  });
});

describe('the participant line says "occupied ±N B"', () => {
  it('carries the unit after the bytes', () => {
    const party: ParticipantInfo = {
      address: seededAddress('occ001'),
      lockHash: `0x${'01'.repeat(32)}`,
      lockHashPrefix: null,
      lockKind: 'Default Lock',
      roles: [],
      tags: 0,
      ckbDelta: '-100000000',
      usedDelta: '-6100000000',
      itemDeltas: [],
      sent: true,
    };
    const { container } = render(
      <ActivityRow
        activity={activityRead({
          txHash: `0x${'cd'.repeat(32)}`,
          blockNumber: 21000000,
          timestamp: String(Date.now() - 60_000),
          isCellbase: false,
          protocolActions: [],
          typeCalls: [],
          lockCalls: [],
          recognizedTypes: [],
          participants: [party, { ...party, lockHash: `0x${'02'.repeat(32)}`, ckbDelta: '1' }],
        })}
      />
    );
    const occupied = container.querySelector('[data-role="occupied"]') as HTMLElement;
    expect(occupied).toHaveTextContent(new RegExp(`^occupied ${MINUS}61 B$`));
    expect(occupied).toHaveClass('whitespace-nowrap');
  });
});

describe('the balance changes write occupied bytes with their unit, in every form', () => {
  const a = feedSamples.objectTransfer;
  afterEach(() => vi.unstubAllGlobals());

  it('in the table: each Occupied cell and the Net row, one unit each, never broken from it', () => {
    stubChartWidth(912);
    render(<BalanceChanges activity={a} fee="64119" compensation={null} />);
    const table = screen.getByRole('table', { name: 'Balance changes by participant' });
    const occupied = within(table)
      .getAllByRole('row')
      .slice(1)
      .map((r) => within(r).getAllByRole('cell')[2]);
    // B4 second fix round (re-pinned on purpose): figure and unit joined by a no-break space
    expect(occupied.map((c) => c.textContent)).toEqual([
      `${MINUS}502\u00a0B`,
      '+320\u00a0B',
      '+208\u00a0B',
      '+26\u00a0B',
    ]);
    for (const c of occupied) expect(c).toHaveClass('whitespace-nowrap');
  });

  it.each([358, 608])('in the cards at %i px: "occupied ±N B", the Net row too', (width) => {
    stubChartWidth(width);
    render(<BalanceChanges activity={a} fee="64119" compensation={null} />);
    const list = screen.getByRole('list', { name: 'Balance changes by participant' });
    expect(list).toHaveTextContent(`occupied ${MINUS}502 B`);
    expect(screen.getByTestId('tx-balance-net')).toHaveTextContent('occupied +26 B');
  });
});
