import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { NetworkSwitcher } from '@/components/layout/network-switcher';

// Capture router.push; drive pathname, search+hash and active network per-test via refs.
const pushMock = vi.hoisted(() => vi.fn());
const pathnameRef = vi.hoisted(() => ({ current: '/dao' }));
const searchAndHashRef = vi.hoisted(() => ({ current: '' }));
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
  useSearchAndHash: () => searchAndHashRef.current,
}));

vi.mock('@/hooks/useActiveNetwork', () => ({
  useActiveNetwork: () => activeRef.current,
}));

describe('NetworkSwitcher', () => {
  beforeEach(() => {
    pushMock.mockReset();
    pathnameRef.current = '/dao';
    searchAndHashRef.current = '';
    activeRef.current = 'mainnet';
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
  });

  afterEach(() => {
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  it('renders the active network as a distinct chain-context selector', () => {
    render(<NetworkSwitcher />);

    const trigger = screen.getByRole('button', { name: 'Select network' });
    const menu = screen.getByRole('menu', { name: 'CKB network' });
    const mainnet = within(menu).getByRole('menuitemradio', { name: 'Switch to mainnet' });
    const testnet = within(menu).getByRole('menuitemradio', { name: 'Switch to testnet' });

    expect(trigger).toHaveTextContent('mainnet');
    expect(trigger).not.toHaveTextContent('CKB network');
    expect(trigger).toHaveAttribute('aria-haspopup', 'menu');
    expect(screen.getByText('CKB network')).toBeInTheDocument();
    expect(mainnet).toHaveAttribute('aria-current', 'page');
    expect(mainnet).toHaveAttribute('aria-checked', 'true');
    expect(testnet).not.toHaveAttribute('aria-current');
    expect(testnet).toHaveAttribute('aria-checked', 'false');
  });

  it('navigates to the same page under the chosen network prefix on selection', () => {
    render(<NetworkSwitcher />);

    fireEvent.click(screen.getByRole('menuitemradio', { name: 'Switch to testnet' }));

    expect(pushMock).toHaveBeenCalledTimes(1);
    expect(pushMock).toHaveBeenCalledWith('/testnet/dao');
  });

  it('builds a bare `/<network>` target from the root path', () => {
    pathnameRef.current = '/';
    render(<NetworkSwitcher />);

    fireEvent.click(screen.getByRole('menuitemradio', { name: 'Switch to testnet' }));

    expect(pushMock).toHaveBeenCalledWith('/testnet');
  });

  it('preserves the query string and hash when switching networks', () => {
    // Browsing /mainnet/activities?type=dao#row-7
    pathnameRef.current = '/activities';
    searchAndHashRef.current = '?type=dao#row-7';
    render(<NetworkSwitcher />);

    fireEvent.click(screen.getByRole('menuitemradio', { name: 'Switch to testnet' }));

    // NetworkGuard preserves search+hash when it prefixes a path; the switcher
    // must not silently drop the user's filters and anchor.
    expect(pushMock).toHaveBeenCalledWith('/testnet/activities?type=dao#row-7');
  });

  it('renders nothing when only one network is live', () => {
    window.__CKBADGER_RUNTIME_CONFIG__ = { networks: [{ name: 'mainnet' }] };

    const { container } = render(<NetworkSwitcher />);

    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByTestId('network-switcher')).toBeNull();
  });
});
