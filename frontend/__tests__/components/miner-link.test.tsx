import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { MinerLink } from '@/components/forks/miner-link';

describe('MinerLink', () => {
  it('names a miner by its address tail and links the address', () => {
    const address = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqw93hhsj';
    render(<MinerLink address={address} />);
    const link = screen.getByRole('link', { name: '…93hhsj' });
    expect(link).toHaveAttribute('href', `/mainnet/address/${address}`);
    expect(link).toHaveAttribute('title', address);
  });
});
