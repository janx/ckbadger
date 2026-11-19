import { fireEvent, render } from '@testing-library/react';
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
      <HexDisplay value="0xabcdef12" truncate={false} color="aqua" showGroupHighlight={false} />
    );

    expect(container).toHaveTextContent('0xabcdef12');
  });

  // U-09 (re-pinned): it shortens at its size, the one ellipsis between the halves
  it('shortens a long hex value at its size', () => {
    const { container } = render(
      <HexDisplay value="0xabcdef1234567890abcdef1234567890" size="row" />
    );

    expect(container).toHaveTextContent('0xabcdef12…567890');
  });

  // Re-pinned (lane c2-finish, rulings round 1): an id copies nothing on a click. Its
  // click-to-copy reached one call site, the .cell party's lock hash inside its link, where the
  // link went to the address and the copy was never seen; it went with that site, and its two
  // tests (the click and its "Copied", the timer) with it. Its whole value is its title.
  it('copies nothing on a click, its whole value its title', () => {
    const value = '0xabcdef1234567890abcdef1234567890';
    const { container, getByTitle } = render(<HexDisplay value={value} truncate={false} />);
    fireEvent.click(getByTitle(value));
    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
    expect(container).not.toHaveTextContent('Copied');
  });
});
