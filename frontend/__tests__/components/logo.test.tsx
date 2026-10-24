import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen } from '../utils/test-utils';
import { Logo } from '@/components/layout/logo';
import { useRealtimeStore } from '@/hooks/useRealtimeStore';

type Block = NonNullable<ReturnType<typeof useRealtimeStore.getState>['latestBlock']>;

function block(number: number): Block {
  return { number } as Block;
}

describe('Logo', () => {
  afterEach(() => {
    vi.useRealTimers();
    act(() => useRealtimeStore.getState().reset());
  });

  it('is the tilted badge, sized from 96 px on a phone to 157 px on a desktop', () => {
    render(<Logo />);
    const link = screen.getByRole('link', { name: 'CKBadger Home' });
    expect(link).toHaveAttribute('href', '/mainnet');
    expect(link.className).not.toContain('hidden');
    const img = screen.getByAltText('CKBadger');
    expect(img).toHaveClass('logo-image', 'rotate-[8deg]', 'group-hover:rotate-[9deg]');
    expect(img.className).toContain('w-[clamp(96px,11vw,157px)]');
  });

  it('flickers once on every new block', () => {
    vi.useFakeTimers();
    render(<Logo />);
    const img = screen.getByAltText('CKBadger');
    expect(img).not.toHaveClass('neon-flicker');

    act(() => useRealtimeStore.setState({ latestBlock: block(100) }));
    expect(img).toHaveClass('neon-flicker');
    act(() => vi.advanceTimersByTime(820));
    expect(img).not.toHaveClass('neon-flicker');

    act(() => useRealtimeStore.setState({ latestBlock: block(101) }));
    expect(img).toHaveClass('neon-flicker');
  });
});
