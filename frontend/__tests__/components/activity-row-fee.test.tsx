// Readability 2026-11-18, lane B4 (pG), item 3: a participant line whose only movement is the fee
// is drawn in the reading ink, not the cinnabar, and says "fee". The review counted a first screen
// of /activities red with "−0.0003 CKB" lines (the .bit time oracles), though nothing went wrong,
// and a one-line "CKB Transfer" whose only line was its fee.
//
// A line's only movement is the fee when the row lists every party of the transaction with its
// change, no CKB entered it (not a cellbase, no Nervos DAO compensation), no item moved, and this
// party is the one whose CKB changed, downward: then CKB is conserved and its change is the fee
// exactly. Nothing is summed to find it.
import { describe, expect, it } from 'vitest';
import { render } from '@/__tests__/utils/test-utils';
import { seededAddress } from '@/__tests__/utils/ckb-address';
import { addressActivitiesFixture, addressFixture } from '@/__tests__/msw/fixtures/address';
import { inkOf, QUIET, READING } from '@/__tests__/utils/ink-role';
import { ActivityRow, type ActivityRowData } from '@/components/activity/activity-row';
import { AddressActivity } from '@/components/address/address-history';
import type { ItemDelta, ParticipantInfo } from '@/lib/api';

const MINUS = '−';

function party(
  seed: string,
  ckbDelta: string,
  over: Partial<ParticipantInfo> = {}
): ParticipantInfo {
  return {
    address: seededAddress(seed),
    lockHash: `0x${seed.padStart(64, '0')}`,
    lockHashPrefix: null,
    lockKind: 'Default Lock',
    roles: [],
    tags: 0,
    // a party whose CKB fell held an input
    sent: ckbDelta.startsWith('-'),
    ckbDelta,
    usedDelta: '0',
    itemDeltas: [],
    ...over,
  };
}

function row(
  participants: ParticipantInfo[],
  over: Partial<ActivityRowData> = {}
): ActivityRowData {
  return {
    txHash: `0x${'fe'.repeat(32)}`,
    blockNumber: 21047791,
    timestamp: String(Date.now() - 49_000),
    isCellbase: false,
    protocolActions: [],
    typeCalls: [],
    lockCalls: [],
    recognizedTypes: [],
    participants,
    ...over,
  };
}

const ckbOf = (c: HTMLElement, i = 0) =>
  c.querySelectorAll<HTMLElement>('[data-role="participant"] [data-role="ckb"]')[i];

describe('a line whose only movement is the fee (B4 item 3)', () => {
  it('says "fee" and the fee unsigned, in the reading ink, not the cinnabar', () => {
    // a .bit time oracle's update: one party, 0.0003 CKB out, nothing else moved
    const { container } = render(<ActivityRow activity={row([party('q3q7ue', '-30000')])} />);
    const ckb = ckbOf(container);
    expect(ckb).toHaveTextContent(/^fee 0\.0003 CKB$/);
    expect(ckb).toHaveAttribute('data-fee');
    const amount = ckb.querySelector('[data-ckb-amount]') as HTMLElement;
    expect(amount).not.toHaveClass('text-negative');
    expect(inkOf(amount)).toBe(READING);
    // "fee" is the amount's label: the quiet ink
    expect(inkOf(ckb.querySelector('[data-role="fee-label"]') as HTMLElement)).toBe(QUIET);
    expect(ckb).not.toHaveTextContent(MINUS);
  });

  it('is the fee whatever the party’s cells did to their occupancy', () => {
    // a party gathering its cells: the fee out, 61 bytes freed
    const { container } = render(
      <ActivityRow
        activity={row([party('gather', '-1000', { usedDelta: `-${61 * 1e8}` })], {
          lockCalls: [],
        })}
      />
    );
    expect(ckbOf(container)).toHaveTextContent(/^fee 0\.00001 CKB$/);
    expect(container.querySelector('[data-role="occupied"]')).toHaveTextContent(
      `occupied ${MINUS}61 B`
    );
  });

  it('is the fee beside parties whose change is zero', () => {
    const { container } = render(
      <ActivityRow activity={row([party('payer1', '-9030'), party('named1', '0')])} />
    );
    expect(ckbOf(container, 0)).toHaveTextContent(/^fee 0\.0000903 CKB$/);
    expect(ckbOf(container, 1)).not.toHaveAttribute('data-fee');
  });

  it('is not the fee when CKB went to another party', () => {
    const { container } = render(
      <ActivityRow activity={row([party('sendr1', '-100000000'), party('recvr1', '99970000')])} />
    );
    const payer = ckbOf(container, 0);
    expect(payer).not.toHaveAttribute('data-fee');
    expect(payer).toHaveTextContent(new RegExp(`^${MINUS}1 CKB$`));
    expect(payer.querySelector('[data-ckb-amount]')).toHaveClass('text-negative');
  });

  it('is not the fee when an item moved', () => {
    const token: ItemDelta = {
      kind: 'token',
      typeScriptHash: `0x${'77'.repeat(32)}`,
      delta: '0',
      symbol: 'RGB++',
      decimals: 8,
    };
    const { container } = render(
      <ActivityRow activity={row([party('tokenp', '-30000', { itemDeltas: [token] })])} />
    );
    expect(ckbOf(container)).not.toHaveAttribute('data-fee');
  });

  it('is not the fee where CKB entered: a Nervos DAO withdrawal’s compensation, a cellbase', () => {
    const withdrawal = row([party('daowd1', '-30000')], {
      protocolActions: [
        {
          protocol: 'dao',
          protocolName: 'Nervos DAO',
          action: 'withdraw_complete',
          metadata: { capacity: '10200000000', compensation: '1000' },
        },
      ],
    });
    const { container } = render(<ActivityRow activity={withdrawal} />);
    expect(ckbOf(container)).not.toHaveAttribute('data-fee');
    const { container: cellbase } = render(
      <ActivityRow activity={row([party('miner1', '-30000')], { isCellbase: true })} />
    );
    expect(ckbOf(cellbase)).not.toHaveAttribute('data-fee');
  });
});

describe('on an address page, the row lists this address alone', () => {
  const oracle = { ...addressActivitiesFixture[0], ckbDelta: '-30000', usedDelta: '0' };

  it('says the fee when the transaction has no other party', () => {
    const { container } = render(
      <AddressActivity address={addressFixture} activity={{ ...oracle, participants: [] }} />
    );
    expect(ckbOf(container)).toHaveTextContent(/^fee 0\.0003 CKB$/);
  });

  it('cannot say it when other parties are unlisted: their changes are not on the row', () => {
    const other = {
      address: seededAddress('other1'),
      lockHash: `0x${'0e'.repeat(32)}`,
      lockHashPrefix: null,
      roles: [],
      sent: false,
    };
    const { container } = render(
      <AddressActivity address={addressFixture} activity={{ ...oracle, participants: [other] }} />
    );
    expect(ckbOf(container)).not.toHaveAttribute('data-fee');
    expect(ckbOf(container)).toHaveTextContent(`${MINUS}0.0003 CKB`);
  });
});
