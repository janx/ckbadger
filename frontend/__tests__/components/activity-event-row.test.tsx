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
    assetChanges: overrides.assetChanges ?? [],
    scriptCalls: overrides.scriptCalls ?? [],
    peers: overrides.peers ?? [],
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
          assetChanges: [{ type: 'daoDeposit', capacity: '10200000000' }],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/DAO Deposit/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/CKB Transfer/).length).toBeGreaterThan(0);
  });

  it('renders token sub-row with symbol', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          assetChanges: [
            {
              type: 'token',
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

  it('renders script call sub-row', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          scriptCalls: [
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
    expect(screen.getAllByText(/Omnilock/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/Script call/).length).toBeGreaterThan(0);
  });

  it('renders protocol name instead of Script call when protocolName is set', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          scriptCalls: [
            {
              typeCodeHash: '0xcode',
              typeHashType: 'type',
              typeArgs: '0x1234',
              scriptHash: '0xhash',
              scriptName: 'Pool',
              protocolName: 'Stable++',
            },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/Stable\+\+/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/Pool/).length).toBeGreaterThan(0);
    // Should NOT show "Script call" when protocol name is present
    expect(screen.queryByText(/Script call/)).not.toBeInTheDocument();
  });

  it('renders multiple event types in one activity', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          ckbDelta: '-1000000000',
          assetChanges: [
            { type: 'token', typeScriptHash: '0xt', delta: '500', symbol: 'SEAL', decimals: 0 },
            { type: 'object', objectId: '0xobj123', standard: 'spore', action: 'mint' },
          ],
          scriptCalls: [
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
    // All four event types present: token, object, script call, CKB
    expect(screen.getAllByText(/SEAL Transfer/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/Spore Mint/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/Script call/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/CKB Transfer/).length).toBeGreaterThan(0);
  });

  it('renders DAO Withdraw Complete with compensation', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          assetChanges: [
            { type: 'daoWithdrawComplete', capacity: '20000000000', compensation: '500000000' },
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
          assetChanges: [
            { type: 'identity', identityId: '0xid123', standard: 'dotbit', action: 'register' },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/\.bit Register/).length).toBeGreaterThan(0);
  });

  it('renders time ago text', () => {
    render(<ActivityEventGroup activity={makeActivity()} formatTimeAgo={() => '5 mins ago'} />);
    expect(screen.getAllByText('5 mins ago').length).toBeGreaterThan(0);
  });
});
