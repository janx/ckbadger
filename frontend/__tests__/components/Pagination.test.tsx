import { render, screen, fireEvent } from '@testing-library/react';
import { Pagination } from '@/components/ui/pagination';

describe('Pagination', () => {
  const mockOnPageChange = vi.fn();

  beforeEach(() => {
    mockOnPageChange.mockClear();
  });

  describe('navigation buttons', () => {
    it('disables Prev button on first page', () => {
      render(<Pagination page={1} totalPages={10} onPageChange={mockOnPageChange} />);
      expect(screen.getByText('Prev')).toBeDisabled();
    });

    it('disables Next button on last page', () => {
      render(<Pagination page={10} totalPages={10} onPageChange={mockOnPageChange} />);
      expect(screen.getByText('Next')).toBeDisabled();
    });

    it('calls onPageChange with page-1 when Prev clicked', () => {
      render(<Pagination page={5} totalPages={10} onPageChange={mockOnPageChange} />);
      fireEvent.click(screen.getByText('Prev'));
      expect(mockOnPageChange).toHaveBeenCalledWith(4);
    });

    it('calls onPageChange with page+1 when Next clicked', () => {
      render(<Pagination page={5} totalPages={10} onPageChange={mockOnPageChange} />);
      fireEvent.click(screen.getByText('Next'));
      expect(mockOnPageChange).toHaveBeenCalledWith(6);
    });
  });

  describe('page numbers', () => {
    it('shows all pages when totalPages <= 7', () => {
      render(<Pagination page={3} totalPages={5} onPageChange={mockOnPageChange} />);
      for (let i = 1; i <= 5; i++) {
        expect(screen.getByText(String(i))).toBeInTheDocument();
      }
      expect(screen.queryByText('...')).not.toBeInTheDocument();
    });

    it('shows ellipsis for large page counts', () => {
      render(<Pagination page={5} totalPages={20} onPageChange={mockOnPageChange} />);
      expect(screen.getAllByText('...').length).toBeGreaterThan(0);
    });

    it('shows first pages when near start (page <= 3)', () => {
      render(<Pagination page={2} totalPages={20} onPageChange={mockOnPageChange} />);
      expect(screen.getByText('1')).toBeInTheDocument();
      expect(screen.getByText('2')).toBeInTheDocument();
      expect(screen.getByText('3')).toBeInTheDocument();
      expect(screen.getByText('4')).toBeInTheDocument();
      expect(screen.getByText('5')).toBeInTheDocument();
      expect(screen.getByText('20')).toBeInTheDocument();
    });

    it('shows last pages when near end (page >= total - 2)', () => {
      render(<Pagination page={18} totalPages={20} onPageChange={mockOnPageChange} />);
      expect(screen.getByText('1')).toBeInTheDocument();
      expect(screen.getByText('16')).toBeInTheDocument();
      expect(screen.getByText('17')).toBeInTheDocument();
      expect(screen.getByText('18')).toBeInTheDocument();
      expect(screen.getByText('19')).toBeInTheDocument();
      expect(screen.getByText('20')).toBeInTheDocument();
    });

    it('shows surrounding pages when in middle', () => {
      render(<Pagination page={10} totalPages={20} onPageChange={mockOnPageChange} />);
      expect(screen.getByText('1')).toBeInTheDocument();
      expect(screen.getByText('9')).toBeInTheDocument();
      expect(screen.getByText('10')).toBeInTheDocument();
      expect(screen.getByText('11')).toBeInTheDocument();
      expect(screen.getByText('20')).toBeInTheDocument();
    });

    it('calls onPageChange when page number clicked', () => {
      render(<Pagination page={5} totalPages={10} onPageChange={mockOnPageChange} />);
      fireEvent.click(screen.getByText('1'));
      expect(mockOnPageChange).toHaveBeenCalledWith(1);
    });
  });
});

// Responsive fix plan 2026-11-01 (A2): Prev, up to seven page buttons and Next are ~420 px of
// unshrinkable buttons — wider than a 358 px phone column. Below sm the page numbers take their own
// row and Prev / Next share the row under it; from sm up it is one row as before.
describe('Pagination across widths', () => {
  it('gives the page numbers their own row below sm and one row from sm', () => {
    const { container } = render(
      <Pagination page={5000} totalPages={10419} onPageChange={vi.fn()} />
    );
    const root = container.firstElementChild as HTMLElement;
    expect(root).toHaveClass('flex-wrap', 'justify-between', 'sm:flex-nowrap', 'sm:justify-center');
    const numbers = root.querySelector('[data-role="page-numbers"]') as HTMLElement;
    expect(numbers).toHaveClass('order-first', 'basis-full', 'sm:order-none', 'sm:basis-auto');
    expect(numbers.textContent).toBe('1...499950005001...10419');
    expect(screen.getByText('Prev')).toHaveClass('whitespace-nowrap');
    expect(screen.getByText('Next')).toHaveClass('whitespace-nowrap');
  });
});
