import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor } from '../utils/test-utils';
import AssetsRedirectPage from '@/app/assets/page';

const mockReplace = vi.fn();

vi.mock('@/src/navigation', () => ({
  useSearchParams: () => new URLSearchParams(window.location.search),
  usePathname: () => '/assets',
  useRouter: () => ({ replace: mockReplace }),
}));

vi.mock('@/lib/api', () => ({
  api: { getAssets: vi.fn() },
  isWarmupPendingError: vi.fn(() => false),
}));

describe('AssetsRedirectPage', () => {
  beforeEach(() => {
    mockReplace.mockClear();
  });

  it('redirects to /inventory/tokens by default', async () => {
    window.history.replaceState(null, '', '/assets');
    render(<AssetsRedirectPage />);
    await waitFor(() => {
      expect(mockReplace).toHaveBeenCalledWith('/inventory/tokens');
    });
  });

  it('redirects to /inventory/objects for type=object', async () => {
    window.history.replaceState(null, '', '/assets?type=object');
    render(<AssetsRedirectPage />);
    await waitFor(() => {
      expect(mockReplace).toHaveBeenCalledWith('/inventory/objects');
    });
  });

  it('redirects to /inventory/objects for legacy type=dob', async () => {
    window.history.replaceState(null, '', '/assets?type=dob');
    render(<AssetsRedirectPage />);
    await waitFor(() => {
      expect(mockReplace).toHaveBeenCalledWith('/inventory/objects');
    });
  });

  it('redirects to /inventory/tokens for legacy type=nft (no longer recognized)', async () => {
    window.history.replaceState(null, '', '/assets?type=nft');
    render(<AssetsRedirectPage />);
    await waitFor(() => {
      expect(mockReplace).toHaveBeenCalledWith('/inventory/tokens');
    });
  });

  it('redirects to /inventory/identities for type=identity', async () => {
    window.history.replaceState(null, '', '/assets?type=identity');
    render(<AssetsRedirectPage />);
    await waitFor(() => {
      expect(mockReplace).toHaveBeenCalledWith('/inventory/identities');
    });
  });
});
