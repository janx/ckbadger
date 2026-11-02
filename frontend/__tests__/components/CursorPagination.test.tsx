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

    // C11: one wording on every list — the range with an en dash, the page size after a dot.
    expect(screen.getByText('Showing 21–40 of 105 Spores · 20 per page')).toBeInTheDocument();
    expect(screen.getByText('Page 2 of 6')).toBeInTheDocument();
  });

  it('shows no count and no page total for an unknown total (round 3 L2: total null)', () => {
    render(
      <CursorPagination
        total={null}
        totalLabel="activities"
        page={1}
        pageSize={50}
        currentCount={50}
        hasMore
        hasPrevious={false}
        onNext={() => {}}
        onPrevious={() => {}}
      />
    );
    expect(screen.getByText(/Showing 1–50/)).toHaveTextContent(
      /^Showing 1–50 activities · 50 per page$/
    );
    expect(screen.queryByText(/ of /)).toBeNull();
    expect(screen.getByRole('button', { name: 'Next' })).toBeEnabled();
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

// Responsive fix plan 2026-11-01 (A2): below sm the four items no longer share one row — the count
// sentence wrapped and "Page 1 of 10,419" broke over three lines between the buttons (report B#16,
// A#18, C#14). Two rows below sm, one row from sm up as before.
describe('CursorPagination across widths', () => {
  function pager() {
    return render(
      <CursorPagination
        hasMore
        hasPrevious={false}
        onNext={vi.fn()}
        onPrevious={vi.fn()}
        total={250043}
        pageSize={24}
        page={1}
      />
    );
  }

  it('puts the count sentence on its own row and the controls on one row below sm', () => {
    const { container } = pager();
    const root = container.firstElementChild as HTMLElement;
    expect(root).toHaveClass('flex-col', 'sm:flex-row', 'sm:items-center', 'sm:justify-between');
    const [count, controls] = [...root.children] as HTMLElement[];
    expect(count).toHaveTextContent(/^250,043 items · 24 per page$/);
    expect(controls).toHaveAttribute('data-role', 'pager-controls');
    expect([...controls.children].map((c) => c.textContent)).toEqual([
      'Previous',
      'Page 1 of 10,419',
      'Next',
    ]);
    expect(controls).toHaveClass('flex', 'items-center', 'justify-between');
    // Lane P: in a column narrower than both, the controls drop under the sentence as one unit.
    expect(root).toHaveClass('sm:flex-wrap');
    expect(controls).not.toHaveClass('flex-wrap');
  });

  it('never breaks "Page x of y" and keeps the buttons whole', () => {
    pager();
    expect(screen.getByText('Page 1 of 10,419')).toHaveClass('whitespace-nowrap');
    for (const name of ['Previous', 'Next'])
      expect(screen.getByRole('button', { name })).toHaveClass('shrink-0', 'whitespace-nowrap');
  });
});

// Consistency round C11: one pager. Every list says "Showing a–b of N <noun> · S per page" and
// "Page x of y"; the range is one text node, so it reads (and is found) as one phrase.
describe('CursorPagination wording (C11)', () => {
  it('reads one phrase for the range, the total, the noun and the page size', () => {
    render(
      <CursorPagination
        total={1298}
        totalLabel="objects"
        pageSize={20}
        page={1}
        currentCount={20}
        hasMore
        hasPrevious={false}
        onNext={vi.fn()}
        onPrevious={vi.fn()}
      />
    );
    const line = screen.getByText('Showing 1–20 of 1,298 objects · 20 per page');
    expect(line.childNodes).toHaveLength(1);
    expect(screen.getByText('Page 1 of 65')).toBeInTheDocument();
    expect(document.body.textContent).not.toMatch(/\d-\d|, \d+ per page/);
  });

  it('counts the last, short page from its rows', () => {
    render(
      <CursorPagination
        total={45}
        totalLabel="holders"
        pageSize={20}
        page={3}
        currentCount={5}
        hasMore={false}
        hasPrevious
        onNext={vi.fn()}
        onPrevious={vi.fn()}
      />
    );
    expect(screen.getByText('Showing 41–45 of 45 holders · 20 per page')).toBeInTheDocument();
    expect(screen.getByText('Page 3 of 3')).toBeInTheDocument();
  });

  it('refuses a page that holds more rows than its page size', () => {
    expect(() =>
      render(
        <CursorPagination
          total={100}
          totalLabel="rows"
          pageSize={20}
          page={1}
          currentCount={21}
          hasMore
          hasPrevious={false}
          onNext={vi.fn()}
          onPrevious={vi.fn()}
        />
      )
    ).toThrow(/21 rows on a page of 20/);
  });
});
