import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { Capacity } from '@/components/ui/capacity';

describe('Capacity', () => {
  it('renders formatted capacity with the unit by default and can hide it', () => {
    const { rerender } = render(<Capacity value="10000000000" />);

    expect(screen.getByText('100')).toBeInTheDocument();
    expect(screen.getByText('CKB')).toBeInTheDocument();

    rerender(<Capacity value="10000000000" showUnit={false} />);
    expect(screen.getByText('100')).toBeInTheDocument();
    expect(screen.queryByText('CKB')).not.toBeInTheDocument();
  });

  it('accepts bigint values and can show a positive sign', () => {
    render(<Capacity value={BigInt('50000000000000')} showSign />);
    expect(screen.getByText('500,000')).toBeInTheDocument();
    expect(screen.getByText('+')).toBeInTheDocument();
  });

  it('shows negative sign for negative values', () => {
    render(<Capacity value="-10000000000" />);
    expect(screen.getByText('-')).toBeInTheDocument();
    expect(screen.getByText('100')).toBeInTheDocument();
  });
});
