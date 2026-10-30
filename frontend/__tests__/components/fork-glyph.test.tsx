import { describe, expect, it } from 'vitest';
import { render } from '../utils/test-utils';
import { ForkGlyph } from '@/components/forks/fork-glyph';

describe('ForkGlyph (Forks V67 row shape, FF-L3)', () => {
  it('draws the spine, the dropped block dashed, and a jade curve to the block that took it as an uncle', () => {
    const { container } = render(
      <ForkGlyph forkPoint={20835739} depth={1} uncleIn={20835742} fate="included_as_uncle" />
    );
    const svg = container.querySelector('svg')!;
    expect(svg.querySelectorAll('[data-slot]')).toHaveLength(4);
    expect(svg.querySelectorAll('[data-dropped]')).toHaveLength(1);
    expect(svg.querySelector('[data-dropped]')).toHaveAttribute('stroke-dasharray', '2 2');
    const link = svg.querySelector('[data-uncle-link]')!;
    expect(link).toHaveAttribute('stroke', '#2edba3');
    expect(link).toHaveAttribute('data-uncle-slot', '3');
  });

  it('draws two dropped blocks for a two-block switch, and none of the curve when it came back', () => {
    const deep = render(
      <ForkGlyph forkPoint={10} depth={2} uncleIn={13} fate="included_as_uncle" />
    );
    expect(deep.container.querySelectorAll('[data-dropped]')).toHaveLength(2);
    const back = render(<ForkGlyph forkPoint={10} depth={1} uncleIn={null} fate="canonical_now" />);
    expect(back.container.querySelector('[data-uncle-link]')).toBeNull();
    expect(back.container.querySelector('[data-came-back]')).not.toBeNull();
  });

  it('refuses an uncle inclusion that is not after the fork point', () => {
    expect(() =>
      render(<ForkGlyph forkPoint={10} depth={1} uncleIn={10} fate="included_as_uncle" />)
    ).toThrow('Fork #10: uncle inclusion #10 is not after the dropped block');
  });
});
