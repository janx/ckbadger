import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { HexCode } from '@/components/ui/hex-code';

const HASH = '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef';
const FULL_VALUE = 'inline-block max-w-full overflow-x-auto whitespace-nowrap align-top';

describe('HexCode', () => {
  beforeEach(() => {
    Object.assign(navigator, { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
  });

  // Re-pinned with readability ruling R3: an id is a value, so the quiet variant is gone (it had
  // no caller); a quiet hash is a set-back row's (the `quiet` class), never the renderer's.
  it('sets its ink by variant: the reading ink by default, or bright', () => {
    const { rerender } = render(<HexCode value={HASH} truncate={false} />);
    expect(screen.getByText(HASH)).toHaveClass('font-mono', 'text-text');
    rerender(<HexCode value={HASH} truncate={false} variant="bright" />);
    expect(screen.getByText(HASH)).toHaveClass('text-text-bright');
  });

  // Seals by kind, C10 round 2: a name plate's id line sets the voice once (text-small in
  // text-text-dim) and the hash in it takes it, so `inherit` writes no ink and no size of its own.
  // Re-pinned (lane c2-finish, rulings round 1): an id copies nothing on a click (its click-to-copy,
  // its tick and its glow went with their last call site; a copy is the copy button's, after an id
  // line), so its whole value is its title; the two tests of that copy alone (the click and its
  // tick, the tick's timer) went with it.
  it('takes the ink and the size of the line it stands in under `inherit`, copying nothing', () => {
    render(<HexCode value={HASH} truncate={false} variant="inherit" />);
    const hash = screen.getByText(HASH);
    // U-16 (re-pinned): a full value is one line in its own box, as wide as its column at most
    expect(hash.className).toBe(`font-mono ${FULL_VALUE}`);
    expect(hash).toHaveAttribute('title', HASH);
    fireEvent.click(hash);
    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
    expect(screen.queryByText('✓')).toBeNull();
  });

  it('keeps the full value as its title under `inherit`', () => {
    render(<HexCode value={HASH} truncate={false} variant="inherit" className="min-w-0" />);
    const hash = screen.getByText(HASH);
    expect(hash.className).toBe(`font-mono ${FULL_VALUE} min-w-0`);
    expect(hash).toHaveAttribute('title', HASH);
  });
});
