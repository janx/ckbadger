import { useId, type ReactNode } from 'react';
import { cn } from '@/lib/utils';
import {
  documentTitleParts,
  useDocumentTitle,
  type DocumentTitleProps,
} from '@/lib/use-document-title';

// F2 group `fiber`: the hero of the Fiber and Forks boards (V65, V67) — title and a one-sentence
// lede on the left, a 2 × 2 grid of figures on the right (hierarchy plan R1: index title, body
// lede, hero figures, meta labels). Hierarchy v3 (plan §1b, R11, R13) adds the `figure` layout:
// the DAO page's hero shape for a topic page with one defining number.

export interface TopicFigure {
  label: ReactNode;
  value: ReactNode;
  sub?: ReactNode;
  tone?: 'jade';
}

/** The page's one display figure (hierarchy v3, R4, R11, R13). */
export interface TopicDisplayFigure {
  /** What the number counts: the eyebrow over it (meta, upper case). */
  label: ReactNode;
  /** The number itself, at the display step (index under 640 px, R4). */
  value: ReactNode;
  /** Its unit on the same line, a step quieter ("channels", "%"). */
  unit?: ReactNode;
  /** One companion line on the figure's baseline row, at its right (body). */
  companion?: ReactNode;
  /** The page's class colour where it has one (R11: Fiber's jade); bright otherwise. */
  tone?: 'jade';
}

interface TopicHeroBaseProps {
  /** One sentence under the title; the concept prose belongs in tier 2. */
  lede: ReactNode;
  /** Undefined while the data loads: no empty grid (or, in the figure layout, no quiet row). */
  figures: readonly TopicFigure[] | undefined;
  breadcrumb?: ReactNode;
  /**
   * `grid` (default): the boards' title beside a 2 × 2 grid of hero figures. `figure` (hierarchy
   * v3): title, lede, the display `figure`, the `picture` under it, its `reading`, then `figures`
   * as one quiet row.
   */
  layout?: 'grid' | 'figure';
  /** `figure` layout: the page's one display figure; undefined while it loads. */
  figure?: TopicDisplayFigure;
  /** `figure` layout: the page's main graphic, directly under the figure. */
  picture?: ReactNode;
  /** `figure` layout: the picture's reading row and caption, under the hero section. */
  reading?: ReactNode;
}
/** `title` is the h1 and, as `[title]`, the browser tab's name (C1) unless `documentTitle` says. */
type TopicHeroProps = TopicHeroBaseProps & DocumentTitleProps;

export function TopicHero(props: TopicHeroProps) {
  useDocumentTitle(documentTitleParts(props.title, props.documentTitle));
  return props.layout === 'figure' ? <FigureHero {...props} /> : <GridHero {...props} />;
}

function GridHero({ title, lede, figures, breadcrumb }: TopicHeroProps) {
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

/**
 * Hierarchy v3 (R13), after the DAO page: the hero section holds the title, one short lede (dim,
 * ≤ 90 characters, 560 px), the one display figure with its companion on the same baseline row, and
 * the picture right under it; the picture's reading and the other figures (one quiet row: meta
 * label, section value, inline dim note) follow the section.
 */
function FigureHero({
  title,
  lede,
  figures,
  breadcrumb,
  figure,
  picture,
  reading,
}: TopicHeroProps) {
  const titleId = useId();
  return (
    <div className="min-w-0">
      <section aria-labelledby={titleId} className="min-w-0">
        {breadcrumb}
        {/* The title row, as the DAO page's (the h1 a flex item, its box the size of its words). */}
        <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-2">
          <h1 id={titleId} className="text-text-bright text-index font-medium">
            {title}
          </h1>
        </div>
        <p className="text-text-dim text-body mt-1.5 max-w-[560px]">{lede}</p>
        {figure && (
          <div data-role="figure" className="mt-9 min-w-0">
            <div className="text-text-dim text-meta mb-1.5 w-fit uppercase tracking-[0.08em]">
              {figure.label}
            </div>
            <div className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-2">
              <div
                className={cn(
                  'text-index sm:text-display whitespace-nowrap font-medium tabular-nums',
                  figure.tone === 'jade' ? 'text-jade' : 'text-text-bright'
                )}
              >
                {figure.value}
                {figure.unit && (
                  <span className="text-hero text-text-dim font-normal">
                    {figure.unit === '%' ? '' : ' '}
                    {figure.unit}
                  </span>
                )}
              </div>
              {figure.companion && (
                <p className="text-body text-text-dim min-w-0">{figure.companion}</p>
              )}
            </div>
          </div>
        )}
        {picture && <div className="mt-[18px] min-w-0">{picture}</div>}
      </section>
      {reading && <div className="mt-2.5 min-w-0">{reading}</div>}
      {figures && figures.length > 0 && (
        <dl
          className={cn(
            'mt-7 grid grid-cols-1 gap-x-8 gap-y-4 sm:grid-cols-2',
            figures.length > 3 ? 'lg:grid-cols-4' : 'lg:grid-cols-3'
          )}
        >
          {figures.map((item, index) => (
            <div key={index} className="min-w-0">
              <dt className="text-text-dim text-meta">{item.label}</dt>
              <dd className="text-text-bright text-section mt-0.5 w-fit tabular-nums">
                {item.value}
                {item.sub && <span className="text-text-dim text-small"> · {item.sub}</span>}
              </dd>
            </div>
          ))}
        </dl>
      )}
    </div>
  );
}
