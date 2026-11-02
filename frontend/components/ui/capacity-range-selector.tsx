'use client';

import { CAPACITY_RANGE_OPTIONS, CapacityRangeKey } from '@/lib/capacity-range';
import { cn } from '@/lib/utils';

interface CapacityRangeSelectorProps {
  value: CapacityRangeKey;
  onChange: (range: CapacityRangeKey) => void;
  /**
   * Hierarchy v3 (R11, R13; additive, off = the boxed selector with the jade pressed button): one
   * inline group of buttons for a page's title row, the pressed one neutral, as the DAO range.
   */
  quiet?: boolean;
}

export function CapacityRangeSelector({
  value,
  onChange,
  quiet = false,
}: CapacityRangeSelectorProps) {
  if (quiet)
    return (
      <div role="group" aria-label="Range" className="whitespace-nowrap">
        {CAPACITY_RANGE_OPTIONS.map((option, i) => {
          const active = value === option.key;
          return (
            <button
              key={option.key}
              type="button"
              aria-pressed={active}
              onClick={() => onChange(option.key)}
              className={cn(
                'text-small inline-flex h-7 items-center rounded-[4px] border px-2.5 font-mono',
                i > 0 && 'ml-1',
                active
                  ? 'border-base-border bg-base-border-subtle text-text-bright'
                  : 'border-base-border-subtle text-text hover:text-text-bright'
              )}
            >
              {option.label}
            </button>
          );
        })}
      </div>
    );
  return (
    <div className="border-base-border bg-base-surface/50 flex items-center gap-1 rounded border p-1">
      {CAPACITY_RANGE_OPTIONS.map((option) => {
        const active = value === option.key;
        return (
          <button
            key={option.key}
            type="button"
            onClick={() => onChange(option.key)}
            className={`text-small rounded px-2 py-1 font-mono transition-colors ${
              active
                ? 'bg-jade/15 text-jade border-jade-dim border'
                : 'text-text-dim hover:bg-base-elevated hover:text-text-bright'
            }`}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
