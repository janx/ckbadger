// Lane L (L2): a block height written as text links to the block; a sentence computed as parts
// renders once as text and once with every height a link, from the same parts.
import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import {
  BlockLink,
  SentenceNodes,
  heightListParts,
  sentenceText,
  type SentencePart,
} from '@/components/ui/block-link';

describe('BlockLink', () => {
  it('writes the height the site way and links its block', () => {
    render(<BlockLink number={20894845} />);
    expect(screen.getByRole('link', { name: '#20,894,845' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/20894845'
    );
  });

  it('refuses a value that is not a height', () => {
    expect(() => render(<BlockLink number={-1} />)).toThrow('Invalid block number -1');
  });
});

describe('sentence parts', () => {
  const parts: SentencePart[] = ['committed what ', ...heightListParts([7, 8, 9]), ' proposed'];

  it('renders the same words as text and as nodes', () => {
    expect(sentenceText(parts)).toBe('committed what #7, #8 and #9 proposed');
    const { container } = render(
      <p>
        <SentenceNodes parts={parts} />
      </p>
    );
    expect(container.textContent).toBe('committed what #7, #8 and #9 proposed');
    expect(screen.getAllByRole('link').map((a) => a.getAttribute('href'))).toEqual([
      '/mainnet/blocks/7',
      '/mainnet/blocks/8',
      '/mainnet/blocks/9',
    ]);
  });

  it('joins two heights with "and"', () => {
    expect(sentenceText(heightListParts([1, 2]))).toBe('#1 and #2');
    expect(sentenceText(heightListParts([1]))).toBe('#1');
  });
});

// Lane c2-links: a height's look is where it stands, which its caller says; in a sentence it is a
// name inside a line, in the line's ink, underlined in the muted ink (re-pinned with the
// coordinator's ruling: inline was "no ink of its own" while the underline's colour waited).
describe('BlockLink’s look', () => {
  const inks = (el: Element) =>
    [...el.classList].filter((c) => /^(hover:)?text-|underline|decoration/.test(c));
  const UNDERLINE = [
    'underline',
    'decoration-1',
    'underline-offset-[3px]',
    'decoration-[color:var(--c-ink-muted)]',
  ];

  it('draws a row’s key bright, and a height inside a line in the line’s ink, underlined', () => {
    render(
      <>
        <BlockLink number={7} look="key" />
        <BlockLink number={8} look="inline" />
        <BlockLink number={9} />
      </>
    );
    const [key, inline, none] = screen.getAllByRole('link');
    expect(inks(key)).toEqual(['text-text-bright']);
    expect(inks(inline)).toEqual(UNDERLINE);
    // no ink of its own where the caller states none, as before (hover is the one rule)
    expect(inks(none)).toEqual([]);
  });

  it('writes a sentence’s heights as links inside the line', () => {
    const { container } = render(
      <p className="text-text">
        <SentenceNodes parts={heightListParts([7, 8])} />
      </p>
    );
    const links = container.querySelectorAll('a');
    expect(links).toHaveLength(2);
    for (const link of links) expect(inks(link)).toEqual(UNDERLINE);
  });
});
