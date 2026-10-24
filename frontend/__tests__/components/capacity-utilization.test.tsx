import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { CapacityUtilization } from '@/components/ui/capacity-utilization';

function renderBar(totalCapacity: string, commonKnowledgeSize: string) {
  return render(
    <CapacityUtilization
      subject="spore 0xabc"
      totalCapacity={totalCapacity}
      commonKnowledgeSize={commonKnowledgeSize}
      totalLabel="Owned Capacity"
    />
  );
}

function expectError(message: string) {
  const alert = screen.getByRole('alert');
  expect(alert).toHaveAttribute('data-testid', 'capacity-utilization-error');
  expect(alert).toHaveTextContent(message);
  expect(screen.queryByText(/Common Knowledge:/)).toBeNull();
}

describe('CapacityUtilization', () => {
  it('draws the split with an exact, half-up share', () => {
    // 12.45% exactly: a float rounds it down to 12.4.
    renderBar('10000000000', '1245000000');
    expect(screen.getByText('Owned Capacity')).toBeInTheDocument();
    expect(screen.getByText(/Common Knowledge: 12\.45 CKB/)).toBeInTheDocument();
    expect(screen.getByText('(12.5% share)')).toBeInTheDocument();
    expect(screen.getByText(/Free Capacity: 87\.55 CKB/)).toBeInTheDocument();
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('draws a full bar when common knowledge equals the total', () => {
    renderBar('6100000000', '6100000000');
    expect(screen.getByText('(100.0% share)')).toBeInTheDocument();
  });

  it('rejects a total it cannot parse', () => {
    renderBar('abc', '0');
    expectError(
      'Owned Capacity of spore 0xabc: totalCapacity "abc" is not an integer shannon amount'
    );
  });

  it('rejects a common knowledge size it cannot parse', () => {
    renderBar('100', '1.5');
    expectError(
      'Owned Capacity of spore 0xabc: commonKnowledgeSize "1.5" is not an integer shannon amount'
    );
  });

  it('rejects a total that is not positive instead of drawing nothing', () => {
    renderBar('0', '0');
    expectError('Owned Capacity of spore 0xabc: total capacity 0 is not positive');
  });

  it('rejects common knowledge above the total instead of clamping it', () => {
    renderBar('100', '200');
    expectError('Owned Capacity of spore 0xabc: common knowledge 200 exceeds total capacity 100');
  });

  it('rejects negative common knowledge instead of clamping it', () => {
    renderBar('100', '-1');
    expectError('Owned Capacity of spore 0xabc: common knowledge -1 is negative');
  });
});
