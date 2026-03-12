import { beforeEach, describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { Logo } from '@/components/layout/logo';
import { useRealtimeStore } from '@/hooks/useRealtimeStore';

describe('Logo', () => {
  beforeEach(() => {
    useRealtimeStore.setState({ latestBlock: null });
  });

  it('uses the tuned size and adjusted vertical position', () => {
    render(<Logo />);

    const link = screen.getByRole('link', { name: 'CKBadger Home' });
    const image = screen.getByAltText('CKBadger');

    expect(link.className).toContain('md:-left-[14px]');
    expect(link.className).toContain('-top-[28px]');
    expect(link.className).toContain('md:-top-[22px]');
    expect(image.className).toContain('w-[96px]');
    expect(image.className).toContain('md:w-[112px]');
  });
});
