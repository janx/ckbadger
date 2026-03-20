'use client';

import type { StackedAreaChartResponse } from '@/lib/api';
import type { CapacityRangeKey } from '@/lib/capacity-range';
import { CapacityUtilization } from '@/components/ui/capacity-utilization';
import { CapacityRangeSelector } from '@/components/ui/capacity-range-selector';
import { StackedAreaChart } from '@/components/ui/stacked-area-chart';
import {
  TerminalPanel,
  TerminalPanelContent,
  TerminalPanelHeader,
} from '@/components/ui/terminal-panel';

interface CapacityStatisticsSectionProps {
  capacityRange: CapacityRangeKey;
  onCapacityRangeChange: (range: CapacityRangeKey) => void;
  capacityChart: StackedAreaChartResponse | undefined;
  isCapacityChartLoading: boolean;
  totalCapacity?: string | null;
  commonKnowledgeSize?: string | null;
  totalCapacityLabel?: string;
  className?: string;
}

export function CapacityStatisticsSection({
  capacityRange,
  onCapacityRangeChange,
  capacityChart,
  isCapacityChartLoading,
  totalCapacity,
  commonKnowledgeSize,
  totalCapacityLabel = 'Total Capacity',
  className,
}: CapacityStatisticsSectionProps) {
  const hasCapacityData = Boolean(totalCapacity && commonKnowledgeSize);

  return (
    <TerminalPanel className={className}>
      <TerminalPanelHeader
        indicator="none"
        actions={<CapacityRangeSelector value={capacityRange} onChange={onCapacityRangeChange} />}
      >
        Capacity Statistics
      </TerminalPanelHeader>
      <TerminalPanelContent>
        {hasCapacityData && (
          <CapacityUtilization
            totalCapacity={totalCapacity!}
            commonKnowledgeSize={commonKnowledgeSize!}
            totalLabel={totalCapacityLabel}
            className="mb-3"
          />
        )}

        {isCapacityChartLoading ? (
          <div className="text-text-dim py-6 text-center">Loading capacity history...</div>
        ) : capacityChart && capacityChart.data.length > 0 ? (
          <StackedAreaChart
            data={capacityChart.data}
            series={capacityChart.series}
            height={180}
            valueUnit="shannon"
          />
        ) : (
          <div className="text-text-dim py-6 text-center">No capacity history yet</div>
        )}
      </TerminalPanelContent>
    </TerminalPanel>
  );
}
