import { render } from '@testing-library/react';
import { ByteGroupDisplay, HexDisplay } from '@/components/ui/hex-display';

describe('HexDisplay accent color', () => {
  it('renders accent classes for prefix and byte chars', () => {
    const { container } = render(
      <HexDisplay value="0xabcdef12" truncate={false} copyable={false} color="accent" />
    );

    expect(container.querySelector('.text-terminal-dark')).toBeTruthy();
    expect(container.querySelector('.text-terminal-green')).toBeTruthy();
  });

  it('renders accent classes in byte group mode', () => {
    const { container } = render(
      <ByteGroupDisplay value="0xabcdef12" bytesPerGroup={1} color="accent" />
    );

    expect(container.querySelector('.text-terminal-green')).toBeTruthy();
    expect(container.querySelector('.text-terminal-dark')).toBeTruthy();
  });
});
