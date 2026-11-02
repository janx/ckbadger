import { render, screen } from '@testing-library/react';
import { Utc8Date } from '@/components/ui/utc8-date';

// Lane P: one dated form outside the activity rows — the UTC+8 date, the age its hover.
describe('Utc8Date', () => {
  it('writes the UTC+8 date and keeps the age in its title', () => {
    const ts = Date.now() - 3 * 86_400_000;
    render(<Utc8Date timestamp={String(ts)} className="text-text-dim" />);
    const el = screen.getByText(/^\d{4}-\d\d-\d\d$/);
    expect(el.tagName).toBe('TIME');
    expect(el).toHaveAttribute('title', '3d ago');
    expect(el).toHaveClass('text-text-dim');
    // a UTC+8 day: 16:30 UTC is the next day in UTC+8
    const { container } = render(<Utc8Date timestamp={Date.UTC(2026, 0, 1, 16, 30)} />);
    expect(container.querySelector('time')).toHaveTextContent('2026-01-02');
  });
});
