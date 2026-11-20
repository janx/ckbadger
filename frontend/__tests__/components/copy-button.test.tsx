import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { CopyButton } from '@/components/ui/copy-button';
import { ANNOUNCE_GAP_MS, COPIED_MS } from '@/hooks/useCopy';
import { inkOf, QUIET } from '@/__tests__/utils/ink-role';
import { boxPx, contentPx, inFlowAt, targetHeightPx } from '@/__tests__/utils/tailwind-box';
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

  // Re-pinned (lane c2-finish, rulings round 1): `not copied` stands for the same 1.5 s as
  // `copied` (it stood until the next try: drawn over the line under its box, it would cover it)
  it('says “not copied” where the clipboard refuses the write, and never “copied”', async () => {
    setClipboard({ writeText: vi.fn().mockRejectedValue(new DOMException('denied')) });
    render(<CopyButton value={HASH} label="copy transaction hash" />);
    await click();
    const status = screen.getByRole('status');
    expect(status).toHaveTextContent('not copied');
    expect(status).toHaveClass('text-negative');
    expect(status.textContent).not.toBe('copied');
    act(() => vi.advanceTimersByTime(COPIED_MS - 1));
    expect(status).toHaveTextContent('not copied');
    act(() => vi.advanceTimersByTime(1));
    expect(status).toBeEmptyDOMElement();
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

  // Lane c2-finish, item 4: the crash page's copy is an action with a purpose, not an id's, so its
  // words stay on screen beside the icon (they were only its accessible name). The smallest form:
  // the label written after the icon inside the button, one target of the same height, quiet.
  it('writes its words after the icon where the copy is an action with a purpose', async () => {
    setClipboard({ writeText: vi.fn().mockResolvedValue(undefined) });
    render(<CopyButton value={HASH} label="copy details for a bug report" words />);
    const button = screen.getByRole('button', { name: 'copy details for a bug report' });
    expect(button).toHaveTextContent(/^copy details for a bug report$/);
    expect(button.querySelectorAll('svg')).toHaveLength(1);
    expect(inkOf(screen.getByText('copy details for a bug report'))).toBe(QUIET);
    expect([1280, 640, 390, 360].map((w) => targetHeightPx(button, w))).toEqual([24, 24, 32, 32]);
    // as wide as the icon and its words, not the icon's square box
    expect(button.className).not.toMatch(/(^|\s)(sm:)?w-[68](\s|$)/);
    await act(async () => {
      fireEvent.click(button);
    });
    expect(screen.getByRole('status').textContent).toBe('copied');
    // its answer as every copy's: out of the line's flow, under its box
    expect(inFlowAt(screen.getByRole('status'), 390)).toBe(false);
    expect(screen.getByRole('status')).toHaveClass('absolute', 'top-full', 'right-0');
  });
});

/**
 * Lane c2-finish, rulings round 1: a copy's answer never moves a line. A line that jumps under
 * the reader's finger is worse than the answer it carries, so the words stand out of the line's
 * flow, a small label anchored under the copy box, its right edge on the box's right edge (it never
 * runs past the line's end and never covers the id before the box), on the elevated ground in the
 * frame line so the words under it do not show through; it takes no width and no height.
 */
// Lane cd-fixes-controls (L1 NOTE 13): a second copy within 1.5 s is heard: the live region's
// words go and come back (they stood "copied", unchanged, and a screen reader said nothing).
describe('CopyButton: a second copy is announced again', () => {
  it('empties its answer and writes it again', async () => {
    setClipboard({ writeText: vi.fn().mockResolvedValue(undefined) });
    render(<CopyButton value={HASH} label="copy transaction hash" />);
    const status = screen.getByRole('status');
    await click();
    expect(status).toHaveTextContent(/^copied$/);
    act(() => vi.advanceTimersByTime(500));
    await click();
    expect(status).toBeEmptyDOMElement();
    act(() => vi.advanceTimersByTime(ANNOUNCE_GAP_MS));
    expect(status).toHaveTextContent(/^copied$/);
  });
});

describe('CopyButton: its answer moves no line', () => {
  const ANSWER_GROUND = ['bg-base-elevated', 'border', 'border-base-border'];

  it('takes no width and no height: the copy is as wide as its box, the answer out of flow', async () => {
    setClipboard({ writeText: vi.fn().mockResolvedValue(undefined) });
    render(<CopyButton value={HASH} label="copy transaction hash" />);
    const button = screen.getByRole('button', { name: 'copy transaction hash' });
    const root = button.parentElement!;
    const status = screen.getByRole('status');
    const widths = () => [1280, 640, 390, 360].map((w) => contentPx(root, w));
    expect(widths()).toEqual([24, 24, 32, 32]);
    expect(root).toHaveClass('relative');
    // while it has nothing to say it draws nothing: no ground, no frame
    for (const cls of ANSWER_GROUND) expect(status).not.toHaveClass(cls);
    await click();
    expect(status).toHaveTextContent(/^copied$/);
    for (const w of [1280, 640, 390, 360]) expect(inFlowAt(status, w)).toBe(false);
    expect(widths()).toEqual([24, 24, 32, 32]);
    expect(status).toHaveClass(
      'absolute',
      'top-full',
      'right-0',
      'whitespace-nowrap',
      ...ANSWER_GROUND
    );
    expect(status).not.toHaveClass('left-0');
  });

  it('anchors its left edge on the box’s where its right edge would run past the page’s start', async () => {
    setClipboard({ writeText: vi.fn().mockResolvedValue(undefined) });
    render(<CopyButton value={HASH} label="copy transaction hash" />);
    const status = screen.getByRole('status');
    // a box at the start of a 360 px phone's line: its right edge at 16 + 32 = 48 px, the label
    // 57.2 px wide would start at -9.2 px
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
      this: HTMLElement
    ) {
      const left = this === status ? -9.2 : 16;
      return {
        left,
        right: left + 57.2,
        width: 57.2,
        top: 0,
        bottom: 0,
        height: 0,
        x: left,
        y: 0,
        toJSON: () => ({}),
      } as DOMRect;
    });
    await click();
    expect(status).toHaveClass('left-0');
    expect(status).not.toHaveClass('right-0');
    act(() => vi.advanceTimersByTime(COPIED_MS));
    expect(status).toBeEmptyDOMElement();
    expect(status).toHaveClass('right-0');
  });
});
