import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { HolderLabel } from '@/components/holder-label';

const LOCK_HASH = `0x${'ab'.repeat(32)}`;
const ADDRESS = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq';

describe('HolderLabel', () => {
  it('links a holder with an address to its address page, showing the address', () => {
    render(<HolderLabel holder={{ lockScriptHash: LOCK_HASH, address: ADDRESS, itemCount: 1 }} />);
    expect(screen.getByRole('link', { name: ADDRESS })).toHaveAttribute(
      'href',
      `/mainnet/address/${ADDRESS}`
    );
  });

  it('links a holder whose lock encodes to no address by its lock hash', () => {
    render(<HolderLabel holder={{ lockScriptHash: LOCK_HASH, address: null, itemCount: 1 }} />);
    expect(screen.getByRole('link')).toHaveAttribute('href', `/mainnet/address/${LOCK_HASH}`);
  });

  it('never invents a link for a holder with neither an address nor a lock hash', () => {
    render(
      <HolderLabel
        holder={{
          lockScriptHash: null,
          ownerHashPrefix: `0x${'cd'.repeat(20)}`,
          address: null,
          itemCount: 1,
        }}
      />
    );
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
    expect(screen.getByText('Unknown holder')).toBeInTheDocument();
  });
});
