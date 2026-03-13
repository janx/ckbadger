import { fireEvent, render, waitFor } from '@testing-library/react';
import { beforeEach, expect, vi } from 'vitest';
import { ByteGroupDisplay, HexDisplay } from '@/components/ui/hex-display';

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

  it('groups bytes with separators in byte group mode', () => {
    const { container } = render(
      <ByteGroupDisplay value="0xabcdef12" bytesPerGroup={1} color="aqua" />
    );

    expect(container).toHaveTextContent('0xab cd ef 12');
  });

  it('truncates long hex values by default', () => {
    const { container } = render(
      <HexDisplay value="0xabcdef1234567890abcdef1234567890" truncate copyable={false} />
    );

    expect(container).toHaveTextContent('0xabcdef1234...34567890');
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
});
