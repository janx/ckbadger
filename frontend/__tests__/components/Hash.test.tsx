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

  it('truncates hash by default', () => {
    render(<Hash hash={fullHash} />);
    const element = screen.getByText(/0x12345678.*90abcdef/);
    expect(element).toBeInTheDocument();
    expect(screen.queryByText(fullHash)).not.toBeInTheDocument();
  });

  it('uses custom startChars and endChars', () => {
    render(<Hash hash={fullHash} startChars={8} endChars={6} />);
    expect(screen.getByText('0x123456...abcdef')).toBeInTheDocument();
  });

  it('does not truncate short hashes', () => {
    const shortHash = '0x1234';
    render(<Hash hash={shortHash} />);
    expect(screen.getByText(shortHash)).toBeInTheDocument();
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

  it('does not copy when copyable is false', () => {
    render(<Hash hash={fullHash} copyable={false} />);

    const element = screen.getByText(/0x12345678/);
    fireEvent.click(element);

    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
  });

  it('applies custom className', () => {
    render(<Hash hash={fullHash} className="custom-class" />);
    const element = screen.getByText(/0x12345678/);
    expect(element).toHaveClass('custom-class');
  });

  it('has correct title attribute for copyable hash', () => {
    render(<Hash hash={fullHash} copyable />);
    const element = screen.getByText(/0x12345678/);
    expect(element).toHaveAttribute('title', 'Click to copy');
  });

  it('has hash as title when not copyable', () => {
    render(<Hash hash={fullHash} copyable={false} />);
    const element = screen.getByText(/0x12345678/);
    expect(element).toHaveAttribute('title', fullHash);
  });
});
