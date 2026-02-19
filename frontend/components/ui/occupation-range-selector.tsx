'use client';

import { OCCUPATION_RANGE_OPTIONS, OccupationRangeKey } from '@/lib/occupation-range';

interface OccupationRangeSelectorProps {
  value: OccupationRangeKey;
  onChange: (range: OccupationRangeKey) => void;
}

export function OccupationRangeSelector({ value, onChange }: OccupationRangeSelectorProps) {
  return (
    <div className="mb-3 flex items-center gap-2">
      <span className="font-mono text-xs uppercase tracking-wider text-slate-500">Range</span>
      <div className="flex items-center gap-1 rounded border border-slate-800 bg-slate-900/50 p-1">
        {OCCUPATION_RANGE_OPTIONS.map((option) => {
          const active = value === option.key;
          return (
            <button
              key={option.key}
              type="button"
              onClick={() => onChange(option.key)}
              className={`rounded px-2 py-1 font-mono text-xs transition-colors ${
                active
                  ? 'bg-terminal-dark/30 text-terminal-green border-terminal-dark border'
                  : 'text-slate-400 hover:bg-slate-800 hover:text-slate-200'
              }`}
            >
              {option.label}
            </button>
          );
        })}
      </div>
    </div>
  );
}
