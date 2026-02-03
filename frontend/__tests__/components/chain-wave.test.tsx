import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { ProposalsContainer } from '@/components/chain-wave/proposals-container';
import { EpochProgress } from '@/components/chain-wave/epoch-progress';
import type { PendingProposal } from '@/lib/api';

function createProposal(
  proposalId: string,
  overrides: Partial<PendingProposal> = {}
): PendingProposal {
  return {
    proposalId,
    fullTxHash: null,
    proposedAtBlock: 1000,
    proposedAtIndex: 0,
    blocksUntilExpiry: 5,
    fee: null,
    size: null,
    cycles: null,
    feeRate: null,
    ...overrides,
  };
}

describe('ProposalsContainer', () => {
  it('renders empty state when no proposals', () => {
    render(<ProposalsContainer proposals={[]} totalCount={0} />);
    expect(screen.getByText('No proposed txs')).toBeInTheDocument();
    expect(screen.getByText('0')).toBeInTheDocument();
  });

  it('renders proposal short IDs', () => {
    const proposals = [
      createProposal('0x1234567890abcdef1234'),
      createProposal('0xfedcba0987654321fedc'),
    ];
    render(<ProposalsContainer proposals={proposals} totalCount={2} />);

    expect(screen.getByText(/0x123456/)).toBeInTheDocument();
    expect(screen.getByText(/0xfedcba/)).toBeInTheDocument();
    expect(screen.getByText('2')).toBeInTheDocument();
  });

  it('displays correct title and subtitle', () => {
    render(<ProposalsContainer proposals={[]} totalCount={0} />);
    expect(screen.getByText('Proposed')).toBeInTheDocument();
    expect(screen.getByText('Awaiting commit')).toBeInTheDocument();
  });

  it('displays fee rate when available', () => {
    const proposals = [createProposal('0xabcdef1234567890abcd', { feeRate: 1.5 })];
    render(<ProposalsContainer proposals={proposals} totalCount={1} />);
    expect(screen.getByText('1.5')).toBeInTheDocument();
  });
});

describe('EpochProgress', () => {
  it('renders epoch number and progress', () => {
    render(
      <EpochProgress epochNumber={100} epochIndex={450} epochLength={1800} latestBlock={10450} />
    );
    expect(screen.getByText('100')).toBeInTheDocument();
    expect(screen.getByText('25.0%')).toBeInTheDocument();
    expect(screen.getByText('450 / 1,800')).toBeInTheDocument();
  });

  it('renders estimated time remaining', () => {
    render(
      <EpochProgress
        epochNumber={100}
        epochIndex={900}
        epochLength={1800}
        latestBlock={10900}
        estimatedTimeRemaining="2h 30m"
      />
    );
    expect(screen.getByText('2h 30m')).toBeInTheDocument();
  });

  it('renders epoch start and end block numbers', () => {
    render(
      <EpochProgress epochNumber={100} epochIndex={450} epochLength={1800} latestBlock={10450} />
    );
    expect(screen.getByText('#10,000')).toBeInTheDocument();
    expect(screen.getByText('#11,799')).toBeInTheDocument();
  });

  it('calculates progress percentage correctly', () => {
    render(
      <EpochProgress epochNumber={1} epochIndex={1800} epochLength={1800} latestBlock={3600} />
    );
    expect(screen.getByText('100.0%')).toBeInTheDocument();
  });

  it('handles zero epoch length gracefully', () => {
    render(<EpochProgress epochNumber={1} epochIndex={0} epochLength={0} latestBlock={0} />);
    expect(screen.getByText('0.0%')).toBeInTheDocument();
  });
});
