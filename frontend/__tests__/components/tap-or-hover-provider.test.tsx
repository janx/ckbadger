import { StrictMode } from 'react';
import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import {
  TapOrHoverHint,
  TapOrHoverHintProvider,
  tapOrHover,
} from '@/components/ui/tap-or-hover-hint';

// Material pass v2 (move 5): "tap or hover" appears once per page. The provider at the route root
// keeps a registry of mounted hints; the earliest registered renders, the later ones render
// nothing, and when a page unmounts its hints the next page's first hint is first again. The
// strings for titles (`tapOrHover()`) are untouched.

function Page({ hints }: { hints: string[] }) {
  return (
    <>
      {hints.map((what) => (
        <p key={what}>
          {what}: <TapOrHoverHint what={what} />
        </p>
      ))}
    </>
  );
}

const rendered = () =>
  [...document.querySelectorAll('[data-hint="tap-or-hover"]')].map((e) => e.textContent);

describe('TapOrHoverHint under its provider', () => {
  it('renders the first hint of a page and none of the later ones', () => {
    render(
      <TapOrHoverHintProvider>
        <Page hints={['a day', 'a bar', 'a band']} />
      </TapOrHoverHintProvider>
    );
    expect(rendered()).toEqual(['tap or hover a day']);
    expect(screen.queryByText('tap or hover a bar')).toBeNull();
    // the paragraph is still there: only the hint's words are withheld
    expect(screen.getByText(/^a bar:/)).toBeInTheDocument();
  });

  it('passes the words on to the next mounted hint when the first unmounts', () => {
    const { rerender } = render(
      <TapOrHoverHintProvider>
        <Page hints={['a day', 'a bar']} />
      </TapOrHoverHintProvider>
    );
    rerender(
      <TapOrHoverHintProvider>
        <Page hints={['a bar']} />
      </TapOrHoverHintProvider>
    );
    expect(rendered()).toEqual(['tap or hover a bar']);
  });

  it('starts over on the next page: its first hint renders, not the one mounted later', () => {
    const { rerender } = render(
      <TapOrHoverHintProvider>
        <Page key="home" hints={['a day', 'a bar']} />
      </TapOrHoverHintProvider>
    );
    rerender(
      <TapOrHoverHintProvider>
        <Page key="dao" hints={['a deposit', 'a point']} />
      </TapOrHoverHintProvider>
    );
    expect(rendered()).toEqual(['tap or hover a deposit']);
  });

  it('keeps the first hint first through a strict-mode remount of the effects', () => {
    render(
      <StrictMode>
        <TapOrHoverHintProvider>
          <Page hints={['a day', 'a bar']} />
        </TapOrHoverHintProvider>
      </StrictMode>
    );
    expect(rendered()).toEqual(['tap or hover a day']);
  });

  it('renders every hint when there is no provider (a piece rendered on its own)', () => {
    render(<Page hints={['a day', 'a bar']} />);
    expect(rendered()).toEqual(['tap or hover a day', 'tap or hover a bar']);
  });

  it('leaves the title strings alone', () => {
    expect(tapOrHover('a tx or a proposal', { capital: true })).toBe(
      'Tap or hover a tx or a proposal'
    );
  });
});
