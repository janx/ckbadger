// Readability 2026-11-18, lane B4 (pG) item 5, on the address page: its history lists this
// address's own line alone, and folds its consecutive rows that did the same thing and called the
// same script code (Gate B's rule, lists lane P3), counting this address and the other parties of
// each transaction though the row does not draw them.
import { describe, expect, it, vi } from 'vitest';
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
      /^all 3 transactions ↓ · 1 participant$/
    );
    expect(within(folded).queryByTitle(addressFixture.address!)).toBeNull();
    expect(folded.querySelector('[data-ckb-amount]')).toBeNull();
  });

  // The page-phase review's SHOULD 1 (the P3 follow-up): on an address's own page what was done
  // is received or sent. A CKB row was headed "CKB Transfer" both ways, so received 1,000, sent 250
  // and received 40 folded into one run that said neither; a CKB row is headed by its direction for
  // this address, as a token's is, and the two directions are two runs.
  it('heads a CKB row by its direction here, and folds received with received only', () => {
    const transfer = (n: number, ckbDelta: string): Activity => ({
      ...update(n),
      typeCalls: [],
      ckbDelta,
    });
    render(
      <AddressActivities
        rows={[
          transfer(1, '100000000000'),
          transfer(2, '-25000000000'),
          transfer(3, '4000000000'),
          transfer(4, '1000000000'),
        ]}
        address={addressFixture}
      />
    );
    const drawn = [
      ...document.querySelectorAll('[data-role="activity-run"], article[data-feed-row]'),
    ].map((el) =>
      el.getAttribute('data-role') === 'activity-run'
        ? `${el.querySelector('[data-role="headline"]')!.textContent} ×${el.getAttribute('data-run-count')}`
        : el.querySelector('[data-role="headline"]')!.textContent
    );
    expect(drawn).toEqual(['CKB received', 'CKB sent', 'CKB received ×2']);
  });

  // The third read's Lb SHOULD 2, ruled: the row carries only this address's deltas, so a burn read
  // "Token sent" and folded with plain sends, a mint "Token received" with transfers in. The route
  // states each item's movement in the whole transaction; the row is headed as every other feed
  // heads it ("Token burn", "Token mint"), and by direction only for a transfer.
  const moved = (n: number, delta: string, movement: 'mint' | 'burn' | 'transfer'): Activity => ({
    ...update(n),
    typeCalls: [],
    ckbDelta: '0',
    // the route's tag of a token row (TAG_TOKEN)
    tags: 1,
    itemDeltas: [
      {
        kind: 'token',
        typeScriptHash: `0x${'5e'.repeat(32)}`,
        delta,
        symbol: 'iCKB',
        decimals: 8,
        movement,
      },
    ],
  });
  const drawn = () =>
    [...document.querySelectorAll('[data-role="activity-run"], article[data-feed-row]')].map(
      (el) =>
        el.getAttribute('data-role') === 'activity-run'
          ? `${el.querySelector('[data-role="headline"]')!.textContent} ×${el.getAttribute('data-run-count')}`
          : el.querySelector('[data-role="headline"]')!.textContent
    );

  it('heads a burn and a mint as every feed does, a transfer by its direction, and folds none of them together', () => {
    render(
      <AddressActivities
        rows={[
          moved(1, '-10000000000', 'burn'),
          moved(2, '-5000000000', 'transfer'),
          moved(3, '1000000000', 'mint'),
          moved(4, '4000000000', 'transfer'),
        ]}
        address={addressFixture}
      />
    );
    expect(drawn()).toEqual(['Token burn', 'Token sent', 'Token mint', 'Token received']);
  });

  it('refuses an item an older API states no movement of, by name', () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    const { movement: _gone, ...older } = moved(1, '-100', 'burn').itemDeltas[0] as {
      movement: string;
    };
    expect(() =>
      render(
        <AddressActivities
          rows={[{ ...moved(1, '-100', 'burn'), itemDeltas: [older as never] }]}
          address={addressFixture}
        />
      )
    ).toThrow("an address activity's item: the API sends no movement: it is older than this page");
    spy.mockRestore();
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
      /^all 2 transactions ↓ · 3 participants$/
    );
  });
});
