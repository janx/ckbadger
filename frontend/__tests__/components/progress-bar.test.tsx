import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { ProgressBar, UsageBar } from '@/components/ui/progress-bar';

describe('capacity bars', () => {
  it('shows the true over-budget percentage while clipping only bar geometry', () => {
    const { container } = render(<ProgressBar value={150} max={100} />);
    expect(screen.getByText('150 / 100 (150.0%)')).toBeInTheDocument();
    expect(container.querySelector('[style]')).toHaveStyle({ width: '100%' });
  });

  it('marks a zero denominator as an undefined ratio', () => {
    render(<UsageBar value={0} max={0} />);
    expect(screen.getByText('0 / 0 (—)')).toBeInTheDocument();
  });
});
