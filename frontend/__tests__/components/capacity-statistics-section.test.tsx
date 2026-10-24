import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { CapacityStatisticsSection } from '@/components/ui/capacity-statistics-section';

function renderSection(totalCapacity: string | null, commonKnowledgeSize: string | null) {
  return render(
    <CapacityStatisticsSection
      subject="cluster 0xabc"
      capacityRange="all"
      onCapacityRangeChange={vi.fn()}
      capacityChart={undefined}
      isCapacityChartLoading={false}
      totalCapacity={totalCapacity}
      commonKnowledgeSize={commonKnowledgeSize}
      totalCapacityLabel="Owned Capacity"
    />
  );
}

describe('CapacityStatisticsSection', () => {
  it('draws the bar for an owned capacity', () => {
    renderSection('100000000000', '61000000000');
    expect(screen.getByText('Owned Capacity')).toBeInTheDocument();
    expect(screen.queryByTestId('capacity-statistics-state')).toBeNull();
  });

  it('says so when nothing live holds capacity, instead of hiding the bar', () => {
    renderSection('0', '0');
    const state = screen.getByTestId('capacity-statistics-state');
    expect(state).toHaveAttribute('data-state', 'none-live');
    expect(state).toHaveTextContent('Owned Capacity: no live cells hold capacity right now');
  });

  it('says the owned capacity is not recorded when the API sends none', () => {
    renderSection(null, null);
    const state = screen.getByTestId('capacity-statistics-state');
    expect(state).toHaveAttribute('data-state', 'not-recorded');
    expect(state).toHaveTextContent('Owned Capacity is not recorded for cluster 0xabc');
  });
});
