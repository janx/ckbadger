import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '../utils/test-utils';
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
  formatStandard,
  globalTxEvents,
  readable,
} from '@/components/activity-event-row';
import { MINUS } from '@/lib/chart-ticks';
import { exactCkbText } from '@/lib/ckb-decimal';
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

  it('renders Cellbase for cellbase activities', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({ isCellbase: true, ckbDelta: '100000000' })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/Cellbase/).length).toBeGreaterThan(0);
    expect(screen.queryByText(/Coinbase/)).not.toBeInTheDocument();
  });

  it('renders DAO Deposit sub-row plus CKB sub-row', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          ckbDelta: '-10200000000',
          protocolActions: [
            {
              protocol: 'dao',
              protocolName: 'Nervos DAO',
              action: 'deposit',
              metadata: { capacity: '10200000000' },
            },
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
    expect(screen.getAllByRole('link', { name: '0xcode' }).length).toBeGreaterThan(0);
    expect(screen.queryByText(/0x12345678/)).not.toBeInTheDocument();
    expect(screen.queryByText(/data1:/)).not.toBeInTheDocument();
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
              protocolName: 'Nervos DAO',
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

  it('shows an unreadable DAO step as an error naming the tx, never as +0 CKB', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          txHash: '0xfeed000000000000000000000000000000000000000000000000000000000001',
          protocolActions: [
            { protocol: 'dao', protocolName: 'Nervos DAO', action: 'deposit', metadata: {} },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    const alerts = screen.getAllByRole('alert');
    expect(alerts[0]).toHaveTextContent(
      'Cannot read tx 0xfeed000000000000000000000000000000000000000000000000000000000001: dao:deposit metadata.capacity is missing'
    );
    expect(screen.queryByText(/\+0 CKB/)).not.toBeInTheDocument();
    expect(screen.queryByText(/DAO Deposit/)).not.toBeInTheDocument();
  });

  it('states a zero compensation as an amount, not a stray 0', () => {
    const { container } = render(
      <ActivityEventGroup
        activity={makeActivity({
          protocolActions: [
            {
              protocol: 'dao',
              protocolName: 'Nervos DAO',
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
    expect(screen.getAllByRole('link', { name: '0xunknown' }).length).toBeGreaterThan(0);
    expect(screen.queryByText(/0x87654321/)).not.toBeInTheDocument();
    expect(screen.queryByText(/type:0x/)).not.toBeInTheDocument();
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
    // The full rows render exactly those events.
    expect(buildGlobalTxEvents(activity)).toHaveLength(3);
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

  it('name the protocol by the protocolName the API sends with the action, not its raw id', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          protocolActions: [
            {
              protocol: 'newproto',
              protocolName: 'New Proto',
              action: 'leap_to_ckb',
              metadata: {},
            },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/New Proto · leap to ckb/).length).toBeGreaterThan(0);
    expect(screen.queryByText(/newproto ·/)).not.toBeInTheDocument();
  });

  it('show a lock call by its script name, never by the protocol it decodes to', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          lockCalls: [
            {
              lockCodeHash: '0xutx',
              lockHashType: 'type',
              lockArgs: '0x01',
              scriptHash: '0xu',
              scriptName: 'UTXOSwap Intent',
              decoded: { protocol: 'utxoswap' },
            },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/UTXOSwap Intent/).length).toBeGreaterThan(0);
    expect(screen.queryByText(/^UTXOSwap$/)).not.toBeInTheDocument();
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

// Pass 3 (revised R-8): a lock call whose args do not fit its registry layout says why, quietly.
describe('LockCallExpr decode error (R-8)', () => {
  const lc = {
    lockCodeHash: '0xjoyid',
    lockHashType: 'type',
    lockArgs: '0x0001' + '55'.repeat(21),
    scriptHash: '0xj',
    scriptName: 'JoyID',
  };
  it('adds the reason after the call when the API could not decode the args', () => {
    const reason = 'JoyID: args are 23 bytes, layout subType(2) + pubkeyHash(20) expects 22';
    render(<LockCallExpr lc={{ ...lc, decodeError: reason }} />);
    expect(screen.getByText(`args not decoded: ${reason}`)).toHaveClass('text-text-dim');
  });
  it('says nothing more for decoded args or a lock with no layout', () => {
    const { container } = render(<LockCallExpr lc={{ ...lc, decodeError: null }} />);
    expect(container).not.toHaveTextContent('not decoded');
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
  it('keeps all 8 decimals, ghosting the trailing zeros, coloured by sign; zero is a ghost 0', () => {
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
    // Consistency plan C4 (old: "0.00000000 CKB" in text-text): zero is "0 CKB", the 0 ghosted.
    expect(zero).toHaveTextContent(/^0 CKB$/);
    expect(zero.querySelector('[data-ghost]')).toHaveTextContent(/^0$/);
    expect(zero.querySelector('[data-ghost]')).toHaveClass('text-text-ghost');
  });

  it('states zero as a ghost "0 CKB" with no fraction for every caller (C4)', () => {
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
      const digits = amount.querySelector('[data-ghost]');
      expect(digits).toHaveTextContent(/^0$/);
      expect(digits).toHaveClass('text-text-ghost');
    }
    // The unit as today: dim, meta, after a space.
    const unit = feed.lastElementChild;
    expect(unit).toHaveTextContent(/^CKB$/);
    expect(unit).toHaveClass('text-text-dim', 'text-meta');
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

describe('ItemDeltaFact marks (IV-24, TX-12, TX-29)', () => {
  it('puts a 12 px class mark before every item change', () => {
    const { container } = render(
      <>
        <ItemDeltaFact
          item={{
            kind: 'token',
            typeScriptHash: '0xseal',
            delta: '5',
            symbol: 'Seal',
            decimals: 0,
          }}
        />
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
        item={{
          kind: 'object',
          objectId: '0x51c75a0000000000000096f2',
          delta: -1,
          collection: { id: '0xd585', name: 'Nervape 3x' },
          composition: 'btc_ckb',
        }}
      />
    );
    expect(container).toHaveTextContent('Nervape 3x 0x51c75a...96f2 departed');
    expect(container.querySelector('[data-composition="btc_ckb"]')).not.toBeNull();
    rerender(
      <ItemDeltaFact item={{ kind: 'object', objectId: '0x51c75a0000000000000096f2', delta: -1 }} />
    );
    // No field, no badge and no guessed name.
    expect(container.querySelector('[data-composition]')).toBeNull();
    expect(container).toHaveTextContent('object 0x51c75a...96f2 departed');
  });
});

describe('ProtocolActionNote — the action metadata in one quiet line', () => {
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
    expect(document.body).toHaveTextContent('channel 0x0d3870bb...6e26:0 · 750.00000000 CKB');
    // The channel's out point is its funding cell: it opens that cell's page.
    expect(screen.getByRole('link', { name: '0x0d3870bb...6e26:0' })).toHaveAttribute(
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
  it('names an unregistered script by its code hash, not its script hash (TX-31)', () => {
    render(
      <LockCallExpr
        lc={{
          lockCodeHash: `0x43b292${'00'.repeat(25)}895d`,
          lockHashType: 'type',
          lockArgs: `0x031a9d${'00'.repeat(14)}93d4`,
          scriptHash: `0xba0e5590${'00'.repeat(26)}aaaa`,
        }}
      />
    );
    expect(screen.getByRole('link', { name: '0x43b292...895d' })).toBeInTheDocument();
    expect(screen.queryByText(/0xba0e5590/)).not.toBeInTheDocument();
  });

  it('prints no parentheses for a call with empty args (boards show none)', () => {
    const { container } = render(
      <TypeCallExpr
        sc={{
          typeCodeHash: '0xtime',
          typeHashType: 'type',
          typeArgs: '0x',
          scriptHash: '0xs1',
          scriptName: '.bit Time Info',
        }}
      />
    );
    expect(container).toHaveTextContent(/^\.bit Time Info$/);
  });

  it('draws arrived / departed quiet, the item itself bright (AssetClasses, Main boards)', () => {
    render(
      <ItemDeltaFact
        item={{ kind: 'object', objectId: `0x51c7${'00'.repeat(28)}96f2`, delta: 1 }}
      />
    );
    expect(screen.getByText('arrived')).toHaveClass('text-text-dim');
  });

  it('colours a token amount by its sign when a page asks for it (Address board)', () => {
    const { rerender } = render(
      <ItemDeltaFact
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
    const { rerender } = render(<ItemDeltaFact tone="sign" item={item} />);
    // Default: the symbol shares the amount's ink (Address board, unchanged).
    expect(screen.getByText(/iCKB$/)).toBe(screen.getByRole('link'));
    rerender(<ItemDeltaFact tone="sign" symbol="dim" item={item} />);
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
