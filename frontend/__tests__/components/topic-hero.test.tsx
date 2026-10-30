import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { TopicHero } from '@/components/ui/topic-hero';

describe('TopicHero (Fiber / Forks board hero)', () => {
  it('sets the 26 px title and plain intro beside a two-column figure grid, in the given order', () => {
    render(
      <TopicHero
        title="Forks"
        intro="Two miners sometimes find a block at the same height."
        figures={[
          { label: 'Uncle rate · 30 days', value: '2.57%', sub: '7,178 uncles in 279,561 blocks' },
          { label: 'This node switched branch', value: '0 times', tone: 'jade' },
        ]}
      />
    );
    const title = screen.getByRole('heading', { level: 1, name: 'Forks' });
    expect(title).toHaveClass('text-[26px]');
    expect(screen.getByText('Two miners sometimes find a block at the same height.')).toHaveClass(
      'text-[15px]'
    );
    const terms = screen.getAllByRole('term').map((t) => t.textContent);
    expect(terms).toEqual(['Uncle rate · 30 days', 'This node switched branch']);
    expect(screen.getByText('2.57%')).toHaveClass('text-[18px]', 'text-text-bright');
    expect(screen.getByText('0 times')).toHaveClass('text-jade');
    expect(screen.getByText('7,178 uncles in 279,561 blocks')).toHaveClass('text-[11.5px]');
  });

  it('renders no figure grid while the figures are still loading', () => {
    render(<TopicHero title="Fiber channels" intro="Intro." figures={undefined} />);
    expect(screen.queryByRole('term')).toBeNull();
  });
});
