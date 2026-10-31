import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { MinerLink, minerName } from '@/components/forks/miner-link';

const address = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqw93hhsj';

describe('MinerLink', () => {
  it('names a miner by its address tail and links the address', () => {
    render(<MinerLink address={address} pool={null} />);
    const link = screen.getByRole('link', { name: '…93hhsj' });
    expect(link).toHaveAttribute('href', `/mainnet/address/${address}`);
    expect(link).toHaveAttribute('title', address);
  });

  it('names a miner by the pool pools.toml names, keeping the address in the title (R-12)', () => {
    render(<MinerLink address={address} pool="Example Pool" unnamed={<span>full</span>} />);
    const link = screen.getByRole('link', { name: 'Example Pool' });
    expect(link).toHaveAttribute('href', `/mainnet/address/${address}`);
    expect(link).toHaveAttribute('title', address);
    expect(screen.queryByText('full')).toBeNull();
  });

  it('lets a page show the whole address for an unnamed miner', () => {
    render(<MinerLink address={address} pool={null} unnamed={<span>full</span>} />);
    expect(screen.getByText('full')).toBeInTheDocument();
    expect(screen.queryByRole('link')).toBeNull();
  });

  it('has one naming rule for text-only places (tree tiles, the narrative)', () => {
    expect(minerName(address, null)).toBe('…93hhsj');
    expect(minerName(address, 'Example Pool')).toBe('Example Pool');
  });
});
