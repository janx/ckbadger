'use client';

import type { StackedAreaChartResponse } from '@/lib/api';
import type { OccupationRangeKey } from '@/lib/occupation-range';
import { CapacityUtilization } from '@/components/ui/capacity-utilization';
import { OccupationRangeSelector } from '@/components/ui/occupation-range-selector';
import { StackedAreaChart } from '@/components/ui/stacked-area-chart';
import {
  TerminalPanel,
  TerminalPanelContent,
  TerminalPanelHeader,
} from '@/components/ui/terminal-panel';

interface CapacityOccupationSectionProps {
  description: string;
  occupationRange: OccupationRangeKey;
  onOccupationRangeChange: (range: OccupationRangeKey) => void;
  occupationChart: StackedAreaChartResponse | undefined;
  isOccupationChartLoading: boolean;
  totalCapacity?: string | null;
  occupiedCapacity?: string | null;
  totalCapacityLabel?: string;
  className?: string;
}

export function CapacityOccupationSection({
  description,
  occupationRange,
  onOccupationRangeChange,
  occupationChart,
  isOccupationChartLoading,
  totalCapacity,
  occupiedCapacity,
  totalCapacityLabel = 'Total Capacity',
  className,
}: CapacityOccupationSectionProps) {
  const hasCapacityData = Boolean(totalCapacity && occupiedCapacity);

  return (
    <TerminalPanel className={className}>
      <TerminalPanelHeader indicator="none">Capacity & Occupation</TerminalPanelHeader>
      <TerminalPanelContent>
        <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
          <div className="text-text-muted text-xs">{description}</div>
          <OccupationRangeSelector value={occupationRange} onChange={onOccupationRangeChange} />
        </div>

        {hasCapacityData && (
          <div className="border-base-border bg-base-bg/50 mb-3 rounded border p-3">
            <CapacityUtilization
              totalCapacity={totalCapacity!}
              occupiedCapacity={occupiedCapacity!}
              totalLabel={totalCapacityLabel}
            />
          </div>
        )}

        {isOccupationChartLoading ? (
          <div className="text-text-muted py-6 text-center">Loading occupation history...</div>
        ) : occupationChart && occupationChart.data.length > 0 ? (
          <StackedAreaChart
            data={occupationChart.data}
            series={occupationChart.series}
            height={180}
            valueUnit="shannon"
          />
        ) : (
          <div className="text-text-muted py-6 text-center">No occupation history yet</div>
        )}
      </TerminalPanelContent>
    </TerminalPanel>
  );
}
