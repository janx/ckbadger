import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { HMultiplier, HMultiplierReading } from '@/components/ui/h-multiplier';
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

describe('HMultiplierReading (consistency plan C12)', () => {
  it('reads "H-multiplier ×N" with its definition wherever it is shown', () => {
    const { container } = render(<HMultiplierReading held={BigInt(102)} occupied={BigInt(100)} />);
    const reading = container.querySelector('[data-role="h-multiplier-reading"]');
    expect(reading).toHaveTextContent(/^H-multiplier ×1\.02: CKB held ÷ occupied$/);
    // The full sentence of the term rides along for a hover.
    expect(reading).toHaveAttribute(
      'title',
      expect.stringContaining('H-multiplier = CKB held in its cells ÷ occupied.')
    );
    // The term and its value stay one unbroken unit (B#18).
    expect(screen.getByText('H-multiplier ×1.02')).toHaveAttribute('data-role', 'h-multiplier');
  });
});
