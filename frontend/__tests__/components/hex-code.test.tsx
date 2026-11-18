import { describe, it, expect, vi, beforeEach } from 'vitest';
import { act, screen, fireEvent, waitFor } from '@testing-library/react';
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
  // text-text-dim) and the hash in it takes it, so `inherit` writes no ink and no size of its own;
  // its click-to-copy is unchanged.
  it('takes the ink and the size of the line it stands in under `inherit`, still copying', async () => {
    render(<HexCode value={HASH} truncate={false} variant="inherit" />);
    const hash = screen.getByText(HASH);
    // U-16 (re-pinned): a full value is one line in its own box, as wide as its column at most
    expect(hash.className).toBe(
      `font-mono hover-lit cursor-pointer transition-colors ${FULL_VALUE}`
    );
    expect(hash).toHaveAttribute('title', 'Click to copy');
    fireEvent.click(hash);
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(HASH);
    await waitFor(() => expect(screen.getByText('✓')).toBeInTheDocument());
  });

  // A4 review 3: HexCode copies through the one copy path (hooks/useCopy): its tick's timer goes
  // with it, and a copy without a clipboard shows no tick.
  it('leaves no timer when it goes while its tick shows, and shows none without a clipboard', async () => {
    vi.useFakeTimers();
    try {
      const { unmount } = render(<HexCode value={HASH} truncate={false} />);
      await act(async () => {
        fireEvent.click(screen.getByText(HASH));
      });
      expect(screen.getByText('✓')).toBeInTheDocument();
      unmount();
      expect(vi.getTimerCount()).toBe(0);
      Object.assign(navigator, { clipboard: undefined });
      render(<HexCode value={HASH} truncate={false} />);
      await act(async () => {
        fireEvent.click(screen.getByText(HASH));
      });
      expect(screen.queryByText('✓')).toBeNull();
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it('keeps the full value as its title under `inherit` when it does not copy', () => {
    render(
      <HexCode
        value={HASH}
        truncate={false}
        variant="inherit"
        copyable={false}
        className="min-w-0"
      />
    );
    const hash = screen.getByText(HASH);
    expect(hash.className).toBe(`font-mono ${FULL_VALUE} min-w-0`);
    expect(hash).toHaveAttribute('title', HASH);
  });
});
