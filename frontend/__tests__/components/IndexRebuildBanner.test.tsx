import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { IndexRebuildBanner } from '@/components/stats-cards';
import type { IndexRebuildStatus } from '@/lib/api';

describe('IndexRebuildBanner', () => {
  const activeRebuild: IndexRebuildStatus = {
    isRebuilding: true,
    total: 28,
    completed: 14,
    currentIndex: 'idx_cells_lock_live',
    failed: [],
    progress: 50,
    startedAt: '2024-01-15T10:30:00Z',
  };

  const completedRebuild: IndexRebuildStatus = {
    isRebuilding: false,
    total: 28,
    completed: 28,
    currentIndex: null,
    failed: [],
    progress: 100,
    startedAt: '2024-01-15T10:30:00Z',
  };

  const rebuildWithFailures: IndexRebuildStatus = {
    isRebuilding: true,
    total: 28,
    completed: 20,
    currentIndex: 'idx_tx_hash',
    failed: ['idx_blocks_hash', 'idx_cells_outpoint'],
    progress: 71.43,
    startedAt: '2024-01-15T10:30:00Z',
  };

  it('renders nothing when not rebuilding', () => {
    const { container } = render(<IndexRebuildBanner status={completedRebuild} />);
    expect(container.firstChild).toBeNull();
  });

  it('renders banner when actively rebuilding', () => {
    render(<IndexRebuildBanner status={activeRebuild} />);
    expect(screen.getByText('REBUILDING INDEXES...')).toBeInTheDocument();
  });

  it('shows progress percentage', () => {
    render(<IndexRebuildBanner status={activeRebuild} />);
    expect(screen.getByText('50.0')).toBeInTheDocument();
  });

  it('shows completed/total count', () => {
    render(<IndexRebuildBanner status={activeRebuild} />);
    expect(screen.getByText('14')).toBeInTheDocument();
    expect(screen.getByText('28')).toBeInTheDocument();
  });

  it('shows current index name', () => {
    render(<IndexRebuildBanner status={activeRebuild} />);
    expect(screen.getByText('Current: idx_cells_lock_live')).toBeInTheDocument();
  });

  it('renders with failures', () => {
    render(<IndexRebuildBanner status={rebuildWithFailures} />);
    expect(screen.getByText('REBUILDING INDEXES...')).toBeInTheDocument();
    expect(screen.getByText('71.4')).toBeInTheDocument();
  });

  it('has amber color scheme for active rebuild', () => {
    const { container } = render(<IndexRebuildBanner status={activeRebuild} />);
    const banner = container.firstChild as HTMLElement;
    expect(banner).toHaveClass('border-amber-500/30');
  });

  it('shows progress bar with correct width', () => {
    const { container } = render(<IndexRebuildBanner status={activeRebuild} />);
    const progressBar = container.querySelector('[style*="width: 50%"]');
    expect(progressBar).toBeInTheDocument();
  });
});
