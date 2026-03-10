'use client';

import { OCCUPATION_RANGE_OPTIONS, OccupationRangeKey } from '@/lib/occupation-range';

interface OccupationRangeSelectorProps {
  value: OccupationRangeKey;
  onChange: (range: OccupationRangeKey) => void;
}

export function OccupationRangeSelector({ value, onChange }: OccupationRangeSelectorProps) {
  return (
    <div className="mb-3 flex items-center gap-2">
      <span className="text-text-muted font-mono text-xs uppercase tracking-wider">Range</span>
      <div className="border-base-border bg-base-surface/50 flex items-center gap-1 rounded border p-1">
        {OCCUPATION_RANGE_OPTIONS.map((option) => {
          const active = value === option.key;
          return (
            <button
              key={option.key}
              type="button"
              onClick={() => onChange(option.key)}
              className={`rounded px-2 py-1 font-mono text-xs transition-colors ${
                active
                  ? 'bg-amber/15 text-amber border-amber-dim border'
                  : 'text-text-muted hover:bg-base-elevated hover:text-text-primary'
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
