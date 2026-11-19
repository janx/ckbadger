// Readability 2026-11-18, lane B4 (pG) item 5, on the address page: its history lists this
// address's own line alone, and folds its consecutive rows that did the same thing and called the
// same script code (Gate B's rule, lists lane P3), counting this address and the other parties of
// each transaction though the row does not draw them.
import { describe, expect, it } from 'vitest';
import { render, within } from '@/__tests__/utils/test-utils';
import { seededAddress } from '@/__tests__/utils/ckb-address';
import { addressActivitiesFixture, addressFixture } from '@/__tests__/msw/fixtures/address';
import { AddressActivities } from '@/components/address/address-history';
import type { Activity } from '@/lib/api';

const NOW = Date.now();

describe('an address page folds its own runs (B4 item 5)', () => {
  const update = (n: number): Activity => ({
    ...addressActivitiesFixture[0],
    txHash: `0x${String(n).padStart(2, '0').repeat(32)}`,
    blockNumber: 21047791 - n,
    timestamp: String(NOW - n * 49_000),
    ckbDelta: '-30000',
    usedDelta: '0',
    typeCalls: [
      {
        typeCodeHash: `0x${'3a'.repeat(32)}`,
        typeHashType: 'type',
        typeArgs: '0x00',
        scriptHash: `0x${'3a'.repeat(31)}00`,
        scriptName: '.bit Time Index State',
      },
    ],
    participants: [],
  });

  // re-pinned on purpose (Gate B's rule, lane P3): the folded row wrote this address, the one
  // party of every row; it counts the parties now and names none
  it('folds its consecutive same rows, counting this address', () => {
    render(<AddressActivities rows={[update(1), update(2), update(3)]} address={addressFixture} />);
    const folded = document.querySelector('[data-role="activity-run"]') as HTMLElement;
    expect(folded).toHaveAttribute('data-run-count', '3');
    expect(folded.querySelector('[data-role="run-count"]')).toHaveTextContent(
      /^3 transactions ↓ · 1 participant$/
    );
    expect(within(folded).queryByTitle(addressFixture.address!)).toBeNull();
    expect(folded.querySelector('[data-ckb-amount]')).toBeNull();
  });

  // re-pinned on purpose (Gate B's rule, lane P3): "not the parties": rows whose other parties
  // differ fold, and the run counts them (it did not fold them by the old rule)
  it('folds rows whose other parties differ, and counts every party once', () => {
    const withOther = (n: number, seed: string): Activity => ({
      ...update(n),
      participants: [
        {
          address: seededAddress(seed),
          lockHash: `0x${seed.padStart(64, '0')}`,
          lockHashPrefix: null,
          roles: [],
        },
      ],
    });
    render(
      <AddressActivities
        rows={[withOther(1, 'aaaaaa'), withOther(2, 'bbbbbb')]}
        address={addressFixture}
      />
    );
    const folded = document.querySelector('[data-role="activity-run"]') as HTMLElement;
    expect(folded).toHaveAttribute('data-run-count', '2');
    // this address, and the two others
    expect(folded.querySelector('[data-role="run-count"]')).toHaveTextContent(
      /^2 transactions ↓ · 3 participants$/
    );
  });
});
