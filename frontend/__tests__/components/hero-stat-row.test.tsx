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
  it('renders all 5 stat labels with data', () => {
    render(<HeroStatRow stats={mockStats()} />);

    expect(screen.getByText('Knowledge Size')).toBeInTheDocument();
    expect(screen.getByText('Circulating')).toBeInTheDocument();
    expect(screen.getByText('DAO Locked')).toBeInTheDocument();
    expect(screen.getByText('Block Height')).toBeInTheDocument();
    expect(screen.getByText('Epoch')).toBeInTheDocument();
  });

  it('renders formatted values', () => {
    render(<HeroStatRow stats={mockStats()} />);

    // Block height should be formatted with commas and # prefix
    expect(screen.getByText('#14,235,678')).toBeInTheDocument();

    // Epoch should extract number and format with commas
    expect(screen.getByText('#8,234')).toBeInTheDocument();
  });

  it('renders links to correct pages', () => {
    render(<HeroStatRow stats={mockStats()} />);

    const links = screen.getAllByRole('link');
    const hrefs = links.map((l) => l.getAttribute('href'));

    expect(hrefs).toContain('/charts/knowledge-size');
    expect(hrefs).toContain('/charts/total-supply');
    expect(hrefs).toContain('/nervos-dao');
    expect(hrefs).toContain('/blocks/14235678');
    expect(hrefs).toContain('/charts/epoch-time-length');
  });

  it('renders skeleton placeholders when stats is null', () => {
    const { container } = render(<HeroStatRow stats={null} />);

    const pulseElements = container.querySelectorAll('.animate-pulse');
    expect(pulseElements.length).toBeGreaterThanOrEqual(5);
  });
});
