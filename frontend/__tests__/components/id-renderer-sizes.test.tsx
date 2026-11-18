import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Address } from '@/components/ui/address';
import { Hash } from '@/components/ui/hash';
import { HexCode } from '@/components/ui/hex-code';
import { HexDisplay } from '@/components/ui/hex-display';
import { ShortId } from '@/components/ui/short-id';

/**
 * The id renderers take one of the three sizes of lib/shorten.ts and no length (U-09, PB-19): a
 * call site picks `head`, `row` or `tight`, never its own number of characters. The
 * `@ts-expect-error` lines are the type-level check — `pnpm type-check` fails if a renderer accepts
 * a length again — and the renders show a length smuggled past the types changes nothing.
 */
const HASH = '0x8e4f9c6f1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5';
const ADDRESS =
  'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqwgx292hnvmn68xf779vmzrshpmm6epn4c0cgwga';
const LENGTHS = { startChars: 20, endChars: 2 } as object;

describe('HexCode and Hash write a hash at a size', () => {
  it('at head, row and tight', () => {
    render(
      <>
        <HexCode value={HASH} size="head" copyable={false} />
        <Hash hash={HASH} size="row" copyable={false} />
        <Hash hash={HASH} size="tight" copyable={false} />
      </>
    );
    expect(screen.getByText('0x8e4f9c6f1a…92a3b4c5')).toHaveAttribute('data-nobreak');
    expect(screen.getByText('0x8e4f9c6f…a3b4c5')).toHaveAttribute('data-nobreak');
    expect(screen.getByText('0x8e4f…b4c5')).toHaveAttribute('data-nobreak');
  });

  it('take no length', () => {
    // @ts-expect-error a length is not a prop of HexCode
    render(<HexCode value={HASH} size="tight" startChars={20} copyable={false} />);
    // @ts-expect-error a length is not a prop of Hash
    render(<Hash hash={HASH} size="tight" endChars={2} copyable={false} />);
    render(<Hash hash={HASH} size="tight" {...LENGTHS} copyable={false} />);
    expect(screen.getAllByText('0x8e4f…b4c5')).toHaveLength(3);
  });

  it('take a size or truncate={false}, never neither', () => {
    // a size left out fails fast, at the type and at run time
    // @ts-expect-error a shortened hash names its size
    expect(() => render(<Hash hash={HASH} copyable={false} />)).toThrow(/no size/);
    // @ts-expect-error a whole hash has no size
    render(<Hash hash={HASH} truncate={false} size="row" copyable={false} />);
    expect(screen.getAllByText(HASH)).toHaveLength(1);
  });
});

describe('Address writes an address at a size', () => {
  it('at head, row and tight, the full address in its title', () => {
    render(
      <>
        <Address address={ADDRESS} size="head" />
        <Address address={ADDRESS} size="row" />
        <Address address={ADDRESS} size="tight" />
      </>
    );
    const links = screen.getAllByRole('link');
    // R9 (re-pinned): `ckb1…` and the last 8 at the head and row sizes (12…8 and 10…6 before)
    expect(links.map((link) => link.textContent)).toEqual([
      'ckb1…4c0cgwga',
      'ckb1…4c0cgwga',
      'ckb1…0cgwga',
    ]);
    for (const link of links) expect(link).toHaveAttribute('title', ADDRESS);
  });

  it('takes no length', () => {
    // @ts-expect-error a length is not a prop of Address
    render(<Address address={ADDRESS} size="row" startChars={8} endChars={6} />);
    render(<Address address={ADDRESS} size="row" {...LENGTHS} />);
    for (const link of screen.getAllByRole('link')) expect(link.textContent).toBe('ckb1…4c0cgwga');
  });
});

describe('HexDisplay and ShortId take the same sizes', () => {
  it('HexDisplay draws the hash at its size with the one ellipsis', () => {
    const { container } = render(<HexDisplay value={HASH} size="row" copyable={false} />);
    expect(container).toHaveTextContent('0x8e4f9c6f…a3b4c5');
    // @ts-expect-error a length is not a prop of HexDisplay
    render(<HexDisplay value={HASH} size="row" startChars={4} />);
  });

  it('ShortId writes a hash or an address as one unbroken unit, with no title of its own', () => {
    render(
      <>
        <ShortId value={HASH} kind="hash" size="tight" />
        <ShortId value={ADDRESS} kind="address" size="tight" />
      </>
    );
    for (const text of ['0x8e4f…b4c5', 'ckb1…0cgwga']) {
      const unit = screen.getByText(text);
      expect(unit).toHaveAttribute('data-nobreak');
      expect(unit).not.toHaveAttribute('title');
    }
  });
});
