import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ProposalRuler } from '@/components/transaction/proposal-ruler';

describe('ProposalRuler (TX-17)', () => {
  it('draws the proposal, the commit and the window on one block ruler', () => {
    const { container } = render(
      <ProposalRuler proposedIn={20801531} committedIn={20801533} window={{ close: 2, far: 10 }} />
    );
    const svg = screen.getByRole('img', {
      name: 'Proposed in block 20,801,531, committed 2 blocks later in 20,801,533; the commit window is 2 to 10 blocks',
    });
    // One tick per block of the window's far edge, a gold dot at the proposal, a jade square at
    // the commit two steps (48 px of 240) along, and the window line from +2 to +10.
    expect(svg.querySelectorAll('[data-tick]')).toHaveLength(10);
    expect(svg.querySelector('[data-proposal]')).toHaveAttribute('cx', '8');
    expect(svg.querySelector('[data-commit]')).toHaveAttribute('x', '52');
    expect(svg.querySelector('[data-window]')).toHaveAttribute('d', 'M56 11H248');
    expect(svg).toHaveTextContent('…531');
    expect(svg).toHaveTextContent('…533');
    expect(svg).toHaveTextContent('+10');
    expect(container.querySelector('[data-proposal]')).toHaveClass('fill-gold');
    expect(container.querySelector('[data-commit]')).toHaveClass('fill-jade');
  });

  it('refuses a commit outside its window', () => {
    expect(() =>
      render(<ProposalRuler proposedIn={100} committedIn={101} window={{ close: 2, far: 10 }} />)
    ).toThrow('committed 1 blocks after its proposal, outside the window 2–10');
  });
});
