import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { ItemTransactionContext } from '@/components/asset/item-evidence';
import { itemTransactionsProducer } from '@/__tests__/msw/fixtures/item-producer';
import type { ItemTransaction } from '@/lib/api';

const base = itemTransactionsProducer.data[0] as ItemTransaction;
const tx = (context: Partial<ItemTransaction['itemContext']>): ItemTransaction => ({
  ...base,
  itemContext: { ...base.itemContext, ...context },
});

describe('ItemTransactionContext · Transactions tab action names (ID-C4)', () => {
  it('names a .bit row by its protocol action, not the coarse item action', () => {
    render(
      <ItemTransactionContext
        transaction={tx({ action: 'update', protocolActions: ['dotbit:edit_records'] })}
        item={{ kind: 'identity', protocol: 'dotbit' }}
      />
    );
    expect(screen.getByText('Records edited')).toBeInTheDocument();
    expect(screen.queryByText('update')).toBeNull();
  });

  it('dims a row the API says a neighbour acted on', () => {
    render(
      <ItemTransactionContext
        transaction={tx({ actedOn: 'neighbour', ringRelink: { by: 'register', byId: '0xcc' } })}
        item={{ kind: 'identity', protocolName: '.cell' }}
      />
    );
    expect(screen.getByText('Neighbour registered').closest('[data-acted-on]')).toHaveAttribute(
      'data-acted-on',
      'neighbour'
    );
  });
});
