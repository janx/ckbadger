import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { CopyButton } from '@/components/ui/copy-button';
import { COPIED_MS } from '@/hooks/useCopy';
import { inkOf, QUIET } from '@/__tests__/utils/ink-role';
import { boxPx, targetHeightPx } from '@/__tests__/utils/tailwind-box';
import { roundedRects } from '@/__tests__/utils/square-corners';

/**
 * The copy button of the controls board (gate D0′): one icon in a 24 px box (32 on a phone) after
 * an id line; it answers `copied` for 1.5 s, in words a screen reader hears; a write the clipboard
 * refuses says so in words, and it never claims a copy that did not happen.
 */
const HASH = `0x${'ab'.repeat(32)}`;
const setClipboard = (clipboard: unknown) => Object.assign(navigator, { clipboard });

/** A click and the clipboard's answer to it. */
async function click() {
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: 'copy transaction hash' }));
  });
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  vi.useRealTimers();
  setClipboard(undefined);
});

describe('CopyButton', () => {
  it('is one quiet icon in a button named for what it copies', () => {
    const { container } = render(<CopyButton value={HASH} label="copy transaction hash" />);
    const button = screen.getByRole('button', { name: 'copy transaction hash' });
    expect(inkOf(button)).toBe(QUIET);
    expect(button.querySelectorAll('svg')).toHaveLength(1);
    expect(button.querySelector('svg')).toHaveAttribute('aria-hidden', 'true');
    expect(roundedRects(container)).toEqual([]);
    expect(screen.getByRole('status')).toBeEmptyDOMElement();
  });

  it('answers “copied” for 1.5 s after a copy that worked, in the celadon at 12 px', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    setClipboard({ writeText });
    render(<CopyButton value={HASH} label="copy transaction hash" />);
    await click();
    expect(writeText).toHaveBeenCalledWith(HASH);
    const status = screen.getByRole('status');
    expect(status).toHaveTextContent('copied');
    expect(status).toHaveClass('text-meta', 'text-jade');
    act(() => vi.advanceTimersByTime(COPIED_MS - 1));
    expect(status).toHaveTextContent('copied');
    act(() => vi.advanceTimersByTime(1));
    expect(status).toBeEmptyDOMElement();
    expect(COPIED_MS).toBe(1500);
  });

  it('says “not copied” where the clipboard refuses the write, and never “copied”', async () => {
    setClipboard({ writeText: vi.fn().mockRejectedValue(new DOMException('denied')) });
    render(<CopyButton value={HASH} label="copy transaction hash" />);
    await click();
    const status = screen.getByRole('status');
    expect(status).toHaveTextContent('not copied');
    expect(status).toHaveClass('text-negative');
    expect(status.textContent).not.toBe('copied');
    act(() => vi.advanceTimersByTime(COPIED_MS * 2));
    expect(status).toHaveTextContent('not copied');
  });

  it('says “not copied” where there is no clipboard (a page over plain HTTP on a LAN address)', async () => {
    setClipboard(undefined);
    render(<CopyButton value={HASH} label="copy transaction hash" />);
    await click();
    expect(screen.getByRole('status')).toHaveTextContent('not copied');
  });

  it('answers a copy that works after one that was refused with “copied” alone', async () => {
    const writeText = vi
      .fn()
      .mockRejectedValueOnce(new DOMException('denied'))
      .mockResolvedValueOnce(undefined);
    setClipboard({ writeText });
    render(<CopyButton value={HASH} label="copy transaction hash" />);
    await click();
    expect(screen.getByRole('status')).toHaveTextContent('not copied');
    await click();
    expect(screen.getByRole('status').textContent).toBe('copied');
  });

  it('is a 24 px box from 640 and a 32 px one below it', () => {
    render(<CopyButton value={HASH} label="copy transaction hash" />);
    const button = screen.getByRole('button');
    expect([1280, 640, 390, 360].map((w) => targetHeightPx(button, w))).toEqual([24, 24, 32, 32]);
    expect([1280, 640, 390, 360].map((w) => boxPx(button, w))).toEqual([24, 24, 32, 32]);
  });
});
