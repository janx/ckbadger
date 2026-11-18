// Readability 2026-11-18, lane B4 (pG) item 5, on the address page: its history lists this
// address's own line alone, and folds its consecutive rows that say the same thing, the other
// parties of each transaction compared though the row does not draw them.
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

  it('folds its consecutive same rows, written for this address', () => {
    render(<AddressActivities rows={[update(1), update(2), update(3)]} address={addressFixture} />);
    const folded = document.querySelector('[data-role="activity-run"]') as HTMLElement;
    expect(folded).toHaveAttribute('data-run-count', '3');
    // this address, as text (consistency C3), never a link to its own page
    expect(within(folded).getByTitle(addressFixture.address!)).toHaveClass('this-address');
    expect(folded.querySelector('[data-ckb-amount]')).toBeNull();
  });

  it('does not fold rows whose other parties differ, though the row lists this address alone', () => {
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
    expect(document.querySelector('[data-role="activity-run"]')).toBeNull();
  });
});
