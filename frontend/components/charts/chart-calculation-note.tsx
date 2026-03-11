import type { ChartDescription } from '@/components/charts/chart-calculation-descriptions';

interface ChartCalculationNoteProps {
  description: ChartDescription;
}

export function ChartCalculationNote({ description }: ChartCalculationNoteProps) {
  return (
    <div className="border-base-border bg-base-surface/50 mt-6 rounded border p-4">
      <div className="text-text-dim mb-2 font-mono text-xs uppercase tracking-wider">
        Description
      </div>
      <p className="text-text text-sm leading-6">{description.overview}</p>
      {description.legendItems.length > 0 && (
        <div className="border-base-border mt-3 border-t pt-3">
          <div className="text-text-dim mb-2 font-mono text-xs uppercase tracking-wider">
            Legend Item Calculation
          </div>
          <ul className="text-text space-y-2 text-sm leading-6">
            {description.legendItems.map((item) => (
              <li key={item.label}>
                <span className="text-text-bright font-medium">{item.label}: </span>
                <span>{item.description}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
