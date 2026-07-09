import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { DeepForkAlert } from '@/components/deep-fork-alert';
import { DeepForkStatus } from '@/lib/api';

describe('DeepForkAlert', () => {
  const activeDeepFork: DeepForkStatus = {
    detected: true,
    detectedAt: '2024-01-15T10:30:00Z',
    depth: 50,
    dbTip: 1000,
    chainTip: 1050,
    forkPoint: 950,
  };

  const noDeepFork: DeepForkStatus = {
    detected: false,
    detectedAt: null,
    depth: null,
    dbTip: null,
    chainTip: null,
    forkPoint: null,
  };

  it('renders nothing when not detected', () => {
    const { container } = render(<DeepForkAlert status={noDeepFork} />);
    expect(container.firstChild).toBeNull();
  });

  it('renders alert details and details link when a deep fork is detected', () => {
    render(<DeepForkAlert status={activeDeepFork} />);

    expect(screen.getByText('Chain Fork Detected - Sync Paused')).toBeInTheDocument();
    expect(screen.getByText('50 blocks')).toBeInTheDocument();
    expect(screen.getByText('#1,000')).toBeInTheDocument();
    expect(screen.getByText('#1,050')).toBeInTheDocument();
    expect(screen.getByText('#950')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /View Details/i })).toHaveAttribute(
      'href',
      '/mainnet/forks'
    );
  });
});
