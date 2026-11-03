import Link from '@/components/ui/link';
import { CHARTS } from '@/components/charts/chart-catalogue';

/** The pages the catalogue's charts take their numbers from (each chart's `evidenceHref`). */
export type ChartTopic =
  | '/blocks'
  | '/transactions'
  | '/activities'
  | '/dao'
  | '/scripts'
  | '/inventory';

/**
 * Lane L (charts ↔ topics): a topic page links every chart whose numbers come from it, by the
 * chart's own title, in the catalogue's order; each chart links back with "Explore …". One quiet
 * line at the end of the page.
 */
export function TopicCharts({ topic }: { topic: ChartTopic }) {
  const charts = CHARTS.filter((chart) => chart.evidenceHref === topic);
  if (charts.length === 0) throw new Error(`No chart takes its numbers from ${topic}`);
  return (
    <nav
      aria-label="Charts of these numbers"
      className="border-base-border text-small mt-12 flex flex-wrap items-baseline gap-x-4 gap-y-1.5 border-t pt-4"
    >
      <span className="text-text-dim">Charts</span>
      {charts.map((chart) => (
        <Link key={chart.slug} href={`/charts/${chart.slug}`} className="text-text hover:text-jade">
          {chart.title}
        </Link>
      ))}
    </nav>
  );
}
