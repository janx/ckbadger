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

  it('shows no pager for an empty list instead of "Page 1 of 0" (FF-L5)', () => {
    const { container } = render(
      <CursorPagination
        hasMore={false}
        hasPrevious={false}
        onNext={vi.fn()}
        onPrevious={vi.fn()}
        total={0}
        totalLabel="events"
        page={1}
        pageSize={50}
        currentCount={0}
      />
    );
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByText(/of 0/)).toBeNull();
  });
});

// F2-tx (TX-27): the Transactions board pages "Newer / Older →", and every count is grouped.
describe('CursorPagination labels and grouping', () => {
  it('takes board labels and groups the page count', () => {
    render(
      <CursorPagination
        hasMore
        hasPrevious={false}
        onNext={vi.fn()}
        onPrevious={vi.fn()}
        total={52789419}
        page={1}
        pageSize={50}
        totalLabel="transactions"
        previousLabel="Newer"
        nextLabel="Older →"
      />
    );
    expect(screen.getByRole('button', { name: 'Newer' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Older →' })).toBeEnabled();
    expect(screen.getByText('Page 1 of 1,055,789')).toBeInTheDocument();
  });
});
