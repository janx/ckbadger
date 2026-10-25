import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { HMultiplier } from '@/components/ui/h-multiplier';
import { computeHMulValue, formatHMulValue } from '@/components/ui/hmul-info';

describe('HMul exact display', () => {
  it('formats a large ratio with integer arithmetic', () => {
    expect(computeHMulValue('9007199254740993', '3')).toBe('3002399751580331.00');
    expect(formatHMulValue(computeHMulValue('9007199254740993', '3'))).toBe('×3002399751580331.00');
    render(<HMultiplier totalCapacity="9007199254740993" commonKnowledgeSize="3" />);
    expect(screen.getByText('HMul: 3002399751580331.00x')).toBeInTheDocument();
  });

  it('distinguishes absent and invalid required capacity', () => {
    expect(computeHMulValue(null, null)).toBeNull();
    expect(() => computeHMulValue('bad', '1')).toThrow('totalCapacity');
  });
});
