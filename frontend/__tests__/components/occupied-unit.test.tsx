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
import { BalanceChanges, lockCodeHashesByAddress } from '@/components/transaction/balance-changes';
import { ActivityRow, formatOccupiedDelta } from '@/components/activity/activity-row';
import type { ParticipantInfo } from '@/lib/api';
import { seededAddress } from '@/__tests__/utils/ckb-address';

const MINUS = '−';

describe('formatOccupiedDelta writes bytes with their unit (B4 item 2)', () => {
  it('writes a signed, grouped whole number of bytes and "B"', () => {
    expect(formatOccupiedDelta('-6100000000')).toBe(`${MINUS}61 B`);
    expect(formatOccupiedDelta('32000000000')).toBe('+320 B');
    expect(formatOccupiedDelta('123456700000000')).toBe('+1,234,567 B');
    expect(formatOccupiedDelta('0')).toBe('0 B');
  });

  it('refuses a change that is not whole bytes, naming it (occupied capacity is bytes)', () => {
    expect(() => formatOccupiedDelta('150000001')).toThrow(
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
    };
    const { container } = render(
      <ActivityRow
        activity={{
          txHash: `0x${'cd'.repeat(32)}`,
          blockNumber: 21000000,
          timestamp: String(Date.now() - 60_000),
          isCellbase: false,
          protocolActions: [],
          typeCalls: [],
          lockCalls: [],
          participants: [party, { ...party, lockHash: `0x${'02'.repeat(32)}`, ckbDelta: '1' }],
        }}
      />
    );
    const occupied = container.querySelector('[data-role="occupied"]') as HTMLElement;
    expect(occupied).toHaveTextContent(new RegExp(`^occupied ${MINUS}61 B$`));
    expect(occupied).toHaveClass('whitespace-nowrap');
  });
});

describe('the balance changes write occupied bytes with their unit, in every form', () => {
  const a = feedSamples.objectTransfer;
  const hashes = () =>
    lockCodeHashesByAddress(
      a.participants.map((p) => ({
        address: p.address!,
        lock: { codeHash: `0x${'ab'.repeat(32)}`, hashType: 'type', args: '0x' },
      }))
    );
  afterEach(() => vi.unstubAllGlobals());

  it('in the table: each Occupied cell and the Net row, one unit each, never broken from it', () => {
    stubChartWidth(912);
    render(
      <BalanceChanges activity={a} fee="64119" compensation={null} lockCodeHashes={hashes()} />
    );
    const table = screen.getByRole('table', { name: 'Balance changes by participant' });
    const occupied = within(table)
      .getAllByRole('row')
      .slice(1)
      .map((r) => within(r).getAllByRole('cell')[2]);
    expect(occupied.map((c) => c.textContent)).toEqual([
      `${MINUS}502 B`,
      '+320 B',
      '+208 B',
      '+26 B',
    ]);
    for (const c of occupied) expect(c).toHaveClass('whitespace-nowrap');
  });

  it.each([358, 608])('in the cards at %i px: "occupied ±N B", the Net row too', (width) => {
    stubChartWidth(width);
    render(
      <BalanceChanges activity={a} fee="64119" compensation={null} lockCodeHashes={hashes()} />
    );
    const list = screen.getByRole('list', { name: 'Balance changes by participant' });
    expect(list).toHaveTextContent(`occupied ${MINUS}502 B`);
    expect(screen.getByTestId('tx-balance-net')).toHaveTextContent('occupied +26 B');
  });
});
