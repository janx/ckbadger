import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { HeroStatRow } from '@/components/hero-stat-row';
import type { NetworkStats } from '@/lib/api';

function mockStats(overrides: Partial<NetworkStats> = {}): NetworkStats {
  return {
    latestBlock: 14235678,
    avgBlockTime: '8.2s',
    hashRate: '1.23 EH/s',
    difficulty: '2.34 P',
    epoch: '8234(150/1800)',
    tps: '1.23',
    estimatedEpochTime: '4h',
    transactionsPerMinute: '12',
    transactionsPerDay: '17280',
    syncStatus: {
      isSyncing: false,
      syncedBlock: 14235678,
      tipBlock: 14235678,
      progress: 100,
      estimatedTime: null,
      chartDataMayBeIncomplete: false,
      blocksPerSecond: null,
      emaBlocksPerSecond: null,
      syncMode: 'normal',
      startedAt: null,
      elapsedTime: null,
      totalTime: null,
    },
    deepForkStatus: {
      detected: false,
      detectedAt: null,
      depth: null,
      dbTip: null,
      chainTip: null,
      forkPoint: null,
    },
    knowledgeSize: '19850000000000000000',
    circulatingSupply: '4380000000000000000',
    daoLocked: '1120000000000000000',
    ...overrides,
  };
}

describe('HeroStatRow', () => {
  it('renders formatted stat values and links to detail pages', () => {
    render(<HeroStatRow stats={mockStats()} />);

    expect(screen.getByText('Knowledge Size')).toBeInTheDocument();
    expect(screen.getByText('Circulating')).toBeInTheDocument();
    expect(screen.getByText('DAO Locked')).toBeInTheDocument();
    expect(screen.getByText('Block Height')).toBeInTheDocument();
    expect(screen.getByText('Epoch')).toBeInTheDocument();
    expect(screen.getByText('#14,235,678')).toBeInTheDocument();
    expect(screen.getByText('#8,234')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Knowledge Size/i })).toHaveAttribute(
      'href',
      '/charts/knowledge-size'
    );
    expect(screen.getByRole('link', { name: /Circulating/i })).toHaveAttribute(
      'href',
      '/charts/total-supply'
    );
    expect(screen.getByRole('link', { name: /DAO Locked/i })).toHaveAttribute(
      'href',
      '/nervos-dao'
    );
    expect(screen.getByRole('link', { name: /Block Height/i })).toHaveAttribute(
      'href',
      '/blocks/14235678'
    );
    expect(screen.getByRole('link', { name: /Epoch/i })).toHaveAttribute(
      'href',
      '/charts/epoch-time-length'
    );
  });

  it('hides stat links while loading', () => {
    render(<HeroStatRow stats={null} />);

    expect(screen.queryAllByRole('link')).toHaveLength(0);
    expect(screen.queryByText('Knowledge Size')).not.toBeInTheDocument();
    expect(screen.queryByText('Block Height')).not.toBeInTheDocument();
  });
});
