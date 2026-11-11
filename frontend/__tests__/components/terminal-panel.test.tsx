import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import {
  TerminalPanel,
  TerminalPanelContent,
  TerminalPanelHeader,
} from '@/components/ui/terminal-panel';

// Midnight, continued (review 2026-11-03, item 5, move 3): "instruments are framed, text is
// flat". TerminalPanel is the one instrument frame — a 1 px line (`border-base-border`), an 8 px
// radius (visual finish V10: glass frames take `rounded-lg`), `.glass` (material pass v2, move 2)
// and inset scanlines — headed by a command line led by the prompt square. Rubbing × Song print,
// phase 2: a flat tile, the glass's materials the opaque surface, and no jade top edge.
describe('TerminalPanel, the one instrument frame (move 3)', () => {
  it('frames its content: hairline, 8 px radius, the flat glass tile, scanlines, no lit edge', () => {
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

  // Rubbing × Song print, phase 2: the frame's head is a command line — the prompt square (still
  // the frame's indicator), then the instrument's name as the command, lowercase, in the moon ink
  // at 13 px / 500 — where it was the section mark and a dim upper-case 12 px eyebrow.
  it('heads the frame with the prompt square and the instrument’s name as the command', () => {
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
    const command = screen.getByText('Cells');
    expect(command).toHaveClass('lowercase');
    const head = command.parentElement!;
    expect(head).toHaveClass('prompt', 'text-text-bright', 'text-body', 'font-medium');
    expect(head).not.toHaveClass('eyebrow', 'text-text-dim', 'text-small');
    // not a heading of the page outline: the frame's head is a div
    expect(head.tagName).toBe('DIV');
    // the mark precedes the words, in one row with the actions at the right
    expect(mark.compareDocumentPosition(command)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(mark.parentElement).toBe(head);
    const row = head.parentElement!;
    expect(row).toHaveClass('border-b', 'border-base-border', 'justify-between');
    expect(row).toContainElement(screen.getByRole('button', { name: 'Act' }));
  });

  // Rubbing × Song print, phase 5 (H1): the head runs its command on args — `pipeline --tip #N`
  // — dim and regular after the name, inside the command line, never among the actions.
  it('writes its args dim after the command, inside the command line, not with the actions', () => {
    const { rerender } = render(
      <TerminalPanelHeader
        args={<span data-testid="args">--tip #20,969,560</span>}
        actions={<a href="/blocks">all blocks →</a>}
      >
        Pipeline
      </TerminalPanelHeader>
    );
    const command = screen.getByText('Pipeline');
    const head = command.parentElement!;
    const args = screen.getByTestId('args').parentElement!;
    expect(args.parentElement).toBe(head);
    expect(args).toHaveClass('text-text-dim', 'font-normal', 'normal-case');
    expect(args).not.toHaveClass('lowercase');
    expect(command.compareDocumentPosition(args)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(head).toHaveTextContent(/^Pipeline--tip #20,969,560$/);
    const link = screen.getByRole('link', { name: 'all blocks →' });
    expect(head).not.toContainElement(link);
    expect(head.parentElement).toContainElement(link);
    // no args: the name alone, as before
    rerender(<TerminalPanelHeader>Pipeline</TerminalPanelHeader>);
    expect(screen.getByText('Pipeline').parentElement!.children).toHaveLength(2);
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
