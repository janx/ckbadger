import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { HexCode } from '@/components/ui/hex-code';

const HASH = '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef';

describe('HexCode', () => {
  beforeEach(() => {
    Object.assign(navigator, { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
  });

  it('sets its ink by variant: the text ink by default, bright, dim', () => {
    const { rerender } = render(<HexCode value={HASH} truncate={false} />);
    expect(screen.getByText(HASH)).toHaveClass('font-mono', 'text-text');
    rerender(<HexCode value={HASH} truncate={false} variant="bright" />);
    expect(screen.getByText(HASH)).toHaveClass('text-text-bright');
    rerender(<HexCode value={HASH} truncate={false} variant="dim" />);
    expect(screen.getByText(HASH)).toHaveClass('text-text-dim');
  });

  // Seals by kind, C10 round 2: a name plate's id line sets the voice once (text-small in
  // text-text-dim) and the hash in it takes it, so `inherit` writes no ink and no size of its own;
  // its click-to-copy is unchanged.
  it('takes the ink and the size of the line it stands in under `inherit`, still copying', async () => {
    render(<HexCode value={HASH} truncate={false} variant="inherit" />);
    const hash = screen.getByText(HASH);
    expect(hash.className).toBe('font-mono hover-lit cursor-pointer transition-colors');
    expect(hash).toHaveAttribute('title', 'Click to copy');
    fireEvent.click(hash);
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(HASH);
    await waitFor(() => expect(screen.getByText('✓')).toBeInTheDocument());
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
    expect(hash.className).toBe('font-mono min-w-0');
    expect(hash).toHaveAttribute('title', HASH);
  });
});
