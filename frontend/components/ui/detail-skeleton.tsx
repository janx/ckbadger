import { cn } from '@/lib/utils';

const BAR = 'bg-base-surface animate-pulse rounded';

/**
 * An entity page while its data loads (visual finish V8): the shape of what is coming, not a
 * "Loading…" line — the h1 (30 px, 40 %), the lede (13 px, 60 %) and the hero (72 px, the full
 * width) as three quiet bars, at DetailLayout's spacing (the identity line 8 px under the h1, tier
 * one 32 px under the header). Rendered where DetailLayout would be, inside the page's main; the
 * page's content fades in over it when the data arrives (DetailLayout carries `.arrive`).
 */
export function DetailSkeleton() {
  return (
    <div role="status" aria-label="Loading …" className="min-w-0">
      <div className={cn(BAR, 'h-[30px] w-2/5')} />
      <div className={cn(BAR, 'mt-2 h-[13px] w-3/5')} />
      <div className={cn(BAR, 'mt-8 h-[72px] w-full')} />
    </div>
  );
}

/**
 * The skeleton in the page's main and its gutters, for a page whose layout owns the main
 * (AssetDetailShell: the token, object and identity pages).
 */
export function DetailSkeletonPage() {
  return (
    <main className="container mx-auto min-w-0 py-8">
      <DetailSkeleton />
    </main>
  );
}
