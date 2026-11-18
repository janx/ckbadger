import { type HTMLAttributes } from 'react';
import { cn } from '@/lib/utils';

/**
 * Tier-2 prose (material pass v2, leftovers): the one piece an explanation is set in — the Scripts,
 * Forks, Network, Fiber, DAO and chart pages' "how it works" paragraphs, a page's "in words". Its
 * `<p>` children read at the body step (14 px) on a 1.7 line, on the measure (`max-w-measure`, 76
 * characters: readability R4, lane B7), in the reading ink, 14 px apart (`.prose-tier2` in
 * app/globals.css); `em` is the true italic. A paragraph inside carries no size, colour or measure
 * class of its own. Drawings and lists between paragraphs keep their own width and spacing.
 */
export function Prose({ className, children, ...props }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div data-prose="" className={cn('prose-tier2 min-w-0', className)} {...props}>
      {children}
    </div>
  );
}
