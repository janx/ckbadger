import { fireEvent, render, screen } from '@testing-library/react';
import { CursorPagination } from '@/components/ui/cursor-pagination';

describe('CursorPagination', () => {
  it('renders button controls with type=button', () => {
    render(
      <CursorPagination
        hasMore
        hasPrevious
        onNext={vi.fn()}
        onPrevious={vi.fn()}
        total={100}
        page={2}
        pageSize={20}
      />
    );

    expect(screen.getByRole('button', { name: 'Previous' })).toHaveAttribute('type', 'button');
    expect(screen.getByRole('button', { name: 'Next' })).toHaveAttribute('type', 'button');
  });

  it('invokes callbacks when navigating', () => {
    const onNext = vi.fn();
    const onPrevious = vi.fn();
    render(
      <CursorPagination hasMore hasPrevious onNext={onNext} onPrevious={onPrevious} page={1} />
    );

    fireEvent.click(screen.getByRole('button', { name: 'Previous' }));
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    expect(onPrevious).toHaveBeenCalledTimes(1);
    expect(onNext).toHaveBeenCalledTimes(1);
  });

  it('shows detailed range and page information', () => {
    render(
      <CursorPagination
        hasMore
        hasPrevious
        onNext={vi.fn()}
        onPrevious={vi.fn()}
        total={105}
        page={2}
        pageSize={20}
        currentCount={20}
        totalLabel="Spores"
      />
    );

    expect(screen.getByText('Showing 21-40 of 105 Spores, 20 per page')).toBeInTheDocument();
    expect(screen.getByText('Page 2 of 6')).toBeInTheDocument();
  });
});
