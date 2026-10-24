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
  /** What the capacity belongs to, e.g. `cluster 0x…` — named in any absence or error. */
  subject: string;
  capacityRange: CapacityRangeKey;
  onCapacityRangeChange: (range: CapacityRangeKey) => void;
  capacityChart: StackedAreaChartResponse | undefined;
  isCapacityChartLoading: boolean;
  /** Shannons; null when the API records none for the subject. */
  totalCapacity: string | null;
  /** Shannons; null when the API records none for the subject. */
  commonKnowledgeSize: string | null;
  totalCapacityLabel?: string;
  className?: string;
}

/**
 * The capacity bar, or — explicitly — why there is none: the API records no owned capacity for
 * the subject, or nothing live holds any (both amounts exactly 0). Any other value goes to the
 * bar, which rejects what it cannot draw.
 */
function CapacityUtilizationOrState({
  subject,
  totalCapacity,
  commonKnowledgeSize,
  totalCapacityLabel,
}: {
  subject: string;
  totalCapacity: string | null;
  commonKnowledgeSize: string | null;
  totalCapacityLabel: string;
}) {
  if (totalCapacity === null || commonKnowledgeSize === null) {
    return (
      <div
        data-testid="capacity-statistics-state"
        data-state="not-recorded"
        className="text-text-dim mb-3 font-mono text-xs"
      >
        {totalCapacityLabel} is not recorded for {subject}
      </div>
    );
  }
  if (totalCapacity === '0' && commonKnowledgeSize === '0') {
    return (
      <div
        data-testid="capacity-statistics-state"
        data-state="none-live"
        className="text-text-dim mb-3 font-mono text-xs"
      >
        {totalCapacityLabel}: no live cells hold capacity right now
      </div>
    );
  }
  return (
    <CapacityUtilization
      totalCapacity={totalCapacity}
      commonKnowledgeSize={commonKnowledgeSize}
      totalLabel={totalCapacityLabel}
      className="mb-3"
    />
  );
}

export function CapacityStatisticsSection({
  subject,
  capacityRange,
  onCapacityRangeChange,
  capacityChart,
  isCapacityChartLoading,
  totalCapacity,
  commonKnowledgeSize,
  totalCapacityLabel = 'Total Capacity',
  className,
}: CapacityStatisticsSectionProps) {
  return (
    <TerminalPanel className={className}>
      <TerminalPanelHeader
        indicator="none"
        actions={<CapacityRangeSelector value={capacityRange} onChange={onCapacityRangeChange} />}
      >
        Capacity Statistics
      </TerminalPanelHeader>
      <TerminalPanelContent>
        <CapacityUtilizationOrState
          subject={subject}
          totalCapacity={totalCapacity}
          commonKnowledgeSize={commonKnowledgeSize}
          totalCapacityLabel={totalCapacityLabel}
        />

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
