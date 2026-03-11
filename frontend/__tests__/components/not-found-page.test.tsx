import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { NotFoundPage } from '@/components/not-found-page';

describe('NotFoundPage', () => {
  beforeEach(() => {
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders 404 with poetry and terminal error', () => {
    render(<NotFoundPage />);

    // 404 characters present (two "4" spans + one "0" span)
    expect(screen.getAllByText('4', { exact: true })).toHaveLength(2);
    expect(screen.getByText('0', { exact: true })).toBeInTheDocument();

    // Poetry lines
    expect(
      screen.getByText('some common knowledge has dissolved into the void')
    ).toBeInTheDocument();
    expect(screen.getByText('yet more is crystallizing from the chain')).toBeInTheDocument();

    // Terminal error line
    expect(screen.getByText(/cell_not_found/)).toBeInTheDocument();

    // Return home link
    expect(screen.getByRole('link', { name: /return home/i })).toHaveAttribute('href', '/');

    // Header nav links still present
    expect(screen.getByRole('link', { name: 'DAO' })).toHaveAttribute('href', '/dao');
    expect(screen.getByRole('link', { name: 'Assets' })).toHaveAttribute('href', '/assets');
    expect(screen.getByRole('link', { name: 'Scripts' })).toHaveAttribute('href', '/scripts');
    expect(screen.getByRole('link', { name: 'Charts' })).toHaveAttribute('href', '/charts');

    // No debug UI
    expect(screen.queryByText('Ocean Tuning')).not.toBeInTheDocument();
    expect(screen.queryByText('Track Blocks')).not.toBeInTheDocument();
  });
});
