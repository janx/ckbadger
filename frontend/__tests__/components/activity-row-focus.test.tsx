// Consistency round 2026-11-02, lane F (C3): one activity row site-wide. `focusLockHash` draws the
// participant a page is about first, its address as text (the page is already its page), with its
// lock kind and, since material pass v2 (move 5), a jade underline instead of a "this address"
// tag; every other participant is drawn as before. A participant
// line whose items are all objects of one composition tier says it once, at the end of its items;
// mixed tiers, or a token / identity on the line, keep one badge per object. jsdom does not lay
// out, so these assert structure and order.
import { fireEvent, render, within } from '@/__tests__/utils/test-utils';
import { ActivityRow, type ActivityRowData } from '@/components/activity/activity-row';
import type { CompositionTier } from '@/lib/asset-utils';
import type { ItemDelta, ParticipantInfo } from '@/lib/api';

const TX = '0xa46efcd8' + 'ef'.repeat(26) + 'dac2e0';

function party(tail: string, over: Partial<ParticipantInfo> = {}): ParticipantInfo {
  return {
    address: `ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq${tail}`,
    lockHash: `0x${tail.padStart(64, '0')}`,
    lockHashPrefix: null,
    lockKind: 'Default Lock',
    roles: [],
    tags: 0,
    ckbDelta: '0',
    usedDelta: '0',
    itemDeltas: [],
    ...over,
  };
}

function object(n: number, delta: 1 | -1, composition: CompositionTier | null): ItemDelta {
  return {
    kind: 'object',
    objectId: `0x${n.toString(16).padStart(2, '0').repeat(32)}`,
    delta,
    collection: { id: '0x' + '77'.repeat(32), name: 'Nervape Special' },
    class: 'object',
    composition,
    role: 'spore',
  };
}

function row(participants: ParticipantInfo[]): ActivityRowData {
  return {
    txHash: TX,
    blockNumber: 17627981,
    timestamp: '1761174000000',
    isCellbase: false,
    protocolActions: [],
    typeCalls: [],
    lockCalls: [],
    participants,
  };
}

const sender = party('sendr1', { ckbDelta: '-326700000000', usedDelta: '-323100000000' });
const receiver = party('hprdnn', { ckbDelta: '326699990000', usedDelta: '323100000000' });
const transfer = row([sender, receiver]);

const lines = (c: HTMLElement) => [...c.querySelectorAll<HTMLElement>('[data-role="participant"]')];
const roles = (line: HTMLElement) => [...line.children].map((e) => e.getAttribute('data-role'));

describe('focusLockHash: the participant the page is about', () => {
  it('draws the focused participant first, before the CKB order would put it', () => {
    // Without a focus the payer leads (negative first); the receiver is this page's address.
    const { container: plain } = render(<ActivityRow activity={transfer} />);
    expect(lines(plain)[0]).toHaveTextContent('sendr1');
    const { container } = render(
      <ActivityRow activity={transfer} focusLockHash={receiver.lockHash!} />
    );
    const shown = lines(container);
    expect(shown).toHaveLength(2);
    expect(shown[0]).toHaveTextContent('hprdnn');
    expect(shown[1]).toHaveTextContent('sendr1');
  });

  it('writes its address as text with the full address in the hover, never a link to itself', () => {
    const { container } = render(
      <ActivityRow activity={transfer} focusLockHash={receiver.lockHash!} />
    );
    const [focus, other] = lines(container);
    const party = focus.querySelector('[data-role="party"]') as HTMLElement;
    expect(party).toHaveAttribute('data-party');
    expect(within(party).queryByRole('link')).toBeNull();
    expect(within(party).getByTitle(receiver.address!)).toHaveTextContent('ckb1qzda...hprdnn');
    expect(container.querySelector(`a[href="/mainnet/address/${receiver.address}"]`)).toBeNull();
    // Every other participant is unchanged: its address is the link to its page.
    expect(
      within(other.querySelector('[data-role="party"]') as HTMLElement).getByRole('link')
    ).toHaveAttribute('href', `/mainnet/address/${sender.address}`);
  });

  // Material pass v2 (move 5): the "this address" tag is gone — the party a page is about is
  // underlined jade (`.this-address` on its address text) and says "this address" to a screen
  // reader only; the lock kind follows as dim small caps, in the line order of the board.
  it('underlines it jade with its lock kind after it, saying "this address" to a reader only', () => {
    const { container } = render(
      <ActivityRow activity={transfer} focusLockHash={receiver.lockHash!} />
    );
    const [focus, other] = lines(container);
    expect(roles(focus)).toEqual(['party', 'lock-kind', 'ckb', 'occupied']);
    const party = focus.querySelector('[data-role="party"]') as HTMLElement;
    expect(party).toHaveAttribute('data-this-address', 'true');
    expect(within(party).getByTitle(receiver.address!)).toHaveClass('this-address');
    expect(within(party).getByText('this address')).toHaveClass('sr-only');
    expect(container.querySelector('[data-role="this-address"]')).toBeNull();
    expect(container.querySelector('[data-row-tag]')).toBeNull();
    // Only the focused line carries it.
    expect(within(other).queryByText('this address')).toBeNull();
    expect(other.querySelector('[data-role="party"]')).not.toHaveAttribute('data-this-address');
    expect(roles(other)).toEqual(['party', 'lock-kind', 'ckb', 'occupied']);
    expect(within(container).getAllByText('this address')).toHaveLength(1);
  });

  it('keeps every participant: the count of lines equals the participants, with or without a focus', () => {
    const many = row([
      sender,
      party('third1', { ckbDelta: '-1000' }),
      receiver,
      party('fourth', { ckbDelta: '0' }),
    ]);
    const { container: plain } = render(<ActivityRow activity={many} />);
    const { container } = render(
      <ActivityRow activity={many} focusLockHash={receiver.lockHash!} />
    );
    expect(lines(plain)).toHaveLength(4);
    expect(lines(container)).toHaveLength(4);
    expect(container.querySelectorAll('[data-party]')).toHaveLength(4);
    // The others keep their own order (negative by size, then zero).
    expect(lines(container).map((l) => l.textContent?.match(/\.\.\.(\w{6})/)?.[1])).toEqual([
      'hprdnn',
      'sendr1',
      'third1',
      'fourth',
    ]);
  });

  it('leads with the focus over six participants and folds the others as before', () => {
    const others = Array.from({ length: 7 }, (_, i) =>
      party(`p${i}x${i}x${i}`, { ckbDelta: `-${(i + 2) * 100000000}` })
    );
    // The focus moved the least CKB: the largest-five fold alone would hide it.
    const focus = party('hprdnn', { ckbDelta: '1' });
    const big = row([...others, focus]);
    const { container } = render(<ActivityRow activity={big} focusLockHash={focus.lockHash!} />);
    expect(lines(container)[0]).toHaveTextContent('hprdnn');
    expect(lines(container)).toHaveLength(6);
    fireEvent.click(within(container).getByRole('button', { name: '+2 participants' }));
    expect(lines(container)).toHaveLength(8);
    expect(lines(container)[0]).toHaveTextContent('hprdnn');
  });

  it('fails loudly when the focus is not a participant of the transaction', () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(<ActivityRow activity={transfer} focusLockHash={`0x${'99'.repeat(32)}`} />)
    ).toThrow(new RegExp(`${TX}.*0x${'99'.repeat(32)}`));
    spy.mockRestore();
  });
});

describe('composition badges: once per participant line when all its items share a tier', () => {
  const badges = (el: HTMLElement) => el.querySelectorAll('[data-composition]');

  it('says a shared tier once, after the items and before occupied', () => {
    const batch = row([
      party('hprdnn', {
        ckbDelta: '108900000000',
        usedDelta: '107700000000',
        itemDeltas: [object(1, 1, 'btc_ckb'), object(2, 1, 'btc_ckb'), object(3, 1, 'btc_ckb')],
      }),
    ]);
    const { container } = render(<ActivityRow activity={batch} />);
    const [line] = lines(container);
    expect(badges(line)).toHaveLength(1);
    expect(roles(line)).toEqual([
      'party',
      'lock-kind',
      'ckb',
      'item',
      'item',
      'item',
      'composition',
      'occupied',
    ]);
    expect(
      line.querySelector('[data-role="composition"] [data-composition="btc_ckb"]')
    ).not.toBeNull();
    for (const item of line.querySelectorAll('[data-role="item"]'))
      expect(item.querySelector('[data-composition]')).toBeNull();
  });

  it('keeps one badge per object when the tiers differ', () => {
    const mixed = row([
      party('hprdnn', {
        itemDeltas: [object(1, 1, 'btc_ckb'), object(2, 1, 'pure_ckb'), object(3, 1, null)],
      }),
    ]);
    const { container } = render(<ActivityRow activity={mixed} />);
    const [line] = lines(container);
    expect(line.querySelector('[data-role="composition"]')).toBeNull();
    expect([...badges(line)].map((b) => b.getAttribute('data-composition'))).toEqual([
      'btc_ckb',
      'pure_ckb',
    ]);
  });

  it('keeps the shared badge on a folded batch and after it opens', () => {
    const batch = row([
      party('hprdnn', {
        itemDeltas: [1, 2, 3, 4, 5].map((n) => object(n, 1, 'btc_ckb')),
      }),
    ]);
    const { container } = render(<ActivityRow activity={batch} foldObjects />);
    const [line] = lines(container);
    const more = within(line).getByRole('button', { name: '+4 more objects' });
    expect(badges(line)).toHaveLength(1);
    fireEvent.click(more);
    expect(line.querySelectorAll('a[href*="/objects/"]')).toHaveLength(5);
    expect(badges(line)).toHaveLength(1);
  });

  it('keeps the badge on the object when the line also moves a token or an identity, which carry no tier', () => {
    // A .cell mint: the deed object arrives and the name identity departs on one line. A badge at
    // the line's end would read as the identity's tier.
    const deed = row([
      party('hprdnn', {
        itemDeltas: [
          object(1, 1, 'pure_ckb'),
          { kind: 'identity', standard: 'dotcell', identityId: '0x' + 'f6'.repeat(20), delta: -1 },
        ],
      }),
    ]);
    const { container } = render(<ActivityRow activity={deed} />);
    const [line] = lines(container);
    expect(line.querySelector('[data-role="composition"]')).toBeNull();
    const items = [...line.querySelectorAll('[data-role="item"]')];
    expect(items.map((i) => i.querySelectorAll('[data-composition]').length)).toEqual([1, 0]);
  });

  it('draws no badge for objects that carry no tier', () => {
    const untiered = row([
      party('hprdnn', { itemDeltas: [object(1, 1, null), object(2, 1, null)] }),
    ]);
    const { container } = render(<ActivityRow activity={untiered} />);
    expect(badges(container)).toHaveLength(0);
    expect(container.querySelector('[data-role="composition"]')).toBeNull();
  });
});
