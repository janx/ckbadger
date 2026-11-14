import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { HMultiplierReading } from '@/components/ui/h-multiplier';

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
