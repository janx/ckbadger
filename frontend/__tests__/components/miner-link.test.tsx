import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { MinerLink, minerName } from '@/components/forks/miner-link';

const address = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqw93hhsj';

describe('MinerLink', () => {
  // U-09 (re-pinned): an unnamed miner is its address at the caller's size, head and tail (it was
  // the tail alone, "…93hhsj", a form of its own)
  it('names a miner by its address at its size and links the address', () => {
    render(<MinerLink size="row" address={address} pool={null} />);
    const link = screen.getByRole('link', { name: 'ckb1qzda0c…93hhsj' });
    expect(link).toHaveAttribute('href', `/mainnet/address/${address}`);
    expect(link).toHaveAttribute('title', address);
  });

  it('names a miner by the pool pools.toml names, keeping the address in the title (R-12)', () => {
    render(
      <MinerLink size="row" address={address} pool="Example Pool" unnamed={<span>full</span>} />
    );
    const link = screen.getByRole('link', { name: 'Example Pool' });
    expect(link).toHaveAttribute('href', `/mainnet/address/${address}`);
    expect(link).toHaveAttribute('title', address);
    expect(screen.queryByText('full')).toBeNull();
  });

  it('lets a page show the whole address for an unnamed miner', () => {
    render(<MinerLink size="row" address={address} pool={null} unnamed={<span>full</span>} />);
    expect(screen.getByText('full')).toBeInTheDocument();
    expect(screen.queryByRole('link')).toBeNull();
  });

  // Table-rows T5: in a list row the key cell is the one bright thing, so a row can set the
  // miner in body ink; everywhere else it stays bright as before.
  it('is bright by default and takes a row ink from its caller', () => {
    const { rerender } = render(<MinerLink size="row" address={address} pool="Example Pool" />);
    expect(screen.getByRole('link', { name: 'Example Pool' })).toHaveClass('text-text-bright');
    rerender(<MinerLink size="row" address={address} pool="Example Pool" className="text-text" />);
    const link = screen.getByRole('link', { name: 'Example Pool' });
    expect(link).toHaveClass('text-text', 'hover:text-jade');
    expect(link).not.toHaveClass('text-text-bright');
  });

  it('has one naming rule for text-only places (tree tiles, the narrative)', () => {
    expect(minerName(address, null, 'tight')).toBe('ckb1…93hhsj');
    expect(minerName(address, null, 'head')).toBe('ckb1qzda0cr0…qw93hhsj');
    expect(minerName(address, 'Example Pool', 'row')).toBe('Example Pool');
  });
});
