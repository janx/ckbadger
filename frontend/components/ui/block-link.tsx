import { Fragment, type ReactNode } from 'react';
import Link from '@/components/ui/link';
import { formatBlockNumber } from '@/lib/format-block';
import { cn } from '@/lib/utils';

/**
 * Lane L (L2): a block height written as text is the block's name, so it links to the block's
 * page — `#20,894,845` → `/blocks/20894845`. The link keeps the surrounding ink (it is a name in
 * a sentence, not a button) and turns jade on hover, as the site's other inline links do.
 */
export function BlockLink({
  number,
  className,
}: {
  number: number | bigint | string;
  className?: string;
}) {
  const name = formatBlockNumber(number);
  return (
    <Link
      href={`/blocks/${name.slice(1).replace(/,/g, '')}`}
      className={cn('hover:text-jade', className)}
    >
      {name}
    </Link>
  );
}

/**
 * A sentence computed once and rendered twice: as text (for a `title`, a test, a log) and as
 * nodes with every block height a {@link BlockLink}. The parts are the sentence's single
 * computation; the two renderings never diverge.
 */
export type SentencePart = string | { block: number };

export function sentenceText(parts: readonly SentencePart[]): string {
  return parts.map((p) => (typeof p === 'string' ? p : formatBlockNumber(p.block))).join('');
}

export function SentenceNodes({ parts }: { parts: readonly SentencePart[] }): ReactNode {
  return parts.map((p, i) =>
    typeof p === 'string' ? (
      <Fragment key={i}>{p}</Fragment>
    ) : (
      <BlockLink key={i} number={p.block} />
    )
  );
}

/** `#1 and #2`, `#1, #2 and #3`: a list of heights as sentence parts. */
export function heightListParts(heights: readonly number[]): SentencePart[] {
  const out: SentencePart[] = [];
  heights.forEach((h, i) => {
    if (i > 0) out.push(i === heights.length - 1 ? ' and ' : ', ');
    out.push({ block: h });
  });
  return out;
}
