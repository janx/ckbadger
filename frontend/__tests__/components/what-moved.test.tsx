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
import { WhatMoved, type WhatMovedHover } from '@/components/transaction/what-moved';
import { lockCodeHashesByAddress } from '@/components/transaction/balance-changes';
import type { GlobalActivity, ItemDelta, ParticipantInfo } from '@/lib/api';

const HASH = `0x${'54'.repeat(32)}`;
const DEFAULT_LOCK = '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8';
const NERVAPE = `0x4a2237a7${'00'.repeat(26)}2abf1d`;
const RGBPP = `0x${'2b'.repeat(32)}`;

function party(
  tail: string,
  ckbDelta: string,
  itemDeltas: ItemDelta[] = [],
  lockKind: string | null = 'JoyID'
): ParticipantInfo {
  return {
    address: `ckb1qrgqep8saj8agswr30pls73hra28ry8jlnlc3ejzh3dl2ju7xxpjxqgqq${tail}`,
    lockHash: `0x${tail.padStart(64, '0')}`,
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
function activity(participants: ParticipantInfo[]): GlobalActivity {
  return {
    txHash: HASH,
    blockNumber: 20_835_685,
    txIndex: 1,
    timestamp: '1761448067000',
    isCellbase: false,
    protocolActions: [],
    typeCalls: [],
    lockCalls: [],
    participants,
  };
}
/** 0x5451be2b…: the suit from the market lock to ay02wn; 1,070 CKB to ct4hxv from both. */
const sale = () =>
  activity([
    party('ct4hxv', '107000000000'),
    party('ay02wn', '-63800008888', [suit(1)]),
    party('sp7v9m', '-43200000000', [suit(-1)], null),
  ]);
const codeHashes = (a: GlobalActivity) =>
  lockCodeHashesByAddress(
    a.participants.map((p) => ({
      address: p.address!,
      lock: { codeHash: DEFAULT_LOCK, hashType: 'type', args: '0x' },
    }))
  );

function renderAt(width: number, a = sale(), onHover?: (h: WhatMovedHover | null) => void) {
  stubChartWidth(width);
  return render(
    <WhatMoved
      hash={HASH}
      activity={a}
      fee="8888"
      compensation={null}
      lockCodeHashes={codeHashes(a)}
      onHover={onHover}
    />
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
    expect(groups[0]).toHaveAccessibleName('Nervape Suit from 1 party to 1 party');
    expect(groups[1]).toHaveAccessibleName('CKB from 2 parties to 2 parties');
  });

  it('names the object on its plate with its composition and id, and the party on each side', () => {
    renderAt(876);
    const [object] = screen.getAllByRole('group');
    const plate = object.querySelector('[data-flow-plate]')!;
    expect(plate).toHaveTextContent('Nervape Suit');
    expect(plate.querySelector('[data-composition="btc_ckb"]')).not.toBeNull();
    expect(
      within(plate as HTMLElement).getByRole('link', { name: '0x4a2237a7...2abf1d' })
    ).toHaveAttribute('href', `/mainnet/objects/${NERVAPE}`);
    // the plate carries no total for a whole object
    expect(plate.querySelector('[data-flow-total]')).toBeNull();
    const from = object.querySelector('[data-flow-side="from"]')!;
    const to = object.querySelector('[data-flow-side="to"]')!;
    expect(from).toHaveTextContent('ckb1qrgq...sp7v9m');
    expect(to).toHaveTextContent('ckb1qrgq...ay02wn');
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
    expect(ends(from)).toEqual([
      ['party', '14', 'ckb1qrgq...ay02wnJoyID638.00008888 CKB'],
      ['party', '10', 'ckb1qrgq...sp7v9m0x9bd7e0...cce8432.00000000 CKB'],
    ]);
    expect(ends(to)).toEqual([
      ['party', '22', 'ckb1qrgq...ct4hxvJoyID1,070.00000000 CKB'],
      ['miner', '2', 'the miner fee0.00008888 CKB'],
    ]);
    // the plate's total is what left the givers, so the fee is inside it
    expect(ckb.querySelector('[data-flow-total]')).toHaveTextContent('1,070.00008888 CKB in all');
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
      party: expect.objectContaining({ address: expect.stringMatching(/ay02wn$/) }),
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
    const a = activity([
      party('aaaaaa', '-3070091791503', [rgbpp('2544240236569')]),
      // the receiver gets the giver's CKB plus the compensation, less the fee
      party('bbbbbb', '3087622394970', [rgbpp('-2544240236569')]),
    ]);
    stubChartWidth(876);
    render(
      <WhatMoved
        hash={HASH}
        activity={a}
        fee="30698"
        compensation={BigInt('17530634165')}
        lockCodeHashes={codeHashes(a)}
      />
    );
    const [token, ckb] = screen.getAllByRole('group');
    expect(token.querySelector('[data-flow-side="from"]')).toHaveTextContent('bbbbbb');
    expect(token.querySelector('[data-flow-side="to"]')).toHaveTextContent('aaaaaa');
    expect(token.querySelector('[data-flow-side="to"]')).toHaveTextContent('25,442.40236569 RGB++');
    expect(ckb.querySelector('[data-flow-side="from"]')).toHaveTextContent('aaaaaa');
    expect(ckb.querySelector('[data-flow-side="to"]')).toHaveTextContent('bbbbbb');
    const first = ckb.querySelector('[data-flow-side="from"] [data-flow-end]')!;
    expect(first).toHaveAttribute('data-flow-end', 'compensation');
    expect(first).toHaveTextContent('Nervos DAO compensation175.30634165 CKB');
    expect(first.querySelector('.text-gold')).not.toBeNull();
    expect(ckb.querySelector('svg [data-ribbon="compensation"] path')).toHaveAttribute(
      'stroke',
      '#f2c55c'
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
    const a = activity([...givers, taker]);
    renderAt(876, a);
    const [token] = screen.getAllByRole('group');
    const from = token.querySelector('[data-flow-side="from"]')!;
    expect(from.querySelectorAll('[data-flow-end="party"]')).toHaveLength(5);
    const more = within(from as HTMLElement).getByRole('button', { name: /4 more parties/ });
    expect(more).toHaveTextContent('4 more parties expand');
    // the four folded give 95 + 94 + 93 + 92 raw units = 0.00000374 RGB++ at 8 decimals
    expect(more).toHaveTextContent('0.00000374 RGB++');
    expect(token).toHaveAccessibleName('RGB++ from 6 parties to 1 party');
    fireEvent.click(more);
    expect(from.querySelectorAll('[data-flow-end="party"]')).toHaveLength(9);
    expect(from.querySelector('[data-flow-end="more"]')).toBeNull();
  });
});

describe('WhatMoved, the list form', () => {
  it('lists each flow as from, a step down through the plate, to — and nothing scrolls sideways', () => {
    renderAt(358);
    const frame = screen.getByTestId('tx-what-moved');
    expect(frame).toHaveAttribute('data-form', 'list');
    expect(document.querySelector('.overflow-x-auto')).toBeNull();
    const [object, ckb] = within(frame).getAllByRole('group');
    expect(object).toHaveAttribute('data-form', 'list');
    expect(object.querySelector('svg[aria-hidden="true"] path[d="M6 0v15"]')).not.toBeNull();
    const eyebrows = Array.from(object.querySelectorAll('.eyebrow')).map((e) => e.textContent);
    expect(eyebrows).toEqual(['from', 'to']);
    expect(object.querySelector('[data-flow-side="from"]')).toHaveTextContent('ckb1qrgq...sp7v9m');
    expect(object.querySelector('[data-flow-side="to"]')).toHaveTextContent('ckb1qrgq...ay02wn');
    expect(ckb.querySelectorAll('[data-flow-side="to"] [data-flow-end]')).toHaveLength(2);
    expect(ckb.querySelector('[data-flow-end="miner"]')).toHaveTextContent(
      'the miner fee0.00008888 CKB'
    );
  });
});
