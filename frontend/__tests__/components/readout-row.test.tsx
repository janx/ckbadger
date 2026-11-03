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

  // Lane U follow-up: hover never moves layout. A row with no idle words and no hint used to be an
  // empty line box (9 px) that grew to a text line (27 px) on hover, pushing the page below it.
  it('keeps its one line while idle with nothing to say, so a hover does not move the page', () => {
    const { rerender } = render(
      <ReadoutRow label="r" pinned={false} hint={false}>
        {null}
      </ReadoutRow>
    );
    const row = screen.getByRole('status', { name: 'r' });
    const reading = row.firstElementChild as HTMLElement;
    // a no-break space holds the line (the row's 12 px / 18 px text line), never an empty box
    expect(reading.textContent).toBe('\u00a0');
    expect(reading).toHaveClass('text-text');
    rerender(
      <ReadoutRow label="r" pinned={false} hint={false}>
        10/21 · 10.09
      </ReadoutRow>
    );
    expect(row.firstElementChild).toBe(reading);
    expect(reading).toHaveTextContent('10/21 · 10.09');
    // an empty idle string holds the line the same way
    rerender(
      <ReadoutRow label="r" pinned={false} idle="" hint={false}>
        {null}
      </ReadoutRow>
    );
    expect(reading.textContent).toBe('\u00a0');
  });

  // Lane U follow-up: at 390 px an idle sentence ("A day on the chart reads its value here.") wraps
  // to two lines and a one-line reading shrank the row under the reader's pointer. The idle words
  // stay reserved under the reading as an invisible ghost (CSS generated content from data-idle), so
  // the row is never shorter than its idle line; the reading's text and name stay its own.
  it('reserves the idle sentence under a reading, without adding it to the reading', () => {
    const idle = 'A day on the chart reads its value here.';
    const { rerender } = render(
      <ReadoutRow label="r" pinned={false} idle={idle}>
        {null}
      </ReadoutRow>
    );
    const row = screen.getByRole('status', { name: 'r' });
    const reading = row.firstElementChild as HTMLElement;
    expect(reading).toHaveAttribute('data-idle', idle);
    expect(reading).toHaveClass(
      'grid',
      'after:invisible',
      'after:content-[attr(data-idle)]',
      'after:[grid-area:1/1]'
    );
    expect(reading.firstElementChild).toHaveClass('[grid-area:1/1]');
    rerender(
      <ReadoutRow label="r" pinned={false} idle={idle}>
        2023-12-16 · 165.06 PH/s
      </ReadoutRow>
    );
    expect(row.firstElementChild).toBe(reading);
    expect(reading).toHaveAttribute('data-idle', idle);
    expect(reading).toHaveTextContent(/^2023-12-16 · 165\.06 PH\/s$/);
    expect(row).not.toHaveTextContent('A day on the chart');
    // a markup idle (a reading itself, e.g. the DAO cycle line) has no ghost to reserve
    rerender(
      <ReadoutRow label="r" pinned={false} idle={<b>843 CKB</b>}>
        {null}
      </ReadoutRow>
    );
    expect(reading).not.toHaveAttribute('data-idle');
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
