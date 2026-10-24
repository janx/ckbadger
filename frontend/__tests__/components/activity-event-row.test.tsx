import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import {
  ActivityEventGroup,
  ExactCkb,
  ItemDeltaFact,
  LockCallExpr,
  ParticipantLine,
  ProtocolActionNote,
  TypeCallExpr,
  activityHeadline,
  buildGlobalTxEvents,
  globalTxEvents,
} from '@/components/activity-event-row';
import { MINUS } from '@/lib/chart-ticks';
import type { Activity, GlobalActivity } from '@/lib/api';

// Spread the overrides rather than `??`-ing each field: a pool row's
// `blockNumber` is explicitly null, and `null ?? default` would quietly hand it
// a block it does not have.
function makeActivity(overrides: Partial<Activity> = {}): Activity {
  return {
    txHash: '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890',
    blockNumber: 10_000,
    txIndex: 0,
    timestamp: '1700000000',
    ckbDelta: '0',
    usedDelta: '0',
    isCellbase: false,
    itemDeltas: [],
    typeCalls: [],
    lockCalls: [],
    protocolActions: [],
    participants: [],
    roles: [],
    tags: 0,
    ...overrides,
  };
}

const mockFormatTimeAgo = () => '2 hrs ago';

describe('ActivityEventGroup', () => {
  it('renders tx hash and block number', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({ blockNumber: 12345 })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/12,345/).length).toBeGreaterThan(0);
  });

  it('always renders CKB Transfer sub-row', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({ ckbDelta: '-50000000000' })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/CKB Transfer/).length).toBeGreaterThan(0);
  });

  it('renders Coinbase for cellbase activities', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({ isCellbase: true, ckbDelta: '100000000' })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/Coinbase/).length).toBeGreaterThan(0);
  });

  it('renders DAO Deposit sub-row plus CKB sub-row', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          ckbDelta: '-10200000000',
          protocolActions: [
            { protocol: 'dao', action: 'deposit', metadata: { capacity: '10200000000' } },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/DAO Deposit/).length).toBeGreaterThan(0);
    // CKB row is labeled "CKB" (not "CKB Transfer") when L3 events are present
    expect(screen.getAllByText(/CKB/).length).toBeGreaterThan(0);
  });

  it('renders token sub-row with symbol', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          itemDeltas: [
            {
              kind: 'token',
              typeScriptHash: '0xtoken',
              delta: '1200',
              symbol: 'SEAL',
              decimals: 8,
            },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/SEAL Transfer/).length).toBeGreaterThan(0);
  });

  it('marks token amounts as raw when decimals are unknown', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          itemDeltas: [
            {
              kind: 'token',
              typeScriptHash: '0xtoken',
              delta: '1200',
              symbol: 'MYST',
              decimals: null,
            },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/\(raw\)/).length).toBeGreaterThan(0);
  });

  it('renders generic type script call label', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          typeCalls: [
            {
              typeCodeHash: '0xcode',
              typeHashType: 'type',
              typeArgs: '0x1234',
              scriptHash: '0xhash',
              scriptName: 'Omnilock',
            },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/Script Call \(type\)/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/Omnilock/).length).toBeGreaterThan(0);
    expect(screen.queryByText(/Type call/)).not.toBeInTheDocument();
  });

  it('keeps generic type script call label when scriptName is set', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          typeCalls: [
            {
              typeCodeHash: '0xcode',
              typeHashType: 'type',
              typeArgs: '0x1234',
              scriptHash: '0xhash',
              scriptName: 'Stable++ Pool',
            },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/Script Call \(type\)/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/Stable\+\+ Pool/).length).toBeGreaterThan(0);
    expect(screen.queryByText(/Type call/)).not.toBeInTheDocument();
  });

  it('removes the hash-type prefix from type script refs', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          typeCalls: [
            {
              typeCodeHash: '0xcode',
              typeHashType: 'data1',
              typeArgs: '0x1234',
              scriptHash: '0x1234567890abcdef',
            },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/Script Call \(type\)/).length).toBeGreaterThan(0);
    expect(screen.getAllByRole('link', { name: '0x12345678' }).length).toBeGreaterThan(0);
    expect(screen.queryByText('data1:0x12345678')).not.toBeInTheDocument();
  });

  it('renders multiple event types in one activity', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          ckbDelta: '-1000000000',
          itemDeltas: [
            { kind: 'token', typeScriptHash: '0xt', delta: '500', symbol: 'SEAL', decimals: 0 },
            { kind: 'object', objectId: '0xobj123', delta: 1 },
          ],
          typeCalls: [
            {
              typeCodeHash: '0xc',
              typeHashType: 'type',
              typeArgs: '0xa',
              scriptHash: '0xh',
              scriptName: 'Spore',
            },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    // All four event types present: token, object, script call (labeled by scriptName), CKB
    expect(screen.getAllByText(/SEAL Transfer/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/Object/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/Spore/).length).toBeGreaterThan(0);
    // CKB row is labeled "CKB" (not "CKB Transfer") when L2/L3 events are present
    expect(screen.getAllByText(/CKB/).length).toBeGreaterThan(0);
  });

  it('renders DAO Withdraw Complete with compensation', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          protocolActions: [
            {
              protocol: 'dao',
              action: 'withdraw_complete',
              metadata: { capacity: '20000000000', compensation: '500000000' },
            },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/DAO Withdraw Complete/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/compensation/).length).toBeGreaterThan(0);
  });

  it('states a zero compensation as an amount, not a stray 0', () => {
    const { container } = render(
      <ActivityEventGroup
        activity={makeActivity({
          protocolActions: [
            {
              protocol: 'dao',
              action: 'withdraw_complete',
              metadata: { capacity: '20000000000', compensation: '0' },
            },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText('+0.00000000 CKB compensation').length).toBeGreaterThan(0);
    // A bigint 0n in JSX would print a bare "0" next to the amount.
    expect(container.innerHTML).not.toMatch(/>0</);
  });

  it('renders identity sub-row', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          itemDeltas: [{ kind: 'identity', standard: 'dotbit', identityId: '0xid123', delta: 1 }],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/Identity/).length).toBeGreaterThan(0);
  });

  // Every identity standard has its own detail route; an identity delta used to
  // be routed as 'identity', which fell through to /objects/mnft/{id} (a 404).
  const DOTCELL_NAME_ID = `0x${'ab'.repeat(20)}`;
  const DOTBIT_ACCOUNT_ID = `0x${'cd'.repeat(20)}`;
  const identityDeltas: Activity['itemDeltas'] = [
    { kind: 'identity', standard: 'dotcell', identityId: DOTCELL_NAME_ID, delta: 1 },
    { kind: 'identity', standard: 'dotbit', identityId: DOTBIT_ACCOUNT_ID, delta: -1 },
  ];

  function renderedHrefs(): string[] {
    return screen.getAllByRole('link').map((link) => link.getAttribute('href') ?? '');
  }

  it('links identity item deltas to the detail page of their own standard', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({ itemDeltas: identityDeltas })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    const hrefs = renderedHrefs();
    expect(hrefs.some((href) => href.endsWith(`/identities/dotcell/${DOTCELL_NAME_ID}`))).toBe(
      true
    );
    expect(hrefs.some((href) => href.endsWith(`/identities/dotbit/${DOTBIT_ACCOUNT_ID}`))).toBe(
      true
    );
    expect(hrefs.some((href) => href.includes('/objects/mnft/'))).toBe(false);
  });

  it('links identity item deltas on a participant line to their own standard', () => {
    render(
      <ParticipantLine
        participant={{
          address: null,
          lockHash: null,
          lockHashPrefix: `0x${'11'.repeat(20)}`,
          roles: [],
          ckbDelta: '0',
          usedDelta: '0',
          itemDeltas: identityDeltas,
          tags: 0,
        }}
      />
    );
    const hrefs = renderedHrefs();
    expect(hrefs.some((href) => href.endsWith(`/identities/dotcell/${DOTCELL_NAME_ID}`))).toBe(
      true
    );
    expect(hrefs.some((href) => href.endsWith(`/identities/dotbit/${DOTBIT_ACCOUNT_ID}`))).toBe(
      true
    );
    expect(hrefs.some((href) => href.includes('/objects/mnft/'))).toBe(false);
  });

  it('renders time ago text', () => {
    render(<ActivityEventGroup activity={makeActivity()} formatTimeAgo={() => '5 mins ago'} />);
    expect(screen.getAllByText('5 mins ago').length).toBeGreaterThan(0);
  });

  it('renders generic lock script call label', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          lockCalls: [
            {
              lockCodeHash: '0xintent',
              lockHashType: 'type',
              lockArgs: '0xargs1234',
              scriptHash: '0xhash',
              scriptName: 'UTXOSwap Intent',
            },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/Script Call \(lock\)/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/UTXOSwap Intent/).length).toBeGreaterThan(0);
  });

  it('renders generic lock script call label when lock call has no script name or protocol', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          lockCalls: [
            {
              lockCodeHash: '0xunknown',
              lockHashType: 'type',
              lockArgs: '0xargs',
              scriptHash: '0xhash',
            },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/Script Call \(lock\)/).length).toBeGreaterThan(0);
  });

  it('removes the hash-type prefix from lock script refs', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          lockCalls: [
            {
              lockCodeHash: '0xunknown',
              lockHashType: 'type',
              lockArgs: '0xargs',
              scriptHash: '0x8765432100abcdef',
            },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/Script Call \(lock\)/).length).toBeGreaterThan(0);
    expect(screen.getAllByRole('link', { name: '0x87654321' }).length).toBeGreaterThan(0);
    expect(screen.queryByText('type:0x87654321')).not.toBeInTheDocument();
  });
});

describe('ActivityEventGroup — tx-pool rows', () => {
  function makePoolActivity(overrides: Partial<Activity> = {}): Activity {
    return makeActivity({
      blockNumber: null,
      txIndex: null,
      timestamp: null,
      poolStatus: 'pending',
      timeAddedToPool: new Date(Date.now() - 5 * 60 * 1000).toISOString(),
      interpretation: { status: 'complete' },
      ...overrides,
    });
  }

  it('shows the pool status instead of a block link', () => {
    render(<ActivityEventGroup activity={makePoolActivity()} formatTimeAgo={mockFormatTimeAgo} />);
    expect(screen.getAllByText('Pending').length).toBeGreaterThan(0);
    expect(screen.queryByText(/^#/)).not.toBeInTheDocument();
  });

  it('reports how long the transaction has been in the pool instead of a block time', () => {
    render(<ActivityEventGroup activity={makePoolActivity()} formatTimeAgo={mockFormatTimeAgo} />);
    expect(screen.getAllByText(/in pool for 5m/).length).toBeGreaterThan(0);
    expect(screen.queryByText('2 hrs ago')).not.toBeInTheDocument();
  });

  it('still links to the transaction, which renders pending transactions', () => {
    const activity = makePoolActivity();
    render(<ActivityEventGroup activity={activity} formatTimeAgo={mockFormatTimeAgo} />);
    const links = screen.getAllByRole('link');
    expect(
      links.some((link) => link.getAttribute('href')?.endsWith(`/tx/${activity.txHash}`))
    ).toBe(true);
  });

  it('shows why an interpretation is incomplete', () => {
    render(
      <ActivityEventGroup
        activity={makePoolActivity({
          interpretation: {
            status: 'partial',
            reasons: [{ code: 'unresolved_input', detail: '0xabc:1' }],
          },
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/input not yet resolvable \(0xabc:1\)/).length).toBeGreaterThan(0);
  });

  it('keeps rendering a committed row with its block link', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({ blockNumber: 12345 })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/12,345/).length).toBeGreaterThan(0);
    expect(screen.queryByText('Pending')).not.toBeInTheDocument();
  });
});

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
      protocolActions: [{ protocol: 'utxoswap', action: 'swap', metadata: {} }],
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
    // The full rows render exactly those events.
    expect(buildGlobalTxEvents(activity)).toHaveLength(3);
  });
});

describe('activityHeadline — the Layer-3 narrative from the classifier', () => {
  it('names protocol actions with their existing labels', () => {
    expect(
      activityHeadline(
        makeGlobal({
          protocolActions: [{ protocol: 'dao', action: 'withdraw_request', metadata: {} }],
        })
      )
    ).toBe('DAO Withdraw Request');
    expect(
      activityHeadline(
        makeGlobal({
          protocolActions: [{ protocol: 'fiber', action: 'channel_close', metadata: {} }],
        })
      )
    ).toBe('fiber \u00B7 Channel Close');
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
    expect(activityHeadline(makeGlobal({ participants: [party] }))).toBe('Object transfer');
    expect(activityHeadline(makeGlobal({ typeCalls: bitTypeCalls }))).toBe('Script call');
    expect(activityHeadline(makeGlobal({}))).toBe('CKB Transfer');
    expect(activityHeadline(makeGlobal({ isCellbase: true }))).toBe('Coinbase');
  });
});

describe('quiet call expressions', () => {
  it('keep the args, with the name as an ink link and the args dim — no gold', () => {
    render(
      <>
        <TypeCallExpr sc={bitTypeCalls[0]} quiet />
        <LockCallExpr
          quiet
          lc={{
            lockCodeHash: '0xnervape',
            lockHashType: 'type',
            lockArgs: '0x0131da00000000000000000000000000005d74',
            scriptHash: '0xn',
            scriptName: 'Nervape Shadow Lock',
          }}
        />
      </>
    );
    const name = screen.getByRole('link', { name: '.bit Time Index State' });
    expect(name).toHaveClass('text-text');
    expect(name.className).not.toContain('gold');
    expect(screen.getByText('0x00')).toHaveClass('text-text-dim');
    expect(screen.getByRole('link', { name: 'Nervape Shadow Lock' })).toHaveAttribute(
      'href',
      '/mainnet/scripts/Nervape%20Shadow%20Lock'
    );
    expect(screen.getByText('0x0131...5d74')).toHaveClass('text-text-dim');
  });
});

describe('ExactCkb', () => {
  it('keeps all 8 decimals, ghosting the trailing zeros, coloured by sign', () => {
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
    expect(zero).toHaveTextContent(/^0\.00000000 CKB$/);
    expect(zero).toHaveClass('text-text');
  });
});

describe('ItemDeltaFact', () => {
  it('states a token change exactly with its sign and symbol', () => {
    render(
      <ItemDeltaFact
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
          item={{ kind: 'object', objectId: '0x51c75a0000000000000096f2', delta: -1 }}
        />
        <ItemDeltaFact
          item={{
            kind: 'identity',
            standard: 'dotbit',
            identityId: '0xabc0000000000000000def',
            delta: 1,
          }}
        />
      </>
    );
    // "object <id>" is one unbreakable run; only the verb may wrap onto the next line.
    const run = screen.getByText(/^object/);
    expect(run).toHaveClass('whitespace-nowrap');
    expect(run.parentElement).toHaveTextContent('object 0x51c75a...96f2 departed');
    expect(screen.getByText(/^\.bit/).parentElement).toHaveTextContent(
      '.bit 0xabc000...0def arrived'
    );
  });
});

describe('ProtocolActionNote — the action metadata in one quiet line', () => {
  it('reads a DAO withdraw request: amount and the deposit block', () => {
    render(
      <ProtocolActionNote
        pa={{
          protocol: 'dao',
          action: 'withdraw_request',
          metadata: { capacity: '2605000000000', depositBlock: '19695550' },
        }}
      />
    );
    expect(document.body).toHaveTextContent('26,050.00000000 CKB · deposited at #19,695,550');
  });

  it('reads a Fiber close: the channel and its capacity', () => {
    render(
      <ProtocolActionNote
        pa={{
          protocol: 'fiber',
          action: 'channel_close',
          metadata: {
            channelOutpoint: '0x0d3870bb00000000000000000000000000000000000000000000000000006e26:0',
            capacity: '75000000000',
          },
        }}
      />
    );
    expect(document.body).toHaveTextContent('channel 0x0d3870bb...6e26:0 · 750.00000000 CKB');
  });

  it('reads an amount past 2^53 exactly', () => {
    render(
      <ProtocolActionNote
        pa={{
          protocol: 'dao',
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
      <ProtocolActionNote pa={{ protocol: 'utxoswap', action: 'swap', metadata: {} }} />
    );
    expect(container).toBeEmptyDOMElement();
  });
});
