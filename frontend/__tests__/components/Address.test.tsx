import { render, screen } from '@testing-library/react';
import { Address } from '@/components/ui/address';

describe('Address', () => {
  const testAddress =
    'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqt4z78ng4yutl5u6xsv27lt52eh9jvvtd9wj5clj';

  describe('rendering', () => {
    it('renders an address link', () => {
      render(<Address address={testAddress} size="row" />);
      const link = screen.getByRole('link');
      expect(link).toBeInTheDocument();
      expect(link).toHaveAttribute('href', `/mainnet/address/${testAddress}`);
    });

    it('sets title attribute for full address on hover', () => {
      render(<Address address={testAddress} size="row" />);
      const link = screen.getByRole('link');
      expect(link).toHaveAttribute('title', testAddress);
    });
  });

  describe('truncation', () => {
    // U-09 (re-pinned): a shortened address names its size; there is no default length
    it('shortens a long address at its size with the one ellipsis', () => {
      render(<Address address={testAddress} size="head" />);
      const link = screen.getByRole('link');
      expect(link.textContent).toBe('ckb1qzda0cr0…d9wj5clj');
      expect(link.textContent).not.toContain('...');
    });

    it('shows full address when truncate is false', () => {
      render(<Address address={testAddress} truncate={false} />);
      const link = screen.getByRole('link');
      expect(link.textContent).toBe(testAddress);
      expect(link.textContent).not.toContain('...');
    });

    // U-09 (re-pinned, the lead's ruling): a tight address is 4…6 (it was 8…6)
    it('writes the tight size, 4…6 for an address (no length is passed)', () => {
      render(<Address address={testAddress} size="tight" />);
      const link = screen.getByRole('link');
      expect(link.textContent).toBe('ckb1…wj5clj');
    });

    it('does not shorten an address no longer than its shortened form', () => {
      // 11 characters, the tight form's 4 + 1 + 6 (it was 14 against 8…6's 15)
      const shortAddress = 'ckb1qzda0cr';
      render(<Address address={shortAddress} size="tight" />);
      const link = screen.getByRole('link');
      expect(link.textContent).toBe(shortAddress);
    });
  });
});
