import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { IndexRebuildBanner } from '@/components/stats-cards';
import type { IndexRebuildStatus } from '@/lib/api';

describe('IndexRebuildBanner', () => {
  const runningRebuild: IndexRebuildStatus = {
    status: 'running',
    isRebuilding: true,
    total: 28,
    completed: 14,
    currentIndex: 'idx_cells_lock_live',
    failed: [],
    progress: 50,
    startedAt: '2024-01-15T10:30:00Z',
  };

  const pendingRebuild: IndexRebuildStatus = {
    status: 'pending',
    isRebuilding: false,
    total: 0,
    completed: 0,
    currentIndex: null,
    failed: [],
    progress: 0,
    startedAt: null,
  };

  const rebuildWithFailures: IndexRebuildStatus = {
    status: 'running',
    isRebuilding: true,
    total: 28,
    completed: 20,
    currentIndex: 'idx_tx_hash',
    failed: ['idx_blocks_hash', 'idx_cells_outpoint'],
    progress: 71.43,
    startedAt: '2024-01-15T10:30:00Z',
  };

  it('renders banner when pending', () => {
    render(<IndexRebuildBanner status={pendingRebuild} />);
    expect(screen.getByText('INDEX REBUILD PENDING...')).toBeInTheDocument();
    expect(screen.getByText('Waiting for task runner...')).toBeInTheDocument();
  });

  it('renders banner when actively rebuilding', () => {
    render(<IndexRebuildBanner status={runningRebuild} />);
    expect(screen.getByText('REBUILDING INDEXES...')).toBeInTheDocument();
  });

  it('shows progress percentage when running', () => {
    render(<IndexRebuildBanner status={runningRebuild} />);
    expect(screen.getByText('50.0')).toBeInTheDocument();
  });

  it('shows completed/total count when running', () => {
    render(<IndexRebuildBanner status={runningRebuild} />);
    expect(screen.getByText('14')).toBeInTheDocument();
    expect(screen.getByText('28')).toBeInTheDocument();
  });

  it('does not show progress bar when pending', () => {
    const { container } = render(<IndexRebuildBanner status={pendingRebuild} />);
    const progressBar = container.querySelector('[style*="width"]');
    expect(progressBar).toBeNull();
  });

  it('shows current index name when running', () => {
    render(<IndexRebuildBanner status={runningRebuild} />);
    expect(screen.getByText('Current: idx_cells_lock_live')).toBeInTheDocument();
  });

  it('renders with failures', () => {
    render(<IndexRebuildBanner status={rebuildWithFailures} />);
    expect(screen.getByText('REBUILDING INDEXES...')).toBeInTheDocument();
    expect(screen.getByText('71.4')).toBeInTheDocument();
  });

  it('has amber color scheme for active rebuild', () => {
    const { container } = render(<IndexRebuildBanner status={runningRebuild} />);
    const banner = container.firstChild as HTMLElement;
    expect(banner).toHaveClass('border-amber-500/30');
  });

  it('has amber color scheme for pending rebuild', () => {
    const { container } = render(<IndexRebuildBanner status={pendingRebuild} />);
    const banner = container.firstChild as HTMLElement;
    expect(banner).toHaveClass('border-amber-500/30');
  });

  it('shows progress bar with correct width when running', () => {
    const { container } = render(<IndexRebuildBanner status={runningRebuild} />);
    const progressBar = container.querySelector('[style*="width: 50%"]');
    expect(progressBar).toBeInTheDocument();
  });
});
