import { describe, it, expect, beforeAll } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { PackedContainer, TxItem } from '@/components/chain-wave/packed-container';
import { EpochProgress } from '@/components/chain-wave/epoch-progress';

// Mock ResizeObserver for PackedContainer tests
beforeAll(() => {
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
});

function createProposalItem(id: string, overrides: Partial<TxItem> = {}): TxItem {
  return {
    id,
    size: 500,
    fee: undefined,
    feeRate: undefined,
    category: 'normal',
    ...overrides,
  };
}

describe('PackedContainer (proposals type)', () => {
  it('renders empty state when no proposals', () => {
    render(
      <PackedContainer
        title="Proposed"
        subtitle="Awaiting commit"
        type="proposals"
        items={[]}
        totalCount={0}
        emptyText="No proposed txs"
        globalMaxSize={10000}
      />
    );
    expect(screen.getByText('No proposed txs')).toBeInTheDocument();
    expect(screen.getByText('0')).toBeInTheDocument();
  });

  it('renders proposal items as boxes', () => {
    const items = [
      createProposalItem('0x1234567890abcdef1234', { size: 1000 }),
      createProposalItem('0xfedcba0987654321fedc', { size: 2000 }),
    ];
    render(
      <PackedContainer
        title="Proposed"
        subtitle="Awaiting commit"
        type="proposals"
        items={items}
        totalCount={2}
        emptyText="No proposed txs"
        globalMaxSize={10000}
      />
    );

    expect(screen.getByText('2')).toBeInTheDocument();
    expect(screen.getByText('Proposed')).toBeInTheDocument();
  });

  it('displays correct title and subtitle', () => {
    render(
      <PackedContainer
        title="Proposed"
        subtitle="Awaiting commit"
        type="proposals"
        items={[]}
        totalCount={0}
        emptyText="No proposed txs"
        globalMaxSize={10000}
      />
    );
    expect(screen.getByText('Proposed')).toBeInTheDocument();
    expect(screen.getByText('Awaiting commit')).toBeInTheDocument();
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
