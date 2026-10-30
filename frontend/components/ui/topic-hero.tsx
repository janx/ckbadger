import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

// F2 group `fiber`: the hero of the Fiber and Forks boards (V65, V67) — title and a plain
// paragraph on the left, a 2 × 2 grid of figures on the right.

export interface TopicFigure {
  label: ReactNode;
  value: ReactNode;
  sub?: ReactNode;
  tone?: 'jade';
}

export function TopicHero({
  title,
  intro,
  figures,
  breadcrumb,
}: {
  title: ReactNode;
  intro: ReactNode;
  /** Undefined while the data loads: no empty grid. */
  figures: readonly TopicFigure[] | undefined;
  breadcrumb?: ReactNode;
}) {
  return (
    <div className="grid min-w-0 items-end gap-x-12 gap-y-6 lg:grid-cols-[minmax(0,1fr)_440px]">
      <div className="flex min-w-0 flex-col gap-2.5">
        {breadcrumb}
        <h1 className="text-text-bright text-[26px] font-medium">{title}</h1>
        <p className="text-text max-w-[720px] text-[15px] leading-relaxed">{intro}</p>
      </div>
      {figures && (
        <dl className="grid min-w-0 grid-cols-2 gap-x-7 gap-y-4">
          {figures.map((figure, index) => (
            <div key={index} className="min-w-0">
              <dt className="text-text-dim text-[12px]">{figure.label}</dt>
              <dd
                className={cn(
                  'mt-[3px] text-[18px] tabular-nums',
                  figure.tone === 'jade' ? 'text-jade' : 'text-text-bright'
                )}
              >
                {figure.value}
              </dd>
              {figure.sub && <dd className="text-text-dim mt-0.5 text-[11.5px]">{figure.sub}</dd>}
            </div>
          ))}
        </dl>
      )}
    </div>
  );
}
