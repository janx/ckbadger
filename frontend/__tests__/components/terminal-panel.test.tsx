import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { TerminalPanel, TerminalPanelContent } from '@/components/ui/terminal-panel';

// Midnight, continued (review 2026-11-03, item 5, move 3): "instruments are framed, text is
// flat". TerminalPanel is the one instrument frame — a 1 px line (`border-base-border`), square
// corners (UI review 2026-11-14, ruling R1), `.glass` (material pass v2, move 2) and inset
// scanlines. Rubbing × Song print, phase 2: a flat tile, the glass's materials the opaque surface,
// and no jade top edge. Its command-line head went with the pipeline, its one user, when the
// pipeline became an unframed section (direction F, lane R).
describe('TerminalPanel, the one instrument frame (move 3)', () => {
  it('frames its content: hairline, the flat glass tile, scanlines, no lit edge', () => {
    const { container } = render(
      <TerminalPanel aria-label="An instrument" data-testid="frame">
        <TerminalPanelContent>inside</TerminalPanelContent>
      </TerminalPanel>
    );
    const frame = screen.getByTestId('frame');
    expect(frame).toHaveAttribute('data-instrument-frame');
    expect(frame).toHaveAttribute('aria-label', 'An instrument');
    expect(frame).toHaveClass(
      'relative',
      'overflow-hidden',
      'border',
      'border-base-border',
      'glass'
    );
    expect(frame).not.toHaveClass('bg-base-surface');
    // only the glass frame is glass
    for (const variant of ['elevated', 'inset'] as const) {
      const { getByTestId, unmount } = render(
        <TerminalPanel data-testid={`flat-${variant}`} variant={variant}>
          flat
        </TerminalPanel>
      );
      expect(getByTestId(`flat-${variant}`)).not.toHaveClass('glass');
      unmount();
    }
    // no lit top edge, no edge colour; the frame carries no inline ink
    expect(frame).not.toHaveClass('neon-edge-top');
    expect(frame.className).not.toContain('--neon-color');
    expect(frame).not.toHaveAttribute('style');
    // the inset scanlines, 1 px on, 1 px off, under the content, in the theme's inset ink
    const scan = container.querySelector('.pointer-events-none > div') as HTMLElement;
    expect(scan.style.background).toContain('repeating-linear-gradient');
    expect(scan.style.background).toContain('var(--m-inset-scan)');
    expect(screen.getByText('inside').parentElement).toHaveClass('relative', 'z-10');
  });

  // The accent colours went with the edge they coloured: every frame is the same tile.
  it('draws every variant without an edge or an inline style', () => {
    for (const variant of ['default', 'elevated', 'inset'] as const) {
      const { getByTestId, unmount } = render(
        <TerminalPanel variant={variant} data-testid="frame" children="x" />
      );
      const frame = getByTestId('frame');
      expect(frame).not.toHaveClass('neon-edge-top');
      expect(frame.className).not.toContain('--neon-color');
      expect(frame).not.toHaveAttribute('style');
      unmount();
    }
  });
});
