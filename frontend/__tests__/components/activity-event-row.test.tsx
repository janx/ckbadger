import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '../utils/test-utils';
import {
  ExactCkb,
  ItemDeltaFact,
  protocolActionNoteParts,
  activityHeadline,
  formatStandard,
  globalTxEvents,
  readable,
} from '@/components/activity-event-row';
import { MINUS } from '@/lib/chart-ticks';
import { exactCkbText } from '@/lib/ckb-decimal';
import type { GlobalActivity } from '@/lib/api';
import { Dotted } from '@/components/ui/phone-row';

function makeGlobal(overrides: Partial<GlobalActivity> = {}): GlobalActivity {
  return {
    txHash: '0x54522856aa00000000000000000000000000000000000000000000000000007b60c3',
    blockNumber: 20_819_142,
    txIndex: 1,
    timestamp: '1700000000000',
    isCellbase: false,
    protocolActions: [],
    typeCalls: [],
    lockCalls: [],
    participants: [],
    ...overrides,
  };
}

const bitTypeCalls = [
  {
    typeCodeHash: '0x3a46',
    typeHashType: 'type',
    typeArgs: '0x00',
    scriptHash: '0xh1',
    scriptName: '.bit Time Index State',
  },
  {
    typeCodeHash: '0x9e53',
    typeHashType: 'type',
    typeArgs: '0x00',
    scriptHash: '0xh2',
    scriptName: '.bit Time Info',
  },
];

describe('global tx events — one classification for the full rows and the feed', () => {
  it('orders protocol actions, type calls, then lock calls a protocol does not explain', () => {
    const activity = makeGlobal({
      protocolActions: [
        { protocol: 'utxoswap', protocolName: 'UTXOSwap', action: 'swap', metadata: {} },
      ],
      typeCalls: [bitTypeCalls[0]],
      lockCalls: [
        {
          lockCodeHash: '0xintent',
          lockHashType: 'type',
          lockArgs: '0x01',
          scriptHash: '0xi',
          scriptName: 'UTXOSwap Intent',
          decoded: { protocol: 'utxoswap' },
        },
        {
          lockCodeHash: '0xomni',
          lockHashType: 'type',
          lockArgs: '0x02',
          scriptHash: '0xo',
          scriptName: 'Omnilock',
        },
      ],
    });
    expect(globalTxEvents(activity).map((e) => e.kind)).toEqual(['protocol', 'type', 'lock']);
  });
});

describe('activityHeadline — the Layer-3 narrative from the classifier', () => {
  it('names protocol actions with their existing labels', () => {
    expect(
      activityHeadline(
        makeGlobal({
          protocolActions: [
            {
              protocol: 'dao',
              protocolName: 'Nervos DAO',
              action: 'withdraw_request',
              metadata: {},
            },
          ],
        })
      )
    ).toBe('DAO Withdraw Request');
    expect(
      activityHeadline(
        makeGlobal({
          protocolActions: [
            { protocol: 'fiber', protocolName: 'Fiber', action: 'channel_close', metadata: {} },
          ],
        })
      )
    ).toBe('Fiber \u00B7 Channel Close');
  });

  it('falls to the item layer, then calls, then CKB', () => {
    const party = {
      address: 'ckb1qx',
      lockHash: '0xaa',
      lockHashPrefix: null,
      roles: [],
      ckbDelta: '0',
      usedDelta: '0',
      itemDeltas: [{ kind: 'object' as const, objectId: '0x51', delta: 1 }],
      tags: 2,
    };
    expect(activityHeadline(makeGlobal({ participants: [party] }))).toBe('Object mint');
    expect(activityHeadline(makeGlobal({ typeCalls: bitTypeCalls }))).toBe('Script call');
    expect(activityHeadline(makeGlobal({}))).toBe('CKB Transfer');
    expect(activityHeadline(makeGlobal({ isCellbase: true }))).toBe('Cellbase');
  });
});

describe('protocol display names', () => {
  it('labels xudt_compatible as xUDT-compatible instead of the raw enum (IV-01)', () => {
    expect(formatStandard('xudt_compatible')).toBe('xUDT-compatible');
    expect(formatStandard('xudt')).toBe('xUDT');
    expect(formatStandard('sudt')).toBe('sUDT');
  });
});

describe('ExactCkb', () => {
  it('wraps exact amounts at the decimal or unit without dropping digits', () => {
    const { container } = render(
      <ExactCkb shannons={BigInt('18446744073709551615')} signed={false} wrap />
    );
    const amount = container.querySelector('[data-ckb-amount]');
    expect(amount).toHaveTextContent('184,467,440,737.09551615 CKB');
    expect(amount).toHaveClass('inline-flex', 'flex-wrap', 'justify-end');
    expect(amount?.children[1]).toHaveClass('whitespace-nowrap');
    expect(amount?.children[1]).toHaveTextContent('.09551615');
  });
  it('keeps all 8 decimals, ghosting the trailing zeros, coloured by sign; zero is a dim 0', () => {
    const { container } = render(
      <>
        <ExactCkb shannons={BigInt(-30000)} />
        <ExactCkb shannons={BigInt('75000000000')} />
        <ExactCkb shannons={BigInt(0)} />
      </>
    );
    const [fee, whole, zero] = [...container.querySelectorAll('[data-ckb-amount]')];
    expect(fee).toHaveTextContent(`${MINUS}0.00030000 CKB`);
    expect(fee).toHaveClass('text-negative');
    expect(fee.querySelector('[data-ghost]')).toHaveTextContent(/^0000$/);
    expect(whole).toHaveTextContent('+750.00000000 CKB');
    expect(whole).toHaveClass('text-positive');
    expect(whole.querySelector('[data-ghost]')).toHaveTextContent(/^\.00000000$/);
    // Consistency plan C4 (old: "0.00000000 CKB" in text-text): zero is "0 CKB"; its 0 is a datum
    // in the dim ink (R4, re-pinned on purpose: it was ghosted as if it were padding).
    expect(zero).toHaveTextContent(/^0 CKB$/);
    expect(zero.querySelector('[data-zero]')).toHaveTextContent(/^0$/);
    expect(zero.querySelector('[data-zero]')).toHaveClass('text-text-dim');
    expect(zero.querySelector('[data-ghost]')).toBeNull();
  });

  it('states zero as "0 CKB", its 0 dim, with no fraction for every caller (C4, R4)', () => {
    const { container } = render(
      <>
        <ExactCkb shannons={BigInt(0)} />
        <ExactCkb shannons={BigInt(0)} signed={false} tone="plain" wrap />
        <ExactCkb shannons={BigInt(0)} unit={false} />
      </>
    );
    const [feed, plain, bare] = [...container.querySelectorAll('[data-ckb-amount]')];
    expect(feed).toHaveTextContent(/^0 CKB$/);
    expect(plain).toHaveTextContent(/^0 CKB$/);
    expect(bare).toHaveTextContent(/^0$/);
    for (const amount of [feed, plain, bare]) {
      expect(amount.textContent).not.toContain('.');
      expect(amount.textContent).not.toMatch(/[+−]/);
      const digits = amount.querySelector('[data-zero]');
      expect(digits).toHaveTextContent(/^0$/);
      expect(digits).toHaveClass('text-text-dim');
    }
    // The unit as today: dim, meta, after a space.
    const unit = feed.lastElementChild;
    expect(unit).toHaveTextContent(/^CKB$/);
    expect(unit).toHaveClass('text-text-dim', 'text-meta');
  });

  // UI review 2026-11-14 (DY-06, U-08, ruling R4): the fraction digits are a step quieter than
  // the integer in a named ink of the amount's sign at 4.5:1 (palette.test), never the sign's ink
  // at 78 % (cinnabar 3.7:1 by night, celadon about 3.2–3.7:1 and the neutral 3.5:1 by day).
  it('sets the fraction digits in the quieter ink of the amount’s sign, never at an opacity', () => {
    const { container } = render(
      <>
        <ExactCkb shannons={BigInt(-30000)} />
        <ExactCkb shannons={BigInt('75000000123')} />
        <ExactCkb shannons={BigInt('75000000123')} signed={false} tone="plain" />
        <ExactCkb shannons={BigInt(-30000)} signed={false} tone="plain" />
      </>
    );
    const amounts = [...container.querySelectorAll('[data-ckb-amount]')];
    const fractions = amounts.map((a) => a.querySelector('[data-fraction]'));
    expect(fractions.map((f) => f?.textContent)).toEqual([
      '.0003',
      '.00000123',
      '.00000123',
      '.0003',
    ]);
    expect(fractions[0]).toHaveClass('text-rouge-dim');
    expect(fractions[1]).toHaveClass('text-jade-dim');
    expect(fractions[2]).toHaveClass('text-text-dim');
    expect(fractions[3]).toHaveClass('text-text-dim');
    for (const amount of amounts)
      for (const el of [amount, ...amount.querySelectorAll('*')])
        expect([...el.classList].filter((c) => c.startsWith('opacity-'))).toEqual([]);
  });

  // UI review 2026-11-14 (U-08, ruling R4): a value that IS zero is a datum ("fee 0 CKB"), so it
  // is the dim ink; the ghost is only for the padding zeros of a non-zero amount.
  it('draws a datum zero in the dim ink and only the padding zeros of a non-zero amount ghost', () => {
    const { container } = render(
      <>
        <ExactCkb shannons={BigInt(0)} signed={false} tone="plain" />
        <ExactCkb shannons={BigInt(100000000)} signed={false} tone="plain" />
      </>
    );
    const [zero, one] = [...container.querySelectorAll('[data-ckb-amount]')];
    const datum = zero.querySelector('[data-zero]');
    expect(datum).toHaveTextContent(/^0$/);
    expect(datum).toHaveClass('text-text-dim');
    expect(zero.querySelector('.text-text-ghost')).toBeNull();
    const padding = one.querySelector('[data-ghost]');
    expect(padding).toHaveTextContent(/^\.00000000$/);
    expect(padding).toHaveClass('text-text-ghost');
  });

  // Rubbing × Song print, phase 5 (T4): a sentence that names an amount the page draws (the
  // transaction comment's fee) takes its digits from exactCkbText; it must read as ExactCkb does.
  it('prints the same digits as exactCkbText, signed or not, zero included', () => {
    const amounts = ['0', '1647', '64119', '100000', '100000000', '75000000000', '-30000'];
    for (const signed of [true, false])
      for (const raw of amounts) {
        const shannons = BigInt(raw);
        const { container, unmount } = render(
          <ExactCkb shannons={shannons} signed={signed} tone="plain" />
        );
        expect(container.querySelector('[data-ckb-amount]')!.textContent).toBe(
          `${exactCkbText(shannons, signed)} CKB`
        );
        unmount();
      }
  });
});

describe('ItemDeltaFact', () => {
  it('states a token change exactly with its sign and symbol', () => {
    render(
      <ItemDeltaFact
        size="row"
        item={{
          kind: 'token',
          typeScriptHash: '0xseal',
          delta: '-3658346853885',
          symbol: 'Seal',
          decimals: 8,
        }}
      />
    );
    expect(screen.getByRole('link', { name: `${MINUS}36,583.46853885 Seal` })).toHaveAttribute(
      'href',
      '/mainnet/tokens/0xseal'
    );
  });

  it('states an object or identity as arrived or departed', () => {
    render(
      <>
        <ItemDeltaFact
          size="row"
          item={{ kind: 'object', objectId: '0x51c75a0000000000000096f2', delta: -1 }}
        />
        <ItemDeltaFact
          size="row"
          item={{
            kind: 'identity',
            standard: 'dotbit',
            identityId: '0xabc0000000000000000def',
            delta: 1,
          }}
        />
      </>
    );
    // re-pinned (A4 Chrome sweep): the id is the one unbreakable unit; "object", the id and the
    // verb are the line's words (it was "object <id>" as one nowrap run)
    const id = screen.getByText('0x51c75a00…0096f2');
    expect(id).toHaveAttribute('data-nobreak');
    const run = id.closest('.text-text-bright') as HTMLElement;
    expect(run).not.toHaveClass('whitespace-nowrap');
    expect(run).toHaveTextContent('object 0x51c75a00…0096f2 departed');
    expect(screen.getByText(/^\.bit/).parentElement).toHaveTextContent(
      '.bit 0xabc00000…000def arrived'
    );
  });
});

describe('ItemDeltaFact marks (IV-24, TX-12, TX-29)', () => {
  it('puts a 12 px class mark before every item change', () => {
    const { container } = render(
      <>
        <ItemDeltaFact
          size="row"
          item={{
            kind: 'token',
            typeScriptHash: '0xseal',
            delta: '5',
            symbol: 'Seal',
            decimals: 0,
          }}
        />
        <ItemDeltaFact
          size="row"
          item={{ kind: 'object', objectId: '0x51c75a0000000000000096f2', delta: -1 }}
        />
        <ItemDeltaFact
          size="row"
          item={{
            kind: 'identity',
            standard: 'dotbit',
            identityId: '0xabc0000000000000000def',
            delta: 1,
          }}
        />
      </>
    );
    const marks = Array.from(container.querySelectorAll('svg[data-class]'));
    expect(marks.map((m) => m.getAttribute('data-class'))).toEqual([
      'tokens',
      'objects',
      'identities',
    ]);
    for (const m of marks) expect(m).toHaveAttribute('width', '12');
  });

  it('names the collection and shows the composition badge only when the API sends them (R12)', () => {
    const { rerender, container } = render(
      <ItemDeltaFact
        size="row"
        item={{
          kind: 'object',
          objectId: '0x51c75a0000000000000096f2',
          delta: -1,
          collection: { id: '0xd585', name: 'Nervape 3x' },
          composition: 'btc_ckb',
        }}
      />
    );
    expect(container).toHaveTextContent('Nervape 3x 0x51c75a00…0096f2 departed');
    expect(container.querySelector('[data-composition="btc_ckb"]')).not.toBeNull();
    rerender(
      <ItemDeltaFact
        size="row"
        item={{ kind: 'object', objectId: '0x51c75a0000000000000096f2', delta: -1 }}
      />
    );
    // No field, no badge and no guessed name.
    expect(container.querySelector('[data-composition]')).toBeNull();
    expect(container).toHaveTextContent('object 0x51c75a00…0096f2 departed');
  });

  // A4 Chrome sweep: the collection's name and the object's id were one nowrap unit, so a name of
  // several words ("Spore Blind Box Series 2") and its id could not break: a phrase is not a unit.
  // The id is the unit (ShortId); the name's words and the space before the id may break.
  it('keeps the object’s id whole, never its collection’s name with it (A4 Chrome sweep)', () => {
    const { container } = render(
      <ItemDeltaFact
        size="row"
        item={{
          kind: 'object',
          objectId: '0x51c75a0000000000000096f2',
          delta: -1,
          collection: { id: '0xd585', name: 'Spore Blind Box Series 2' },
        }}
      />
    );
    expect(container).toHaveTextContent('Spore Blind Box Series 2 0x51c75a00…0096f2 departed');
    const id = screen.getByText('0x51c75a00…0096f2');
    expect(id).toHaveAttribute('data-nobreak');
    expect(id.parentElement!.closest('[data-nobreak], .whitespace-nowrap')).toBeNull();
  });
});

// The action metadata as the rows read it: its parts in the one separated list (E1 kept a dead
// ProtocolActionNote wrapper for these tests; it is gone, and they read the live composition).
function ProtocolActionNote({ pa }: { pa: Parameters<typeof protocolActionNoteParts>[0] }) {
  return <Dotted parts={protocolActionNoteParts(pa, 'row')} />;
}

describe('protocol action metadata — in one quiet line', () => {
  it('reads a DAO withdraw request: amount and the deposit block', () => {
    render(
      <ProtocolActionNote
        pa={{
          protocol: 'dao',
          protocolName: 'Nervos DAO',
          action: 'withdraw_request',
          metadata: { capacity: '2605000000000', depositBlock: '19695550' },
        }}
      />
    );
    expect(document.body).toHaveTextContent('26,050.00000000 CKB · deposited at #19,695,550');
    // Lane L: the deposit block is a link wherever the note is read (tx summary, feeds).
    expect(screen.getByRole('link', { name: '#19,695,550' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/19695550'
    );
  });

  it('reads a Fiber close: the channel and its capacity', () => {
    render(
      <ProtocolActionNote
        pa={{
          protocol: 'fiber',
          protocolName: 'Fiber',
          action: 'channel_close',
          metadata: {
            channelOutpoint: '0x0d3870bb00000000000000000000000000000000000000000000000000006e26:0',
            capacity: '75000000000',
          },
        }}
      />
    );
    expect(document.body).toHaveTextContent('channel 0x0d3870bb…006e26:0 · 750.00000000 CKB');
    // The channel's out point is its funding cell: it opens that cell's page.
    expect(screen.getByRole('link', { name: '0x0d3870bb…006e26:0' })).toHaveAttribute(
      'href',
      expect.stringMatching(/\/cell\/0x0d3870bb0+6e26-0$/)
    );
  });

  it('reads an amount past 2^53 exactly', () => {
    render(
      <ProtocolActionNote
        pa={{
          protocol: 'dao',
          protocolName: 'Nervos DAO',
          action: 'withdraw_complete',
          metadata: { capacity: '9007199254740993', compensation: '12345678901234567' },
        }}
      />
    );
    expect(document.body).toHaveTextContent(
      '90,071,992.54740993 CKB · +123,456,789.01234567 CKB compensation'
    );
  });

  it('renders nothing for an action without metadata', () => {
    const { container } = render(
      <ProtocolActionNote
        pa={{ protocol: 'utxoswap', protocolName: 'UTXOSwap', action: 'swap', metadata: {} }}
      />
    );
    expect(container).toBeEmptyDOMElement();
  });
});

// F2-tx: the shared row parts (report A TX-31, and the blocks group's relay).
describe('row parts (F2-tx)', () => {
  it('draws arrived / departed quiet, the item itself bright (AssetClasses, Main boards)', () => {
    render(
      <ItemDeltaFact
        size="row"
        item={{ kind: 'object', objectId: `0x51c7${'00'.repeat(28)}96f2`, delta: 1 }}
      />
    );
    expect(screen.getByText('arrived')).toHaveClass('text-text-dim');
  });

  it('colours a token amount by its sign when a page asks for it (Address board)', () => {
    const { rerender } = render(
      <ItemDeltaFact
        size="row"
        item={{
          kind: 'token',
          typeScriptHash: '0xickb',
          delta: '8465187699473',
          symbol: 'iCKB',
          decimals: 8,
        }}
      />
    );
    expect(screen.getByRole('link')).toHaveClass('text-text-bright');
    rerender(
      <ItemDeltaFact
        size="row"
        tone="sign"
        item={{
          kind: 'token',
          typeScriptHash: '0xickb',
          delta: '8465187699473',
          symbol: 'iCKB',
          decimals: 8,
        }}
      />
    );
    expect(screen.getByRole('link')).toHaveClass('text-positive');
    rerender(
      <ItemDeltaFact
        size="row"
        tone="sign"
        item={{ kind: 'token', typeScriptHash: '0xickb', delta: '-1', symbol: 'iCKB', decimals: 8 }}
      />
    );
    expect(screen.getByRole('link')).toHaveClass('text-negative');
  });

  it('draws the symbol dim beside a signed amount when a row asks for it (T7 feed rows)', () => {
    const item = {
      kind: 'token' as const,
      typeScriptHash: '0xickb',
      delta: '-8465187699473',
      symbol: 'iCKB',
      decimals: 8,
    };
    const { rerender } = render(<ItemDeltaFact size="row" tone="sign" item={item} />);
    // Default: the symbol shares the amount's ink (Address board, unchanged).
    expect(screen.getByText(/iCKB$/)).toBe(screen.getByRole('link'));
    rerender(<ItemDeltaFact size="row" tone="sign" symbol="dim" item={item} />);
    const link = screen.getByRole('link', { name: `${MINUS}84,651.87699473 iCKB` });
    expect(link).toHaveClass('text-negative');
    expect(within(link).getByText('iCKB')).toHaveClass('text-text-dim');
  });
});

// Review 45fd ruling D7: readable() is a real error boundary — it catches what a component it
// returns throws while rendering, not only what its thunk throws, and adds no element of its own.
describe('readable', () => {
  const TX = `0x${'ab'.repeat(32)}`;

  function Broken(): never {
    throw new Error('participant ckbDelta "abc"');
  }

  it('says a render-time error of a returned component in place, with the transaction', () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      render(
        <p data-testid="slot">
          before{' '}
          {readable(TX, () => (
            <Broken />
          ))}{' '}
          after
        </p>
      );
      const error = screen.getByTestId('activity-read-error');
      expect(error).toHaveTextContent(`Cannot read tx ${TX}: participant ckbDelta "abc"`);
      expect(screen.getByTestId('slot')).toHaveTextContent(/^before .* after$/);
    } finally {
      quiet.mockRestore();
    }
  });

  it('says an error its thunk throws the same way', () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      render(
        <div>
          {readable(TX, () => {
            throw new Error('fee "x"');
          })}
        </div>
      );
      expect(screen.getByTestId('activity-read-error')).toHaveTextContent(
        `Cannot read tx ${TX}: fee "x"`
      );
    } finally {
      quiet.mockRestore();
    }
  });

  it('renders exactly what the thunk returns, and nothing for null', () => {
    const { container } = render(
      <div>
        {readable(TX, () => (
          <span className="a">one</span>
        ))}
        {readable(`0x${'cd'.repeat(32)}`, () => null)}
      </div>
    );
    expect(container.innerHTML).toBe('<div><span class="a">one</span></div>');
  });
});
