import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { Hash } from '@/components/ui/hash';

describe('Hash', () => {
  const fullHash = '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef';

  beforeEach(() => {
    Object.assign(navigator, {
      clipboard: {
        writeText: vi.fn().mockResolvedValue(undefined),
      },
    });
  });

  it('renders full hash when truncate is false', () => {
    render(<Hash hash={fullHash} truncate={false} />);
    expect(screen.getByText(fullHash)).toBeInTheDocument();
  });

  // U-09 (re-pinned): a size, never a length
  it('writes the hash at its size', () => {
    render(<Hash hash={fullHash} size="row" />);
    expect(screen.getByText('0x12345678…abcdef')).toBeInTheDocument();
    expect(screen.queryByText(fullHash)).not.toBeInTheDocument();
  });

  // Re-pinned (lane c2-finish, rulings round 1): an id copies nothing on a click; its
  // click-to-copy (the default of `copyable`) went with its last call site, and its test with it.
  // A copy is the copy button's, after an id line.
  it('uses the full hash as title and copies nothing on a click', () => {
    render(<Hash hash={fullHash} size="row" />);

    const element = screen.getByText(/0x12345678/);
    fireEvent.click(element);

    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
    expect(element).toHaveAttribute('title', fullHash);
  });
});
