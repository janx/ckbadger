// Table-rows round, lane R3 (T5): the boards' 11 px bordered chip for a row's role, symbol,
// standard, lock kind or class mark.
import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { RowTag, rowTagClass } from '@/components/ui/row-tag';

describe('RowTag (table-rows R3)', () => {
  it("is the boards' 11 px bordered chip by default: dim ink, base border, 3 px corners", () => {
    render(<RowTag>lock</RowTag>);
    const tag = screen.getByText('lock');
    expect(tag.tagName).toBe('SPAN');
    expect(tag).toHaveAttribute('data-row-tag', 'default');
    expect(tag).toHaveClass(
      'text-meta',
      'text-text-dim',
      'border',
      'border-base-border',
      'rounded-[3px]',
      'px-[5px]',
      'leading-4',
      'inline-block',
      'whitespace-nowrap',
      'align-middle'
    );
    expect(tag).not.toHaveClass('border-dashed');
  });

  it('ghost marks what is set back (deprecated): dim ink on a dashed border, legible as a warning', () => {
    render(<RowTag variant="ghost">deprecated</RowTag>);
    const tag = screen.getByText('deprecated');
    expect(tag).toHaveAttribute('data-row-tag', 'ghost');
    expect(tag).toHaveClass('text-meta', 'leading-4', 'border', 'border-base-border');
    // Lane F follow-up: the dashed border sets it apart; the ink is the readable dim (#7c849c,
    // ≥ 4.5:1 on the page), not the faint #4b5266 it was, which the user could not read.
    expect(tag).toHaveClass('border-dashed', 'text-text-dim');
    expect(tag.className).not.toMatch(/#4b5266/);
  });

  // Lane F follow-up 2 (user: the dim "deprecated" did not warn): red is reserved for negative
  // deltas; a deprecated script is the one status that earns it — a user must not pick it.
  it('warn marks the one status a user must not pick (deprecated): rouge ink on a solid rouge border', () => {
    render(<RowTag variant="warn">deprecated</RowTag>);
    const tag = screen.getByText('deprecated');
    expect(tag).toHaveAttribute('data-row-tag', 'warn');
    expect(tag).toHaveClass('text-meta', 'leading-4', 'border', 'text-rouge', 'border-rouge');
    expect(tag).not.toHaveClass('text-text-dim', 'border-base-border', 'border-dashed');
  });

  it('accent marks a class (BTC+CKB, live · current): jade ink on a jade border', () => {
    render(<RowTag variant="accent">BTC+CKB</RowTag>);
    const tag = screen.getByText('BTC+CKB');
    expect(tag).toHaveAttribute('data-row-tag', 'accent');
    expect(tag).toHaveClass('text-meta', 'leading-4', 'border', 'text-jade', 'border-[#1f5a46]');
    expect(tag).not.toHaveClass('text-text-dim', 'border-base-border', 'border-dashed');
  });

  it("takes a title and the caller's layout classes", () => {
    render(
      <RowTag title="xUDT-compatible" className="ml-2">
        wCKB
      </RowTag>
    );
    const tag = screen.getByText('wCKB');
    expect(tag).toHaveAttribute('title', 'xUDT-compatible');
    expect(tag).toHaveClass('ml-2', 'text-meta', 'text-text-dim');
  });

  it('gives the same class string to a caller that draws the chip on its own element', () => {
    for (const variant of ['default', 'ghost', 'accent', 'warn'] as const) {
      const { unmount } = render(<RowTag variant={variant}>x</RowTag>);
      expect(screen.getByText('x').className).toBe(rowTagClass(variant));
      unmount();
    }
    expect(rowTagClass()).toBe(rowTagClass('default'));
  });

  it('refuses an empty tag', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => render(<RowTag>{''}</RowTag>)).toThrow('RowTag: empty tag');
    expect(() => render(<RowTag>{null}</RowTag>)).toThrow('RowTag: empty tag');
    vi.restoreAllMocks();
  });
});
