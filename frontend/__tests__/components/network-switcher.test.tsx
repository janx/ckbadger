import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { NetworkSwitcher } from '@/components/layout/network-switcher';

// Capture router.push; drive pathname + active network per-test via refs.
const pushMock = vi.hoisted(() => vi.fn());
const pathnameRef = vi.hoisted(() => ({ current: '/dao' }));
const activeRef = vi.hoisted(() => ({ current: 'mainnet' }));

vi.mock('@/src/navigation', () => ({
  useRouter: () => ({
    push: pushMock,
    replace: vi.fn(),
    back: vi.fn(),
    forward: vi.fn(),
    refresh: vi.fn(),
    prefetch: vi.fn(),
  }),
  usePathname: () => pathnameRef.current,
}));

vi.mock('@/hooks/useActiveNetwork', () => ({
  useActiveNetwork: () => activeRef.current,
}));

describe('NetworkSwitcher', () => {
  beforeEach(() => {
    pushMock.mockReset();
    pathnameRef.current = '/dao';
    activeRef.current = 'mainnet';
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
  });

  afterEach(() => {
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  it('renders one button per live network and marks the active one', () => {
    render(<NetworkSwitcher />);

    const mainnet = screen.getByRole('button', { name: 'Switch to mainnet' });
    const testnet = screen.getByRole('button', { name: 'Switch to testnet' });

    expect(mainnet).toBeInTheDocument();
    expect(testnet).toBeInTheDocument();
    // Active network carries the distinguishing marker + active style.
    expect(mainnet).toHaveAttribute('aria-current', 'page');
    expect(testnet).not.toHaveAttribute('aria-current');
    expect(mainnet.className).toContain('text-jade');
    expect(testnet.className).not.toContain('text-jade');
  });

  it('navigates to the same page under the chosen network prefix on click', () => {
    render(<NetworkSwitcher />);

    fireEvent.click(screen.getByRole('button', { name: 'Switch to testnet' }));

    expect(pushMock).toHaveBeenCalledTimes(1);
    expect(pushMock).toHaveBeenCalledWith('/testnet/dao');
  });

  it('builds a bare `/<network>` target from the root path', () => {
    pathnameRef.current = '/';
    render(<NetworkSwitcher />);

    fireEvent.click(screen.getByRole('button', { name: 'Switch to testnet' }));

    expect(pushMock).toHaveBeenCalledWith('/testnet');
  });

  it('renders nothing when only one network is live', () => {
    window.__CKBADGER_RUNTIME_CONFIG__ = { networks: [{ name: 'mainnet' }] };

    const { container } = render(<NetworkSwitcher />);

    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByTestId('network-switcher')).toBeNull();
  });
});
