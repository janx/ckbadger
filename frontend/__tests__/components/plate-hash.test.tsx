import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { PlateHash } from '@/components/ui/page-seal';
import { stubChartWidth } from '@/__tests__/utils/chart-width';

/**
 * The lead's ruling a (UI review 2026-11-14): the name plate's id line is never a scroller. The
 * head is the first screen and the plate is the page's name, so a full hash sits on ONE line where
 * the measured line holds it, else in TWO HALVES, a designed break: `0x` and the first 32 hex
 * digits, then the last 32 indented 2ch so the digits stand under the digits; each half one unit.
 * The plate's id line is text-small, 12 px: a JetBrains Mono advance of 7.2 px, so the one line
 * takes 66 × 7.2 = 475.2 px and a half 34 × 7.2 = 244.8 px.
 */
const HASH = `0x${'0123456789abcdef'.repeat(4)}`;

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const plate = () => document.querySelector('[data-plate-hash]') as HTMLElement;
const halves = () => [...plate().querySelectorAll<HTMLElement>('[data-plate-half]')];

describe('PlateHash, the full hash on a name plate', () => {
  it('sets the hash in two halves where the line cannot hold it (358 px, a 390 phone)', () => {
    stubChartWidth(358);
    render(<PlateHash hash={HASH} />);
    const [first, second] = halves();
    expect(halves()).toHaveLength(2);
    expect(first.textContent).toBe(HASH.slice(0, 34));
    expect(second.textContent).toBe(HASH.slice(34));
    // each half one unit; the second stands 2ch in, its digits under the first's
    for (const half of halves()) expect(half).toHaveClass('whitespace-nowrap');
    expect(second).toHaveClass('pl-[2ch]');
    expect(first.nextElementSibling?.tagName).toBe('BR');
    // the whole id once, in the text and the title; never a scroller
    expect(plate().textContent).toBe(HASH);
    expect(plate()).toHaveAttribute('title', HASH);
    expect(plate().querySelector('.overflow-x-auto')).toBeNull();
  });

  it('keeps it on one line where the line holds it (from 475.2 px), split just below', () => {
    stubChartWidth(476);
    const { unmount } = render(<PlateHash hash={HASH} />);
    expect(halves()).toHaveLength(1);
    expect(halves()[0].textContent).toBe(HASH);
    expect(plate().querySelector('br')).toBeNull();
    unmount();
    vi.restoreAllMocks();
    stubChartWidth(475);
    render(<PlateHash hash={HASH} />);
    expect(halves()).toHaveLength(2);
  });

  it('copies the whole id when it copies, the plate keeping its two lines', async () => {
    stubChartWidth(326);
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<PlateHash hash={HASH} copyable />);
    fireEvent.click(plate());
    expect(writeText).toHaveBeenCalledWith(HASH);
    expect(screen.getByTitle(HASH)).toBe(plate());
    // the ✓ stands over the hash, which keeps its lines (hidden), so the page under it never moves
    await waitFor(() => expect(screen.getByText('✓')).toBeInTheDocument());
    expect(halves()).toHaveLength(2);
    expect(halves()[0].parentElement).toHaveClass('invisible');
  });

  it('refuses a value that is not a full hash', () => {
    expect(() => render(<PlateHash hash="0x1234" />)).toThrow('not a full hash');
  });
});
