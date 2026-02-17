import { describe, it, expect, vi } from 'vitest';
import { redirect } from 'next/navigation';
import NftsPage from '@/app/nfts/page';

vi.mock('next/navigation', () => ({
  redirect: vi.fn(),
}));

describe('NftsPage', () => {
  it('redirects to assets nft tab', () => {
    NftsPage();
    expect(redirect).toHaveBeenCalledWith('/assets?type=nft');
  });
});
