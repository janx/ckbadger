import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '../utils/test-utils';
import {
  ItemDeltaFact,
  protocolActionNoteParts,
  activityHeadline,
  formatStandard,
  globalTxEvents,
  readable,
} from '@/components/activity-event-row';
import { MINUS } from '@/lib/chart-ticks';
import type { GlobalActivity } from '@/lib/api';
import { Dotted } from '@/components/ui/phone-row';
import { activityRead, ITEM_TYPE } from '@/__tests__/utils/activity-read';

function makeGlobal(overrides: Partial<GlobalActivity> = {}): GlobalActivity {
  return activityRead({
    txHash: '0x54522856aa00000000000000000000000000000000000000000000000000007b60c3',
    blockNumber: 20_819_142,
    txIndex: 1,
    timestamp: '1700000000000',
    isCellbase: false,
    protocolActions: [],
    typeCalls: [],
    lockCalls: [],
    itemTypes: [],
    itemTypesUnread: null,
    participants: [],
    ...overrides,
  });
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
          lockScriptName: 'UTXOSwap Intent',
          decoded: { protocol: 'utxoswap' },
        },
        {
          lockCodeHash: '0xomni',
          lockHashType: 'type',
          lockArgs: '0x02',
          scriptHash: '0xo',
          lockScriptName: 'Omnilock',
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
          itemTypes: [ITEM_TYPE.dao],
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
    expect(
      activityHeadline(makeGlobal({ itemTypes: [ITEM_TYPE.spore], participants: [party] }))
    ).toBe('Object mint');
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

describe('ItemDeltaFact', () => {
  // Readability R5 (re-pinned on purpose): a feed line's token change is the summary form, cut,
  // its exact value in its title (it was every digit: "−36,583.46853885 Seal").
  it('states a token change in its summary form with its sign and symbol, exact in its title', () => {
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
    const link = screen.getByRole('link', { name: `${MINUS}36,583 Seal` });
    expect(link).toHaveAttribute('href', '/mainnet/tokens/0xseal');
    expect(link.querySelector('[data-token-amount]')).toHaveAttribute(
      'title',
      `${MINUS}36,583.46853885`
    );
  });

  // The P3 follow-up's 2c (the link-look gate's entry): the link states its look, a link inside a
  // line; a signed amount's ink is the amount's (its sign), never the link's; the symbol is the
  // link's word, in the line's ink, with no ink written inside the link.
  it('states its link inline: the amount keeps its sign ink, the symbol the line’s', () => {
    render(
      <ItemDeltaFact
        size="row"
        tone="sign"
        item={{
          kind: 'token',
          typeScriptHash: '0xseal',
          delta: '-3658346853885',
          symbol: 'Seal',
          decimals: 8,
        }}
      />
    );
    const link = screen.getByRole('link', { name: `${MINUS}36,583 Seal` });
    expect(link).toHaveClass('underline');
    expect(link.className).not.toMatch(/\btext-(positive|negative|text|jade)/);
    expect(link.querySelector('[data-token-amount]')).toHaveClass('text-negative');
    expect(link.querySelectorAll('[class*="text-text"]')).toHaveLength(0);
    expect(link.parentElement!.className).not.toMatch(/\btext-/);
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
// Re-pinned on purpose (the P3 follow-up's 2c): a caller names the note's amount form, `summary`
// in a line, `exact` where the row is the thing's own record; every one was `column`, padded to
// eight decimals in a sentence ("26,050.00000000 CKB").
function ProtocolActionNote({
  pa,
  form = 'summary',
}: {
  pa: Parameters<typeof protocolActionNoteParts>[0];
  form?: Parameters<typeof protocolActionNoteParts>[2];
}) {
  return <Dotted parts={protocolActionNoteParts(pa, 'row', form)} />;
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
    expect(document.body).toHaveTextContent('26,050 CKB · deposited at #19,695,550');
    // Lane L: the deposit block is a link wherever the note is read (tx summary, feeds).
    const block = screen.getByRole('link', { name: '#19,695,550' });
    expect(block).toHaveAttribute('href', '/mainnet/blocks/19695550');
    // a link inside the note's line (the P3 follow-up's 2c): the inline look, no ink of its own
    expect(block).toHaveClass('underline');
    expect(block.className).not.toMatch(/hover:text-jade/);
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
    expect(document.body).toHaveTextContent('channel 0x0d3870bb…006e26:0 · 750 CKB');
    // The channel's out point is its funding cell: it opens that cell's page.
    const channel = screen.getByRole('link', { name: '0x0d3870bb…006e26:0' });
    expect(channel).toHaveAttribute('href', expect.stringMatching(/\/cell\/0x0d3870bb0+6e26-0$/));
    expect(channel).toHaveClass('underline');
  });

  it('reads an amount past 2^53 exactly', () => {
    const pa = {
      protocol: 'dao',
      protocolName: 'Nervos DAO',
      action: 'withdraw_complete',
      metadata: { capacity: '9007199254740993', compensation: '12345678901234567' },
    };
    // the thing's own record: every digit, no padding
    const { unmount } = render(<ProtocolActionNote pa={pa} form="exact" />);
    expect(document.body).toHaveTextContent(
      '90,071,992.54740993 CKB · +123,456,789.01234567 CKB compensation'
    );
    unmount();
    // a line: cut, never rounded, the exact value in its title
    render(<ProtocolActionNote pa={pa} />);
    expect(document.body).toHaveTextContent('90,071,992 CKB · +123,456,789 CKB compensation');
    expect(screen.getByText('90,071,992').closest('[title]')).toHaveAttribute(
      'title',
      expect.stringContaining('90,071,992.54740993')
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
  // Re-pinned with readability ruling R3: "arrived" / "departed" is the change's direction, a value
  // as a cell's "live" is, so it reads in the reading ink (it was quiet); the item stays bright.
  it('draws arrived / departed in the reading ink, the item itself bright (AssetClasses, Main boards)', () => {
    render(
      <ItemDeltaFact
        size="row"
        item={{ kind: 'object', objectId: `0x51c7${'00'.repeat(28)}96f2`, delta: 1 }}
      />
    );
    expect(screen.getByText('arrived')).toHaveClass('text-text');
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
    // re-pinned on purpose (the P3 follow-up's 2c): the amount's own ink, by its tone (the reading
    // ink of a plain amount), not the link's; the link was text-text-bright, then the sign inks
    const amount = () => screen.getByRole('link').querySelector('[data-token-amount]');
    expect(amount()).toHaveClass('text-text');
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
    expect(amount()).toHaveClass('text-positive');
    rerender(
      <ItemDeltaFact
        size="row"
        tone="sign"
        item={{ kind: 'token', typeScriptHash: '0xickb', delta: '-1', symbol: 'iCKB', decimals: 8 }}
      />
    );
    expect(amount()).toHaveClass('text-negative');
  });

  // Re-pinned on purpose (the P3 follow-up's 2c): a T7 feed row drew the symbol dim on a span
  // inside the link; the symbol is the link's word now, in the line's ink, and only the amount
  // keeps an ink of its own.
  it('draws the symbol as the link’s word beside a signed amount (T7 feed rows)', () => {
    const item = {
      kind: 'token' as const,
      typeScriptHash: '0xickb',
      delta: '-8465187699473',
      symbol: 'iCKB',
      decimals: 8,
    };
    render(<ItemDeltaFact size="row" tone="sign" item={item} />);
    expect(screen.getByText(/iCKB$/)).toBe(screen.getByRole('link'));
    // the summary form (R5, re-pinned on purpose: was every digit)
    const link = screen.getByRole('link', { name: `${MINUS}84,651 iCKB` });
    expect(link.querySelector('[data-token-amount]')).toHaveClass('text-negative');
    expect(link.className).not.toMatch(/\btext-(negative|positive|text)/);
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
