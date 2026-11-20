/**
 * "What moved" (tx page, review 2026-11-03 v2): one instrument of flows, each from the left (the
 * givers) to the right (the receivers) through the asset's plate; items first, CKB last; the fee
 * to the miner, compensation in gold; ribbon weight by share; six parties then a fold; the list
 * form under 700 px; hovering a party or a plate tells the page.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { atViewportWidth } from '@/__tests__/utils/viewport';
import { WhatMoved, type WhatMovedHover } from '@/components/transaction/what-moved';
import { seededAddress } from '@/__tests__/utils/ckb-address';
import type { GlobalActivity, ItemDelta } from '@/lib/api';
import { activityRead, ITEM_TYPE, type WrittenParty } from '@/__tests__/utils/activity-read';
import { typeStepOf } from '@/__tests__/utils/type-step-of';

const HASH = `0x${'54'.repeat(32)}`;
/** JoyID's code hash, the lock these parties' addresses spell (`ckb1qrgqep…`). */
const JOYID = '0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323';
/** A lock the registry does not name (the party whose `lockKind` is null). */
const UNNAMED_LOCK = `0x43b2924e${'00'.repeat(26)}895d`;
const NERVAPE = `0x4a2237a7${'00'.repeat(26)}2abf1d`;
const RGBPP = `0x${'2b'.repeat(32)}`;

/**
 * A fixture party's address as a row writes it, by its seed: `ckb1…` and its last 8 characters
 * (`tight`: 6). B4 fix round: real addresses, whose last characters are their checksum.
 */
const row = (tail: string, lockKind: string | null = 'JoyID') =>
  `ckb1…${seededAddress(tail, lockKind === null ? UNNAMED_LOCK : JOYID).slice(-8)}`;
const tight = (tail: string, lockKind: string | null = 'JoyID') =>
  `ckb1…${seededAddress(tail, lockKind === null ? UNNAMED_LOCK : JOYID).slice(-6)}`;

function party(
  tail: string,
  ckbDelta: string,
  itemDeltas: ItemDelta[] = [],
  lockKind: string | null = 'JoyID'
): WrittenParty {
  return {
    // B4 fix round: a real address on the lock the fixture names (JoyID, else the unnamed lock)
    address: seededAddress(tail, lockKind === null ? UNNAMED_LOCK : JOYID),
    // 32 bytes of hex, as the API sends a lock hash (lane exactness, part B6: a DAO action's owner
    // is read as one): the tail's characters as hex, padded
    lockHash: `0x${[...tail]
      .map((c) => c.charCodeAt(0).toString(16).padStart(2, '0'))
      .join('')
      .padStart(64, '0')}`,
    lockHashPrefix: null,
    roles: [],
    lockKind,
    ckbDelta,
    usedDelta: '0',
    itemDeltas,
    tags: itemDeltas.length ? 2 : 0,
  };
}
const suit = (delta: number): ItemDelta => ({
  kind: 'object',
  objectId: NERVAPE,
  delta,
  collection: { id: `0x${'ff'.repeat(32)}`, name: 'Nervape Suit' },
  class: 'object',
  composition: 'btc_ckb',
});
const rgbpp = (delta: string): ItemDelta => ({
  kind: 'token',
  typeScriptHash: RGBPP,
  delta,
  symbol: 'RGB++',
  decimals: 8,
});
function activity(
  participants: WrittenParty[],
  // the row's item types as its caller states them, as the route would (the third read's T SHOULD-1)
  over: Pick<GlobalActivity, 'itemTypes'> = { itemTypes: [] }
): GlobalActivity {
  return activityRead({
    txHash: HASH,
    blockNumber: 20_835_685,
    txIndex: 1,
    timestamp: '1761448067000',
    isCellbase: false,
    protocolActions: [],
    typeCalls: [],
    lockCalls: [],

    itemTypesUnread: null,
    participants,
    ...over,
  });
}
/** 0x5451be2b…: the suit from the market lock to ay02wn; 1,070 CKB to ct4hxv from both. */
const sale = () =>
  activity(
    [
      party('ct4hxv', '107000000000'),
      party('ay02wn', '-63800008888', [suit(1)]),
      party('sp7v9m', '-43200000000', [suit(-1)], null),
    ],
    { itemTypes: [ITEM_TYPE.spore] }
  );

function renderAt(width: number, a = sale(), onHover?: (h: WhatMovedHover | null) => void) {
  stubChartWidth(width);
  return render(
    <WhatMoved hash={HASH} activity={a} fee="8888" compensation={null} onHover={onHover} />
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('WhatMoved, the ribbon form', () => {
  it('frames the flows, items first then CKB, from the left to the right', () => {
    renderAt(876);
    const frame = screen.getByTestId('tx-what-moved');
    expect(frame).toHaveAttribute('data-instrument-frame');
    expect(frame).toHaveAttribute('data-form', 'ribbons');
    // the frame's eyebrows say the direction once: from on the left, to on the right
    const eyebrows = frame.querySelectorAll('.eyebrow');
    expect(Array.from(eyebrows).map((e) => e.textContent)).toEqual(['from', 'to']);
    expect(screen.getByText('tap or hover a party or an asset')).toBeInTheDocument();
    const groups = within(frame).getAllByRole('group');
    expect(groups.map((g) => g.getAttribute('data-flow'))).toEqual([`object:${NERVAPE}`, 'ckb']);
    expect(groups[0]).toHaveAccessibleName('Nervape Suit from 1\u00a0party to 1\u00a0party');
    expect(groups[1]).toHaveAccessibleName('CKB from 2\u00a0parties to 2\u00a0parties');
  });

  it('names the object on its plate with its composition and id, and the party on each side', () => {
    renderAt(876);
    const [object] = screen.getAllByRole('group');
    const plate = object.querySelector('[data-flow-plate]')!;
    expect(plate).toHaveTextContent('Nervape Suit');
    expect(plate.querySelector('[data-composition="btc_ckb"]')).not.toBeNull();
    expect(
      within(plate as HTMLElement).getByRole('link', { name: '0x4a2237a7…2abf1d' })
    ).toHaveAttribute('href', `/mainnet/objects/${NERVAPE}`);
    // lane P1, the ink and size roles: the object's id is a link, at the plate's 13 px (it was 12)
    expect(
      typeStepOf(within(plate as HTMLElement).getByRole('link', { name: '0x4a2237a7…2abf1d' }))
    ).toBe('small');
    // the plate carries no total for a whole object
    expect(plate.querySelector('[data-flow-total]')).toBeNull();
    const from = object.querySelector('[data-flow-side="from"]')!;
    const to = object.querySelector('[data-flow-side="to"]')!;
    // U-09 (re-pinned): the ribbon's parties at the row size; R9 (re-pinned): 4…8
    expect(from).toHaveTextContent(row('sp7v9m', null));
    expect(to).toHaveTextContent(row('ay02wn'));
    expect(to).toHaveTextContent('JoyID');
    // a whole object is the ribbon's full weight on both sides
    expect(from.querySelector('[data-flow-end="party"]')).toHaveAttribute('data-weight', '22');
    expect(to.querySelector('[data-flow-end="party"]')).toHaveAttribute('data-weight', '22');
  });

  it('conserves the CKB flow: both givers on the left, the receiver and the miner on the right, exact', () => {
    renderAt(876);
    const [, ckb] = screen.getAllByRole('group');
    const from = ckb.querySelector('[data-flow-side="from"]')!;
    const to = ckb.querySelector('[data-flow-side="to"]')!;
    const ends = (side: Element) =>
      Array.from(side.querySelectorAll('[data-flow-end]')).map((e) => [
        e.getAttribute('data-flow-end'),
        e.getAttribute('data-weight'),
        e.textContent,
      ]);
    // U-15 (re-pinned): a lock the registry does not name is 'lock <code hash>'
    expect(ends(from)).toEqual([
      ['party', '14', `${row('ay02wn')}JoyID638.00008888 CKB`],
      ['party', '10', `${row('sp7v9m', null)}lock 0x43b2924e…00895d432.00000000 CKB`],
    ]);
    expect(ends(to)).toEqual([
      ['party', '22', `${row('ct4hxv')}JoyID1,070.00000000 CKB`],
      ['miner', '2', 'the miner fee0.00008888 CKB'],
    ]);
    // the plate's total is what left the givers, so the fee is inside it
    expect(ckb.querySelector('[data-flow-total]')).toHaveTextContent('1,070.00008888 CKB in all');
    // lane P1, the ink and size roles: the total is a value, at the plate's 13 px (it was 12)
    expect(typeStepOf(ckb.querySelector('[data-flow-total]')!)).toBe('small');
    // ribbons: one per end, the miner's dashed, a chevron on every receiver
    const svg = ckb.querySelector('svg')!;
    expect(svg.querySelectorAll('[data-ribbon]')).toHaveLength(4);
    expect(svg.querySelector('[data-ribbon="miner"] path')).toHaveAttribute(
      'stroke-dasharray',
      '5 4'
    );
    expect(svg.querySelectorAll('[data-ribbon="party"] path[fill="none"]')).toHaveLength(3);
    expect(svg.querySelectorAll('path:not([fill="none"])')).toHaveLength(2);
  });

  it('tells the page which party or asset is hovered, and nothing on a terminal', () => {
    const onHover = vi.fn();
    renderAt(876, sale(), onHover);
    const [object, ckb] = screen.getAllByRole('group');
    fireEvent.mouseEnter(object.querySelector('[data-flow-side="to"] [data-flow-end="party"]')!);
    expect(onHover).toHaveBeenLastCalledWith({
      t: 'party',
      party: expect.objectContaining({ address: seededAddress('ay02wn', JOYID) }),
    });
    fireEvent.mouseLeave(object.querySelector('[data-flow-side="to"] [data-flow-end="party"]')!);
    expect(onHover).toHaveBeenLastCalledWith(null);
    fireEvent.mouseEnter(object.querySelector('[data-flow-plate]')!.parentElement!);
    expect(onHover).toHaveBeenLastCalledWith({
      t: 'asset',
      asset: { kind: 'object', item: expect.objectContaining({ objectId: NERVAPE }) },
    });
    onHover.mockClear();
    fireEvent.mouseEnter(ckb.querySelector('[data-flow-end="miner"]')!);
    expect(onHover).not.toHaveBeenCalled();
  });

  it('lets DAO compensation in first, in gold, and a token swap read as two opposite flows', () => {
    const a = activity(
      [
        party('aaaaaa', '-3070091791503', [rgbpp('2544240236569')]),
        // the receiver gets the giver's CKB plus the compensation, less the fee
        party('bbbbbb', '3087622394970', [rgbpp('-2544240236569')]),
      ],
      { itemTypes: [ITEM_TYPE.xudt] }
    );
    stubChartWidth(876);
    render(<WhatMoved hash={HASH} activity={a} fee="30698" compensation={BigInt('17530634165')} />);
    const [token, ckb] = screen.getAllByRole('group');
    expect(token.querySelector('[data-flow-side="from"]')).toHaveTextContent(row('bbbbbb'));
    expect(token.querySelector('[data-flow-side="to"]')).toHaveTextContent(row('aaaaaa'));
    expect(token.querySelector('[data-flow-side="to"]')).toHaveTextContent('25,442.40236569 RGB++');
    expect(ckb.querySelector('[data-flow-side="from"]')).toHaveTextContent(row('aaaaaa'));
    expect(ckb.querySelector('[data-flow-side="to"]')).toHaveTextContent(row('bbbbbb'));
    const first = ckb.querySelector('[data-flow-side="from"] [data-flow-end]')!;
    expect(first).toHaveAttribute('data-flow-end', 'compensation');
    expect(first).toHaveTextContent('Nervos DAO compensation175.30634165 CKB');
    expect(first.querySelector('.text-gold')).not.toBeNull();
    expect(ckb.querySelector('svg [data-ribbon="compensation"] path')).toHaveAttribute(
      'stroke',
      'var(--c-gold)'
    );
  });

  it('folds a side beyond six parties into five and "N more", which expands in place', () => {
    const givers = Array.from({ length: 9 }, (_, i) =>
      party(`g${String(i).padStart(5, '0')}`, String(-(1000 - i * 10)), [rgbpp(String(-(100 - i)))])
    );
    const taker = party(
      'taker1',
      String(givers.reduce((s, p) => s - Number(p.ckbDelta), 0) - 8888),
      [rgbpp(String(givers.reduce((s, p) => s + 100 - Number(p.itemDeltas[0].delta) - 100, 0)))]
    );
    const a = activity([...givers, taker], { itemTypes: [ITEM_TYPE.xudt] });
    renderAt(876, a);
    const [token] = screen.getAllByRole('group');
    const from = token.querySelector('[data-flow-side="from"]')!;
    expect(from.querySelectorAll('[data-flow-end="party"]')).toHaveLength(5);
    const more = within(from as HTMLElement).getByRole('button', { name: /4\smore\sparties/ });
    expect(more).toHaveTextContent('4 more parties expand');
    // the four folded give 95 + 94 + 93 + 92 raw units = 0.00000374 RGB++ at 8 decimals
    expect(more).toHaveTextContent('0.00000374 RGB++');
    expect(token).toHaveAccessibleName('RGB++ from 6\u00a0parties to 1\u00a0party');
    fireEvent.click(more);
    expect(from.querySelectorAll('[data-flow-end="party"]')).toHaveLength(9);
    expect(from.querySelector('[data-flow-end="more"]')).toBeNull();
  });
});

describe('WhatMoved, the list form', () => {
  // PA-11 (UI review 2026-11-14): the list form kept the ribbon form's header row, "FROM · tap or
  // hover a party or an asset · TO", above the stacked flow, so FROM was printed twice (the header,
  // then the flow's first group) and the header's TO stood over nothing. In the stacked form the
  // header keeps the hint alone; FROM and TO label each flow's two groups, once each.
  it('keeps only the hint in the frame’s header: from and to once each per flow (PA-11)', () => {
    const transfer = activity([party('aaaaaa', '-100008888'), party('bbbbbb', '100000000')]);
    renderAt(358, transfer);
    const frame = screen.getByTestId('tx-what-moved');
    expect(frame).toHaveAttribute('data-form', 'list');
    const header = screen.getByText('tap or hover a party or an asset').parentElement!;
    expect(header).toHaveTextContent(/^tap or hover a party or an asset$/);
    const [flow] = within(frame).getAllByRole('group');
    expect(within(frame).getAllByRole('group')).toHaveLength(1);
    expect(header.contains(flow)).toBe(false);
    expect(Array.from(frame.querySelectorAll('.eyebrow')).map((e) => e.textContent)).toEqual([
      'from',
      'to',
    ]);
    for (const eyebrow of frame.querySelectorAll<HTMLElement>('.eyebrow'))
      expect(flow).toContainElement(eyebrow);
  });

  it('lists each flow as from, a step down through the plate, to — and nothing scrolls sideways', () => {
    atViewportWidth(390);
    renderAt(358);
    const frame = screen.getByTestId('tx-what-moved');
    expect(frame).toHaveAttribute('data-form', 'list');
    expect(document.querySelector('.overflow-x-auto')).toBeNull();
    const [object, ckb] = within(frame).getAllByRole('group');
    expect(object).toHaveAttribute('data-form', 'list');
    expect(object.querySelector('svg[aria-hidden="true"] path[d="M6 0v15"]')).not.toBeNull();
    const eyebrows = Array.from(object.querySelectorAll('.eyebrow')).map((e) => e.textContent);
    expect(eyebrows).toEqual(['from', 'to']);
    // U-09 (re-pinned): the list's parties at the tight size, on a phone
    expect(object.querySelector('[data-flow-side="from"]')).toHaveTextContent(
      tight('sp7v9m', null)
    );
    expect(object.querySelector('[data-flow-side="to"]')).toHaveTextContent(tight('ay02wn'));
    expect(ckb.querySelectorAll('[data-flow-side="to"] [data-flow-end]')).toHaveLength(2);
    expect(ckb.querySelector('[data-flow-end="miner"]')).toHaveTextContent(
      'the miner fee0.00008888 CKB'
    );
  });
  // R9: the list is drawn at 1024 too (the transaction's 640 px column), beside the summary, the
  // balances and the tiles, which write the same parties at the row size: tight only below sm.
  it('writes the parties at the row size where the list is drawn from sm', () => {
    renderAt(640);
    const frame = screen.getByTestId('tx-what-moved');
    expect(frame).toHaveAttribute('data-form', 'list');
    const [object] = within(frame).getAllByRole('group');
    expect(object.querySelector('[data-flow-side="from"]')).toHaveTextContent(row('sp7v9m', null));
    expect(object.querySelector('[data-flow-side="to"]')).toHaveTextContent(row('ay02wn'));
  });
});

// Lane P1 follow-up (review chain-b tx-dao-deposit: "what moved draws the deposit as a payment to
// ckb1…vwd5rt DEFAULT LOCK 122,000; nowhere in the figure does it say Nervos DAO"). The activity
// says which party a DAO step is (its DAO tag, bit 3) and what the step is (its protocol action):
// the end of that party says it, so the ribbon to it does not read as a payment.
describe('WhatMoved, a Nervos DAO step', () => {
  const TAG_DAO_BIT = 8;
  const deposit = (): GlobalActivity =>
    activityRead({
      ...activity([
        party('ylvfrs', '-12200000088888'),
        { ...party('vwd5rt', '12200000000000'), tags: TAG_DAO_BIT },
      ]),
      itemTypes: [ITEM_TYPE.dao],
      protocolActions: [
        {
          protocol: 'dao',
          protocolName: 'Nervos DAO',
          action: 'deposit',
          // the deposit cell's owner (lane exactness, part B6)
          metadata: {
            capacity: '12200000000000',
            lockHash: party('vwd5rt', '0').lockHash!.slice(2),
          },
        },
      ],
    });

  it('says the end of the party that holds the deposit is a Nervos DAO deposit', () => {
    for (const width of [1000, 358]) {
      stubChartWidth(width);
      const { unmount } = render(
        <WhatMoved hash={HASH} activity={deposit()} fee="88888" compensation={null} />
      );
      const frame = screen.getByTestId('tx-what-moved');
      const to = frame.querySelector('[data-flow-side="to"]') as HTMLElement;
      const from = frame.querySelector('[data-flow-side="from"]') as HTMLElement;
      const holder = [...to.querySelectorAll<HTMLElement>('[data-flow-end="party"]')].find((e) =>
        // its address's last six characters, in the row and the tight size alike
        e.textContent!.includes(seededAddress('vwd5rt', JOYID).slice(-6))
      )!;
      expect(holder).toHaveTextContent('Nervos DAO deposit');
      // re-pinned (lane P1, the lead's look after the third round): the step on its own line with
      // its own amount, which here is also what reached the holder
      expect(holder.querySelector('[data-dao-step]')).toHaveTextContent(
        /^Nervos DAO deposit 122,000 CKB$/
      );
      expect(from).not.toHaveTextContent('Nervos DAO');
      unmount();
      vi.unstubAllGlobals();
    }
  });

  // Lane P1, the lead's look after the third round (tx 0xcb481014…610a at 1280): a deposit from the
  // owner's own address leaves its one party −fee, so its end read "0.00001124 CKB · Nervos DAO
  // deposit", a deposit of the fee's amount; the deposit is 50,000 CKB and never left its owner.
  // The step stands on a line of its own with its own amount, apart from what flowed to the miner.
  it('says a self-deposit’s own amount apart from the fee that flowed', () => {
    stubChartWidth(1000);
    const selfDeposit: GlobalActivity = activityRead({
      ...activity([{ ...party('vwd5rt', '-1124'), tags: TAG_DAO_BIT }]),
      itemTypes: [ITEM_TYPE.dao],
      protocolActions: [
        {
          protocol: 'dao',
          protocolName: 'Nervos DAO',
          action: 'deposit',
          metadata: {
            capacity: '5000000000000',
            lockHash: party('vwd5rt', '0').lockHash!.slice(2),
          },
        },
      ],
    });
    render(<WhatMoved hash={HASH} activity={selfDeposit} fee="1124" compensation={null} />);
    const frame = screen.getByTestId('tx-what-moved');
    const owner = frame.querySelector('[data-flow-side="from"] [data-flow-end="party"]')!;
    const step = owner.querySelector('[data-dao-step]')!;
    expect(step).toHaveTextContent(/^Nervos DAO deposit 50,000 CKB$/);
    // what flowed, the fee, on its own line, never beside the step
    const flowed = owner.querySelector('[data-ckb-amount]')!.closest('div')!;
    expect(flowed).toHaveTextContent(/^0\.00001124 CKB$/);
    expect(flowed).not.toContainElement(step as HTMLElement);
    expect(frame.querySelector('[data-flow-end="miner"]')).toHaveTextContent(
      'the miner fee0.00001124 CKB'
    );
  });

  // lane exactness, part B6 with the pages reader's NOTE 3: a withdrawal request whose fee another party paid leaves
  // its owner's CKB unchanged, so the owner is no end of the CKB flow; its step is drawn for it all
  // the same, its owner named (it drew only "the fee to the miner" and no step)
  it('draws the step of an owner whose CKB did not change', () => {
    stubChartWidth(1000);
    const request: GlobalActivity = activityRead({
      ...activity([party('ylvfrs', '-1124'), { ...party('vwd5rt', '0'), tags: TAG_DAO_BIT }]),
      itemTypes: [ITEM_TYPE.dao],
      protocolActions: [
        {
          protocol: 'dao',
          protocolName: 'Nervos DAO',
          action: 'withdraw_request',
          metadata: {
            capacity: '2605000000000',
            depositBlock: '19695550',
            lockHash: party('vwd5rt', '0').lockHash!.slice(2),
          },
        },
      ],
    });
    render(<WhatMoved hash={HASH} activity={request} fee="1124" compensation={null} />);
    const frame = screen.getByTestId('tx-what-moved');
    const unmoved = frame.querySelector('[data-dao-steps-unmoved]') as HTMLElement;
    expect(unmoved).not.toBeNull();
    expect(unmoved).toHaveTextContent(seededAddress('vwd5rt', JOYID).slice(-6));
    expect(unmoved.querySelector('[data-dao-step]')).toHaveTextContent(
      /^Nervos DAO withdrawal request 26,050 CKB$/
    );
  });
});
