import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { ActivityEventGroup } from '@/components/activity-event-row';
import type { Activity } from '@/lib/api';

function makeActivity(overrides: Partial<Activity> = {}): Activity {
  return {
    txHash:
      overrides.txHash ?? '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890',
    blockNumber: overrides.blockNumber ?? 10_000,
    txIndex: overrides.txIndex ?? 0,
    timestamp: overrides.timestamp ?? '1700000000',
    ckbDelta: overrides.ckbDelta ?? '0',
    usedDelta: overrides.usedDelta ?? '0',
    isCellbase: overrides.isCellbase ?? false,
    itemDeltas: overrides.itemDeltas ?? [],
    typeCalls: overrides.typeCalls ?? [],
    lockCalls: overrides.lockCalls ?? [],
    protocolActions: overrides.protocolActions ?? [],
    participants: overrides.participants ?? [],
    tags: overrides.tags ?? 0,
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

  it('renders identity sub-row', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          itemDeltas: [{ kind: 'identity', identityId: '0xid123', delta: 1 }],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/Identity/).length).toBeGreaterThan(0);
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
