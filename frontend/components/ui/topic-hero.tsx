import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

// F2 group `fiber`: the hero of the Fiber and Forks boards (V65, V67) — title and a one-sentence
// lede on the left, a 2 × 2 grid of figures on the right (hierarchy plan R1: index title, body
// lede, hero figures, meta labels).

export interface TopicFigure {
  label: ReactNode;
  value: ReactNode;
  sub?: ReactNode;
  tone?: 'jade';
}

export function TopicHero({
  title,
  lede,
  figures,
  breadcrumb,
}: {
  title: ReactNode;
  /** One sentence under the title; the concept prose belongs in tier 2. */
  lede: ReactNode;
  /** Undefined while the data loads: no empty grid. */
  figures: readonly TopicFigure[] | undefined;
  breadcrumb?: ReactNode;
}) {
  return (
    <div className="grid min-w-0 items-end gap-x-12 gap-y-6 lg:grid-cols-[minmax(0,1fr)_440px]">
      <div className="flex min-w-0 flex-col gap-2.5">
        {breadcrumb}
        <h1 className="text-text-bright text-index font-medium">{title}</h1>
        <p className="text-text text-body max-w-[720px] leading-relaxed">{lede}</p>
      </div>
      {figures && (
        <dl className="grid min-w-0 grid-cols-2 gap-x-7 gap-y-4">
          {figures.map((figure, index) => (
            <div key={index} className="min-w-0">
              <dt className="text-text-dim text-meta">{figure.label}</dt>
              <dd
                className={cn(
                  'text-hero mt-[3px] tabular-nums',
                  figure.tone === 'jade' ? 'text-jade' : 'text-text-bright'
                )}
              >
                {figure.value}
              </dd>
              {figure.sub && <dd className="text-text-dim text-meta mt-0.5">{figure.sub}</dd>}
            </div>
          ))}
        </dl>
      )}
    </div>
  );
}
