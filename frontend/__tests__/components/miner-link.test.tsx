import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { MinerLink, minerName } from '@/components/forks/miner-link';

const address = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqw93hhsj';

describe('MinerLink', () => {
  // U-09 (re-pinned): an unnamed miner is its address at the caller's size, head and tail (it was
  // the tail alone, "…93hhsj", a form of its own); R9 (re-pinned): the row size is `ckb1…` and 8
  it('names a miner by its address at its size and links the address', () => {
    render(<MinerLink look="key" size="row" address={address} pool={null} />);
    const link = screen.getByRole('link', { name: 'ckb1…qw93hhsj' });
    expect(link).toHaveAttribute('href', `/mainnet/address/${address}`);
    expect(link).toHaveAttribute('title', address);
  });

  // Re-pinned on purpose (lane P1 follow-up; the F3-e note: the title was the address alone, so a
  // pool name a narrow place cuts showed nowhere whole): the title names the pool whole, then the
  // address it is read from
  it('names a miner by the pool pools.toml names, the pool whole and the address in the title (R-12)', () => {
    render(
      <MinerLink
        look="key"
        size="row"
        address={address}
        pool="Example Pool"
        unnamed={<span>full</span>}
      />
    );
    const link = screen.getByRole('link', { name: 'Example Pool' });
    expect(link).toHaveAttribute('href', `/mainnet/address/${address}`);
    expect(link).toHaveAttribute('title', `Example Pool · ${address}`);
    expect(screen.queryByText('full')).toBeNull();
  });

  it('lets a page show the whole address for an unnamed miner', () => {
    render(
      <MinerLink look="key" size="row" address={address} pool={null} unnamed={<span>full</span>} />
    );
    expect(screen.getByText('full')).toBeInTheDocument();
    expect(screen.queryByRole('link')).toBeNull();
  });

  // Re-pinned on purpose (lane cd-fixes-controls, review-p-tests SHOULD-4): a miner states its
  // look, which is required, and draws no ink of its own: the bright ink "by default" and the row's
  // hand ink were for callers that stated none, and every caller states one. It was "is bright by
  // default and takes a row ink from its caller".
  it('states its look: a row’s key in the bright ink, none of its own', () => {
    render(<MinerLink size="row" address={address} pool="Example Pool" look="key" />);
    const link = screen.getByRole('link', { name: 'Example Pool' });
    expect(link).toHaveClass('text-text-bright');
    expect(link).not.toHaveClass('hover:text-jade');
    // @ts-expect-error a miner's link states its look
    render(<MinerLink size="row" address={address} pool={null} />);
  });

  // Lane P4: a caller that states the link's job gets the owner of the looks' drawing and no ink of
  // MinerLink's own (the fork pages write a miner inside a line or a cell: `inline`)
  it('draws the look its caller states, with no ink of its own', () => {
    render(<MinerLink size="row" address={address} pool="Example Pool" look="inline" />);
    const link = screen.getByRole('link', { name: 'Example Pool' });
    expect(link).toHaveClass('underline');
    expect(link).not.toHaveClass('text-text-bright');
    expect(link).not.toHaveClass('hover:text-jade');
  });

  it('has one naming rule for text-only places (tree tiles, the narrative)', () => {
    expect(minerName(address, null, 'tight')).toBe('ckb1…93hhsj');
    expect(minerName(address, null, 'head')).toBe('ckb1…qw93hhsj');
    expect(minerName(address, 'Example Pool', 'row')).toBe('Example Pool');
  });
});
