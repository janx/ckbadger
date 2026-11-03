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
