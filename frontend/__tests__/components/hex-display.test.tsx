import { act, fireEvent, render, waitFor } from '@testing-library/react';
import { beforeEach, expect, vi } from 'vitest';
import { HexDisplay } from '@/components/ui/hex-display';

describe('HexDisplay', () => {
  beforeEach(() => {
    Object.assign(navigator, {
      clipboard: {
        writeText: vi.fn().mockResolvedValue(undefined),
      },
    });
  });

  it('renders the full hex value when truncation is disabled', () => {
    const { container } = render(
      <HexDisplay
        value="0xabcdef12"
        truncate={false}
        copyable={false}
        color="aqua"
        showGroupHighlight={false}
      />
    );

    expect(container).toHaveTextContent('0xabcdef12');
  });

  // U-09 (re-pinned): it shortens at its size, the one ellipsis between the halves
  it('shortens a long hex value at its size', () => {
    const { container } = render(
      <HexDisplay value="0xabcdef1234567890abcdef1234567890" size="row" copyable={false} />
    );

    expect(container).toHaveTextContent('0xabcdef12…567890');
  });

  it('copies the full hex value when clicked', async () => {
    const value = '0xabcdef1234567890abcdef1234567890';
    const { container, getByTitle } = render(<HexDisplay value={value} truncate={false} />);

    fireEvent.click(getByTitle(`Click to copy: ${value}`));

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(value);
    await waitFor(() => {
      expect(container).toHaveTextContent('Copied');
    });
  });

  // A4 review 3: the one copy path (hooks/useCopy): its tick's timer goes with it, and a copy
  // without a clipboard says nothing.
  it('leaves no timer when it goes while it says Copied, and says nothing without a clipboard', async () => {
    const value = '0xabcdef1234567890abcdef1234567890';
    vi.useFakeTimers();
    try {
      const first = render(<HexDisplay value={value} truncate={false} />);
      await act(async () => {
        fireEvent.click(first.getByTitle(`Click to copy: ${value}`));
      });
      expect(first.container).toHaveTextContent('Copied');
      first.unmount();
      expect(vi.getTimerCount()).toBe(0);
      Object.assign(navigator, { clipboard: undefined });
      const second = render(<HexDisplay value={value} truncate={false} />);
      await act(async () => {
        fireEvent.click(second.getByTitle(`Click to copy: ${value}`));
      });
      expect(second.container).not.toHaveTextContent('Copied');
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });
});
