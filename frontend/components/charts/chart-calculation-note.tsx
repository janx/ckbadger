import type { ChartDescription } from '@/components/charts/chart-calculation-descriptions';

interface ChartCalculationNoteProps {
  description: ChartDescription;
}

export function ChartCalculationNote({ description }: ChartCalculationNoteProps) {
  return (
    <div className="mt-6 rounded border border-slate-800 bg-slate-900/50 p-4">
      <div className="mb-2 font-mono text-xs uppercase tracking-wider text-slate-400">
        Description
      </div>
      <p className="text-sm leading-6 text-slate-300">{description.overview}</p>
      {description.legendItems.length > 0 && (
        <div className="mt-3 border-t border-slate-800 pt-3">
          <div className="mb-2 font-mono text-xs uppercase tracking-wider text-slate-500">
            Legend Item Calculation
          </div>
          <ul className="space-y-2 text-sm leading-6 text-slate-300">
            {description.legendItems.map((item) => (
              <li key={item.label}>
                <span className="font-medium text-slate-200">{item.label}: </span>
                <span>{item.description}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
