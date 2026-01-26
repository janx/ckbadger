import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { Capacity } from '@/components/ui/capacity';

describe('Capacity', () => {
  it('renders capacity with unit by default', () => {
    render(<Capacity value="10000000000" />);
    expect(screen.getByText('100')).toBeInTheDocument();
    expect(screen.getByText('CKB')).toBeInTheDocument();
  });

  it('renders capacity without unit when showUnit is false', () => {
    render(<Capacity value="10000000000" showUnit={false} />);
    expect(screen.getByText('100')).toBeInTheDocument();
    expect(screen.queryByText('CKB')).not.toBeInTheDocument();
  });

  it('handles bigint values', () => {
    render(<Capacity value={BigInt('50000000000000')} />);
    expect(screen.getByText('500,000')).toBeInTheDocument();
  });

  it('formats decimal part correctly', () => {
    render(<Capacity value="12345678901" />);
    expect(screen.getByText('123')).toBeInTheDocument();
    expect(screen.getByText('.45678901')).toBeInTheDocument();
  });

  it('pads decimal part with zeros', () => {
    render(<Capacity value="10000000001" />);
    expect(screen.getByText('100')).toBeInTheDocument();
    expect(screen.getByText('.00000001')).toBeInTheDocument();
  });

  it('shows positive sign when showSign is true', () => {
    render(<Capacity value="10000000000" showSign />);
    expect(screen.getByText('+')).toBeInTheDocument();
    expect(screen.getByText('100')).toBeInTheDocument();
  });

  it('shows negative sign for negative values', () => {
    render(<Capacity value="-10000000000" />);
    expect(screen.getByText('-')).toBeInTheDocument();
    expect(screen.getByText('100')).toBeInTheDocument();
  });

  it('applies custom className', () => {
    render(<Capacity value="10000000000" className="custom-class" />);
    const outerSpan = screen.getByText('100').parentElement;
    expect(outerSpan).toHaveClass('custom-class');
  });

  it('handles zero value', () => {
    render(<Capacity value="0" />);
    expect(screen.getByText('0')).toBeInTheDocument();
    expect(screen.getByText('.00000000')).toBeInTheDocument();
  });

  it('handles large values with proper formatting', () => {
    render(<Capacity value="336000000000000000000" />);
    expect(screen.getByText(/3,360,000,000,000/)).toBeInTheDocument();
  });
});
