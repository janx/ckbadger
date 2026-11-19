// Lane P2's follow-up (item 3): a list drops a column or a sub-line that is the same on every row
// of its page and says it once over the list, in the words the objects list already used ("every
// one of the N … listed · …"), by one helper for every list that does it.
import { render, screen } from '@/__tests__/utils/test-utils';
import { SaidOnce, onEveryRow } from '@/components/asset/said-once';

describe('onEveryRow', () => {
  it('is true where every row of two or more says the same, null included', () => {
    expect(onEveryRow([{ a: 'x' }, { a: 'x' }], (r) => r.a)).toBe(true);
    expect(onEveryRow([{ a: null }, { a: null }], (r) => r.a)).toBe(true);
    expect(onEveryRow([{ a: 'x' }, { a: 'y' }], (r) => r.a)).toBe(false);
  });

  it('says nothing of one row or none: a single row is no column worth folding', () => {
    expect(onEveryRow([{ a: 'x' }], (r) => r.a)).toBe(false);
    expect(onEveryRow([], (r: { a: string }) => r.a)).toBe(false);
  });
});

describe('SaidOnce', () => {
  it('leads with the list it speaks for, then each shared value, in the reading ink', () => {
    render(
      <SaidOnce
        count={20}
        singular="holder"
        parts={[<span key="lock">.bit Lock</span>, <span key="x">minted 2024-01-02</span>]}
      />
    );
    const once = screen.getByText('.bit Lock').closest('[data-role="said-once"]')!;
    expect(once.tagName).toBe('P');
    expect(once).toHaveClass('text-text', 'max-w-measure');
    expect(once).toHaveTextContent(
      /^every one of the 20\sholders listed · \.bit Lock · minted 2024-01-02$/
    );
    // the lead is a label of the parts: quiet
    expect(screen.getByText(/every one of the/)).toHaveClass('text-text-dim');
  });

  it('draws nothing where no value is shared', () => {
    const { container } = render(<SaidOnce count={20} singular="holder" parts={[]} />);
    expect(container.querySelector('[data-role="said-once"]')).toBeNull();
  });
});
