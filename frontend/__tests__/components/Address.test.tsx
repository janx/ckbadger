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
    // U-09 (re-pinned): a shortened address names its size; there is no default length. R9
    // (re-pinned): the head size keeps the network and the last 8 characters, as the row does
    it('shortens a long address at its size with the one ellipsis', () => {
      render(<Address address={testAddress} size="head" />);
      const link = screen.getByRole('link');
      expect(link.textContent).toBe('ckb1…d9wj5clj');
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

  // Lane c2-links: an address's look is where it stands, which its caller says.
  describe('its look', () => {
    const inks = (el: Element) =>
      [...el.classList].filter((c) => /^text-text|underline|decoration/.test(c));

    // Re-pinned with the coordinator's ruling: inline was "no ink of its own"; it is the line's
    // ink with the muted underline now.
    it('is a row’s key in the bright ink, or a name inside a line in its ink, underlined', () => {
      render(
        <>
          <Address address={testAddress} size="row" look="key" />
          <Address address={testAddress} size="row" look="inline" />
        </>
      );
      const [key, inline] = screen.getAllByRole('link');
      expect(inks(key)).toEqual(['text-text-bright']);
      expect(inks(inline)).toEqual([
        'underline',
        'decoration-1',
        'underline-offset-[3px]',
        'decoration-[color:var(--c-ink-muted)]',
      ]);
      // a look sets no size: the address keeps its own step and face
      expect(key).toHaveClass('text-body', 'font-mono');
    });

    it('keeps the look it had before for a caller that states none', () => {
      render(<Address address={testAddress} size="row" />);
      expect(inks(screen.getByRole('link'))).toEqual([
        'text-text-bright',
        'decoration-aqua/35',
        'underline',
        'underline-offset-[3px]',
      ]);
    });
  });
});
