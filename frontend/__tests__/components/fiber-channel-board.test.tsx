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
  stages: [],
  settlementOutputs: [],
};

describe('FiberChannelBoard', () => {
  it('keeps current commitment distinct from initial force close and shows precise protocol fields', () => {
    render(<FiberChannelBoard channel={forceClosed as FiberChannelDetail} evidence={evidence} />);
    expect(screen.getByText('Force-close waiting')).toBeInTheDocument();
    expect(screen.getByTestId('stage-Close')).toHaveTextContent(forceClosed.closeTxHash!);
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
});
