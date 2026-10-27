import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { FiberChannelBoard, observedDuration } from '@/components/fiber/fiber-channel-board';
import forceClosed from '../fixtures/fiber-force-closed.json';
import type { FiberChannelDetail, FiberChannelEvidence } from '@/lib/api';

vi.mock('@/components/cell/cell-transactions', () => ({
  TransactionEvidence: ({ hash, label }: { hash: string; label: string }) => (
    <div data-testid={`stage-${label}`}>{hash}</div>
  ),
}));

const evidence: FiberChannelEvidence = {
  channelId: forceClosed.channelId,
  funding: {
    txHash: forceClosed.fundingTxHash,
    outputIndex: 0,
    blockNumber: 10,
    capacity: forceClosed.capacity,
    lockCodeHash: `0x${'11'.repeat(32)}`,
    lockHashType: 'type',
    lockArgs: `0x${'22'.repeat(20)}`,
    isLive: false,
    consumedByTx: forceClosed.closeTxHash,
  },
  currentCommitment: {
    txHash: `0x${'55'.repeat(32)}`,
    outputIndex: 1,
    blockNumber: 25,
    capacity: forceClosed.capacity,
    lockCodeHash: `0x${'33'.repeat(32)}`,
    lockHashType: 'type',
    lockArgs: `0x${'44'.repeat(57)}`,
    isLive: true,
    consumedByTx: null,
    pubkeyHash: `0x${'44'.repeat(20)}`,
    delayEpoch: forceClosed.delayEpoch!,
    version: '9007199254740993',
    settlementHash: `0x${'66'.repeat(20)}`,
    settlementFlag: 1,
    lockFormat: 'full57',
    epochLock: 'subsequentSettlementNoReappliedDelay',
    readyEpoch: null,
    epochLockMetAtTip: null,
    tipEpoch: '120+1/3',
  },
  openingPartyFlows: [],
  stages: [
    {
      stage: 'currentCommitment',
      txHash: `0x${'55'.repeat(32)}`,
      blockNumber: 25,
      timestamp: '1700000200000',
      inputs: 1,
      outputs: 2,
      witnessCount: 1,
    },
  ],
  settlementOutputs: [],
};

describe('FiberChannelBoard', () => {
  it('keeps current commitment distinct from initial force close and shows precise protocol fields', () => {
    render(<FiberChannelBoard channel={forceClosed as FiberChannelDetail} evidence={evidence} />);
    expect(screen.getByText('Force-close waiting')).toBeInTheDocument();
    expect(screen.getByTestId('stage-Close')).toHaveTextContent(forceClosed.closeTxHash!);
    expect(screen.getByTestId('stage-Updated commitment')).toHaveTextContent(
      evidence.currentCommitment!.txHash
    );
    expect(
      screen.getByText(/non-Fiber locks seen on opening inputs or outputs/)
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Subsequent settlement cell: original delay is not reapplied/)
    ).toBeInTheDocument();
    expect(screen.getByText('9007199254740993')).toBeInTheDocument();
    expect(
      screen.getByRole('link', { name: `${evidence.currentCommitment!.txHash}:1` })
    ).toHaveAttribute('href', `/mainnet/cell/${evidence.currentCommitment!.txHash}-1`);
    expect(screen.getByText(/not a proven channel balance or payment/)).toBeInTheDocument();
  });

  it('formats a lifetime above JS safe integer without losing precision', () => {
    expect(observedDuration('9007199254740993')).toBe('104249991 d 8 h 59 m');
  });

  it('labels every final output, including an unmatched lock, without implying a party payout', () => {
    const settlementHash = `0x${'77'.repeat(32)}`;
    render(
      <FiberChannelBoard
        channel={
          {
            ...forceClosed,
            state: 'settled',
            settlementTxHash: settlementHash,
            settlementBlock: 30,
            settlementTimestamp: '1700000300000',
          } as FiberChannelDetail
        }
        evidence={{
          ...evidence,
          settlementOutputs: [
            {
              txHash: settlementHash,
              outputIndex: 0,
              ownerLockHash: `0x${'88'.repeat(32)}`,
              observedParty: false,
              capacity: '10000000000',
            },
          ],
        }}
      />
    );
    expect(screen.getByText(/All final transaction outputs are shown/)).toBeInTheDocument();
    expect(screen.getByText(/Other lock/)).toBeInTheDocument();
    expect(screen.getByText('04 SETTLEMENT')).toBeInTheDocument();
  });
});
