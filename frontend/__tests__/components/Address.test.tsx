import { render, screen } from '@testing-library/react';
import { Address } from '@/components/ui/address';

describe('Address', () => {
  const testAddress =
    'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqt4z78ng4yutl5u6xsv27lt52eh9jvvtd9wj5clj';

  describe('rendering', () => {
    it('renders an address link', () => {
      render(<Address address={testAddress} />);
      const link = screen.getByRole('link');
      expect(link).toBeInTheDocument();
      expect(link).toHaveAttribute('href', `/address/${testAddress}`);
    });

    it('sets title attribute for full address on hover', () => {
      render(<Address address={testAddress} />);
      const link = screen.getByRole('link');
      expect(link).toHaveAttribute('title', testAddress);
    });
  });

  describe('truncation', () => {
    it('truncates long addresses by default', () => {
      render(<Address address={testAddress} />);
      const link = screen.getByRole('link');
      expect(link.textContent).toContain('...');
      expect(link.textContent?.length).toBeLessThan(testAddress.length);
    });

    it('shows full address when truncate is false', () => {
      render(<Address address={testAddress} truncate={false} />);
      const link = screen.getByRole('link');
      expect(link.textContent).toBe(testAddress);
      expect(link.textContent).not.toContain('...');
    });

    it('uses custom startChars and endChars', () => {
      render(<Address address={testAddress} startChars={8} endChars={6} />);
      const link = screen.getByRole('link');
      const expected = `${testAddress.slice(0, 8)}...${testAddress.slice(-6)}`;
      expect(link.textContent).toBe(expected);
    });

    it('does not truncate short addresses', () => {
      const shortAddress = 'ckb1qzda0cr08m';
      render(<Address address={shortAddress} startChars={20} endChars={10} />);
      const link = screen.getByRole('link');
      expect(link.textContent).toBe(shortAddress);
    });
  });

  describe('styling', () => {
    it('applies default classes', () => {
      render(<Address address={testAddress} />);
      const link = screen.getByRole('link');
      expect(link).toHaveClass('text-sky');
      expect(link).toHaveClass('font-mono');
      expect(link).toHaveClass('text-sm');
    });

    it('merges custom className', () => {
      render(<Address address={testAddress} className="my-custom-class" />);
      const link = screen.getByRole('link');
      expect(link).toHaveClass('my-custom-class');
      expect(link).toHaveClass('text-sky');
    });

    it('enables line wrapping when truncate is false', () => {
      render(<Address address={testAddress} truncate={false} />);
      const link = screen.getByRole('link');
      expect(link).toHaveClass('break-all');
      expect(link).toHaveClass('max-w-full');
    });
  });
});
