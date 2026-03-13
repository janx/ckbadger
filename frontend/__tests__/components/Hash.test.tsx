import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
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

  it('uses truncation settings for the displayed hash', () => {
    render(<Hash hash={fullHash} startChars={8} endChars={6} />);
    expect(screen.getByText('0x123456...abcdef')).toBeInTheDocument();
    expect(screen.queryByText(fullHash)).not.toBeInTheDocument();
  });

  it('copies hash to clipboard on click when copyable', async () => {
    render(<Hash hash={fullHash} copyable />);

    const element = screen.getByText(/0x12345678/);
    fireEvent.click(element);

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(fullHash);

    await waitFor(() => {
      expect(screen.getByText('✓')).toBeInTheDocument();
    });
  });

  it('uses the full hash as title when copying is disabled', () => {
    render(<Hash hash={fullHash} copyable={false} />);

    const element = screen.getByText(/0x12345678/);
    fireEvent.click(element);

    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
    expect(element).toHaveAttribute('title', fullHash);
  });
});
