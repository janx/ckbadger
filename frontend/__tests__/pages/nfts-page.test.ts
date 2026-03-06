import { describe, it, expect, vi } from 'vitest';
import { redirect } from '@/src/navigation';
import NftsPage from '@/app/nfts/page';

vi.mock('@/src/navigation', () => ({
  redirect: vi.fn(),
}));

describe('NftsPage', () => {
  it('redirects to assets nft tab', () => {
    NftsPage();
    expect(redirect).toHaveBeenCalledWith('/assets?type=nft');
  });
});
