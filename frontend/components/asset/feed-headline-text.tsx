import type { ItemFeedHeadline } from '@/lib/feed-headlines';

/** A feed headline as the boards set it: words, then any unworded protocol action raw in mono. */
export function FeedHeadlineText({ headline }: { headline: ItemFeedHeadline }) {
  return (
    <>
      {headline.text}
      {headline.raw?.map((raw, i) => (
        <span key={raw}>
          {headline.text || i > 0 ? ' · ' : ''}
          <code data-raw-action className="font-mono text-[12px] font-normal">
            {raw}
          </code>
        </span>
      ))}
    </>
  );
}
