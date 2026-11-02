import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ReadoutRow } from '@/components/ui/readout-row';

afterEach(() => vi.unstubAllGlobals());

describe('ReadoutRow (the line under a figure that a tap pins, R6)', () => {
  // C11: with a mouse the pinned reading was selected, not tapped (jsdom is a pointer device).
  it('says "selected" before a pinned reading and always offers "tap or hover"', () => {
    render(
      <ReadoutRow label="Party or pair readout" pinned>
        kdeakp · 165 channels
      </ReadoutRow>
    );
    const row = screen.getByRole('status', { name: 'Party or pair readout' });
    expect(row).toHaveTextContent(/^selected kdeakp · 165 channels/);
    expect(row).not.toHaveTextContent('tapped');
    const hint = screen.getByText('tap or hover');
    expect(hint).toHaveClass('whitespace-nowrap');
    expect(hint).toHaveClass('text-meta');
    expect(row).toHaveClass('text-small');
    expect(row).toHaveClass('border-t');
  });

  it('says "tapped" where the pin can only be a tap (a touch screen)', () => {
    vi.stubGlobal(
      'matchMedia',
      vi.fn((query: string) => ({
        matches: query === '(pointer: coarse)',
        addEventListener: () => {},
        removeEventListener: () => {},
      }))
    );
    render(
      <ReadoutRow label="r" pinned>
        kdeakp
      </ReadoutRow>
    );
    expect(screen.getByRole('status', { name: 'r' })).toHaveTextContent(/^tapped kdeakp/);
  });

  // C10: a section of several figures says "tap or hover" once; its readout rows leave it out.
  it('leaves the hint out when the section states it once (hint={false})', () => {
    render(
      <ReadoutRow label="r" pinned={false} hint={false}>
        kdeakp
      </ReadoutRow>
    );
    const row = screen.getByRole('status', { name: 'r' });
    expect(row).toHaveTextContent(/^kdeakp$/);
    expect(row.querySelector('[data-hint="tap-or-hover"]')).toBeNull();
  });

  it('reads a hover without "selected", and shows the idle prompt when nothing is chosen', () => {
    const { rerender } = render(
      <ReadoutRow label="r" pinned={false} idle="Tap or hover a party or a pair.">
        kdeakp
      </ReadoutRow>
    );
    expect(screen.getByRole('status', { name: 'r' })).not.toHaveTextContent(/selected|tapped/);
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
