import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ReadoutRow } from '@/components/ui/readout-row';

describe('ReadoutRow (the line under a figure that a tap pins, R6)', () => {
  it('says "tapped" before a pinned reading and always offers "tap or hover"', () => {
    render(
      <ReadoutRow label="Party or pair readout" pinned>
        kdeakp · 165 channels
      </ReadoutRow>
    );
    const row = screen.getByRole('status', { name: 'Party or pair readout' });
    expect(row).toHaveTextContent(/^tapped kdeakp · 165 channels/);
    const hint = screen.getByText('tap or hover');
    expect(hint).toHaveClass('whitespace-nowrap');
    expect(hint).toHaveClass('text-meta');
    expect(row).toHaveClass('text-small');
    expect(row).toHaveClass('border-t');
  });

  it('reads a hover without "tapped", and shows the idle prompt when nothing is chosen', () => {
    const { rerender } = render(
      <ReadoutRow label="r" pinned={false} idle="Tap or hover a party or a pair.">
        kdeakp
      </ReadoutRow>
    );
    expect(screen.getByRole('status', { name: 'r' })).not.toHaveTextContent('tapped');
    rerender(
      <ReadoutRow label="r" pinned={false} idle="Tap or hover a party or a pair.">
        {null}
      </ReadoutRow>
    );
    expect(screen.getByText('Tap or hover a party or a pair.')).toHaveClass('text-text-dim');
  });

  it('refuses a pinned row with nothing to read', () => {
    expect(() =>
      render(
        <ReadoutRow label="r" pinned idle="x">
          {null}
        </ReadoutRow>
      )
    ).toThrow(/pinned with nothing to read/);
  });
});
