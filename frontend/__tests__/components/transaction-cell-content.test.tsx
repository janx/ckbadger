import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { TransactionCellContent } from '@/components/transaction/transaction-cell-content';
import type { TransactionCellDisplay } from '@/lib/api';

const object: TransactionCellDisplay = {
  kind: 'object',
  standard: 'spore',
  id: `0x${'a'.repeat(64)}`,
  name: null,
  collectionId: null,
  collectionName: null,
  symbol: null,
  decimals: null,
  amount: null,
  compositionTier: null,
};

describe('transaction cell semantics', () => {
  it('keeps the canonical object id reachable when optional names and media are unavailable', () => {
    render(<TransactionCellContent display={object} cellHref="/cell/0xcell-0" />);
    expect(screen.getByText('Spore')).toBeInTheDocument();
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', `/mainnet/objects/${object.id}`);
    expect(link.querySelector('[title]')).toHaveAttribute('title', object.id);
    expect(screen.queryByLabelText(/Media composition/)).not.toBeInTheDocument();
  });

  it('uses the verified BTC+CKB copper badge and names pending decode explicitly', () => {
    const { rerender } = render(
      <TransactionCellContent
        display={{ ...object, compositionTier: 'btc_ckb' }}
        cellHref="/cell/x-0"
      />
    );
    const badge = screen.getByLabelText('Media composition: BTC+CKB');
    expect(badge).toHaveTextContent('BTC+CKB');
    expect(badge).toHaveStyle({ color: '#ffe1cc' });
    rerender(
      <TransactionCellContent
        display={{ ...object, compositionTier: 'decode_pending' }}
        cellHref="/cell/x-0"
      />
    );
    expect(screen.getByLabelText('Media composition: Decode pending')).toHaveAttribute(
      'title',
      'Awaiting DOB execution; media composition has not been classified.'
    );
    expect(screen.queryByText('Pure CKB')).not.toBeInTheDocument();
  });

  it('marks unknown token decimals as raw and never invents a missing amount', () => {
    const token: TransactionCellDisplay = {
      ...object,
      kind: 'token',
      standard: 'xudt',
      amount: '9007199254740993123',
      symbol: 'USDI',
    };
    const { rerender } = render(<TransactionCellContent display={token} cellHref="/cell/x-0" />);
    expect(screen.getByText(/9,007,199,254,740,993,123 \(raw\) USDI/)).toBeInTheDocument();
    rerender(<TransactionCellContent display={{ ...token, amount: null }} cellHref="/cell/x-0" />);
    expect(screen.getByText(/Amount unavailable/)).toBeInTheDocument();
    expect(screen.queryByText(/0 USDI/)).not.toBeInTheDocument();
  });

  it('links mNFT item identifiers to the mNFT item route', () => {
    render(
      <TransactionCellContent display={{ ...object, standard: 'm-nft' }} cellHref="/cell/x-0" />
    );
    expect(screen.getByRole('link')).toHaveAttribute('href', `/mainnet/objects/mnft/${object.id}`);
  });
});
