import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import {
  TerminalPanel,
  TerminalPanelContent,
  TerminalPanelHeader,
} from '@/components/ui/terminal-panel';

// Midnight, continued (review 2026-11-03, item 5, move 3): "instruments are framed, text is
// flat". TerminalPanel is the one instrument frame — a #222840 hairline (`border-base-border`),
// an 8 px radius (visual finish V10: glass frames take `rounded-lg`), glass (material pass v2, move 2:
// `.glass` replaces the flat surface colour, so the sky's pool shows through), the jade top edge
// (`.neon-edge-top`) and inset scanlines — with an uppercase 12 px eyebrow led by the section mark.
describe('TerminalPanel, the one instrument frame (move 3)', () => {
  it('frames its content: hairline, 8 px radius, glass, jade top edge, scanlines', () => {
    const { container } = render(
      <TerminalPanel aria-label="An instrument" data-testid="frame">
        <TerminalPanelContent>inside</TerminalPanelContent>
      </TerminalPanel>
    );
    const frame = screen.getByTestId('frame');
    expect(frame).toHaveAttribute('data-instrument-frame');
    expect(frame).toHaveAttribute('aria-label', 'An instrument');
    expect(frame).toHaveClass(
      'neon-edge-top',
      'relative',
      'overflow-hidden',
      'rounded-lg',
      'border',
      'border-base-border',
      'glass'
    );
    expect(frame).not.toHaveClass('bg-base-surface');
    // only the glass frame takes the glass radius; the flat variants keep the default 4 px
    for (const variant of ['elevated', 'inset'] as const) {
      const { getByTestId, unmount } = render(
        <TerminalPanel data-testid={`flat-${variant}`} variant={variant}>
          flat
        </TerminalPanel>
      );
      expect(getByTestId(`flat-${variant}`)).toHaveClass('rounded');
      expect(getByTestId(`flat-${variant}`)).not.toHaveClass('rounded-lg');
      expect(getByTestId(`flat-${variant}`)).not.toHaveClass('glass');
      unmount();
    }
    // the jade edge's colour, at the approved .45, as a class: the frame carries no inline ink
    expect(frame).toHaveClass('[--neon-color:rgba(46,219,163,0.45)]');
    expect(frame).not.toHaveAttribute('style');
    // the inset scanlines, 1 px on, 1 px off, under the content
    const scan = container.querySelector('.pointer-events-none > div') as HTMLElement;
    expect(scan.style.background).toContain('repeating-linear-gradient');
    expect(scan.style.background).toContain('rgba(0,0,0,0.03)');
    expect(screen.getByText('inside').parentElement).toHaveClass('relative', 'z-10');
  });

  it('colours the edge by accent, still without an inline style', () => {
    render(<TerminalPanel accentColor="gold" data-testid="frame" children="x" />);
    const frame = screen.getByTestId('frame');
    expect(frame).toHaveClass('[--neon-color:rgba(242,197,92,0.4)]');
    expect(frame).not.toHaveClass('[--neon-color:rgba(46,219,163,0.45)]');
    expect(frame).not.toHaveAttribute('style');
  });

  it('heads the frame with the section mark and an uppercase 12 px eyebrow', () => {
    render(
      <TerminalPanel>
        <TerminalPanelHeader actions={<button type="button">Act</button>}>
          Cells
        </TerminalPanelHeader>
      </TerminalPanel>
    );
    const mark = screen.getByTestId('terminal-panel-mark');
    expect(mark).toHaveAttribute('aria-hidden', 'true');
    expect(mark).toHaveClass('section-mark');
    expect(mark).toHaveAttribute('data-indicator', 'none');
    expect(mark).not.toHaveClass('section-mark-live', 'section-mark-gold', 'opacity-30');
    const eyebrow = screen.getByText('Cells');
    // material pass v2 (move 3): the eyebrow's case, tracking (.12 em) and weight (500) are one
    // rule, `.eyebrow`, shared with the tier boundary and the hero labels
    expect(eyebrow).toHaveClass('text-small', 'eyebrow', 'text-text-dim');
    expect(eyebrow).not.toHaveClass('text-body', 'tracking-wider', 'tracking-[0.08em]');
    // the mark precedes the words, in one row with the actions at the right
    expect(mark.compareDocumentPosition(eyebrow)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    const row = mark.parentElement!.parentElement!;
    expect(row).toHaveClass('border-b', 'border-base-border', 'justify-between');
    expect(row).toContainElement(screen.getByRole('button', { name: 'Act' }));
  });

  it('turns the mark into the indicator: live breathes, warning is gold, inactive is dim', () => {
    const { rerender } = render(
      <TerminalPanelHeader indicator="active">Transaction pipeline</TerminalPanelHeader>
    );
    const mark = () => screen.getByTestId('terminal-panel-mark');
    expect(mark()).toHaveClass('section-mark', 'section-mark-live');
    expect(mark()).toHaveAttribute('data-indicator', 'active');
    // one dot: the old live-dot is not drawn beside the mark
    expect(document.querySelector('.live-dot')).toBeNull();
    rerender(<TerminalPanelHeader indicator="warning">Pipeline</TerminalPanelHeader>);
    expect(mark()).toHaveClass('section-mark', 'section-mark-gold');
    rerender(<TerminalPanelHeader indicator="inactive">Pipeline</TerminalPanelHeader>);
    expect(mark()).toHaveClass('section-mark', 'opacity-30');
    expect(mark()).not.toHaveClass('section-mark-live');
  });
});
