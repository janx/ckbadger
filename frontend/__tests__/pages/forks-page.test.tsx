import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ForksPage from '@/app/forks/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getForks: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

describe('ForksPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders fork events list with links and hashes', async () => {
    vi.mocked(api.getForks).mockResolvedValue({
      data: [
        {
          id: 1,
          eventType: 'deep_fork',
          depth: 3,
          oldTipNumber: 100,
          oldTipHash: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
          newTipNumber: 101,
          newTipHash: '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
          forkPointNumber: 98,
          forkPointHash: '0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
          orphanedBlocksCount: 2,
          orphanedTxsCount: 11,
          detectedAt: '2026-02-20T10:00:00Z',
          resolvedAt: null,
          resolvedBy: null,
          resolutionAction: null,
          resolutionNotes: null,
        },
      ],
      total: 1,
      limit: 25,
      hasMore: false,
      nextCursor: null,
    });

    render(<ForksPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Fork Events')).toBeInTheDocument();

    await waitFor(() => {
      expect(api.getForks).toHaveBeenCalledWith({ cursor: undefined, limit: 25 });
      expect(screen.getByText('DEEP_FORK')).toBeInTheDocument();
    });

    expect(screen.getByRole('link', { name: '#98' })).toHaveAttribute('href', '/blocks/98');
    expect(screen.getByText('2 blocks')).toBeInTheDocument();
    expect(
      document.querySelector(
        '[title="Click to copy: 0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]'
      )
    ).toBeTruthy();
  });
});
