import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { TopicHero } from '@/components/ui/topic-hero';

describe('TopicHero (Fiber / Forks board hero)', () => {
  it('sets the index title and a body lede beside a grid of hero figures with meta labels, in order (R1)', () => {
    render(
      <TopicHero
        title="Forks"
        lede="Two miners sometimes find a block at the same height."
        figures={[
          { label: 'Uncle rate · 30 days', value: '2.57%', sub: '7,178 uncles in 279,561 blocks' },
          { label: 'This node switched branch', value: '0 times', tone: 'jade' },
        ]}
      />
    );
    const title = screen.getByRole('heading', { level: 1, name: 'Forks' });
    expect(title).toHaveClass('text-index');
    expect(screen.getByText('Two miners sometimes find a block at the same height.')).toHaveClass(
      'text-body'
    );
    const terms = screen.getAllByRole('term');
    expect(terms.map((t) => t.textContent)).toEqual([
      'Uncle rate · 30 days',
      'This node switched branch',
    ]);
    for (const term of terms) expect(term).toHaveClass('text-meta', 'text-text-dim');
    expect(screen.getByText('2.57%')).toHaveClass('text-hero', 'text-text-bright');
    expect(screen.getByText('0 times')).toHaveClass('text-jade');
    expect(screen.getByText('7,178 uncles in 279,561 blocks')).toHaveClass('text-meta');
  });

  it('renders no figure grid while the figures are still loading', () => {
    render(<TopicHero title="Fiber channels" lede="Lede." figures={undefined} />);
    expect(screen.queryByRole('term')).toBeNull();
  });
});
