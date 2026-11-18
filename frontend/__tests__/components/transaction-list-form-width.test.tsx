import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from '@/__tests__/utils/test-utils';
import {
  TRANSACTION_LIST_MIN_PX,
  TransactionListTable,
} from '@/components/transaction/transaction-list-table';

/**
 * Review F-B (test finding 9): the committed list changes form at one width, at least the sum of
 * its own columns (`TRANSACTION_LIST_MIN_PX`); a page that draws it beside another list gives the
 * larger of their sums, and What it did keeps the difference. A width under its own sum would
 * leave What it did less than it needs, so the list refuses it, naming the list and both widths.
 */
describe('TransactionListTable refuses to change form under its own sum', () => {
  afterEach(() => vi.restoreAllMocks());

  it('refuses a form width a pixel under its sum, and one that is not a number', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    const under = TRANSACTION_LIST_MIN_PX - 1;
    expect(() => render(<TransactionListTable transactions={[]} formWidth={under} />)).toThrow(
      `Committed transactions: cannot change form at ${under} px, under its ${TRANSACTION_LIST_MIN_PX}`
    );
    expect(() =>
      render(<TransactionListTable transactions={[]} label="Pool" formWidth={Number.NaN} />)
    ).toThrow(`Pool: cannot change form at NaN px, under its ${TRANSACTION_LIST_MIN_PX}`);
  });

  it('takes its own sum and anything wider', () => {
    for (const formWidth of [TRANSACTION_LIST_MIN_PX, TRANSACTION_LIST_MIN_PX + 17]) {
      let unmount = () => {};
      expect(() => {
        unmount = render(<TransactionListTable transactions={[]} formWidth={formWidth} />).unmount;
      }, `${formWidth} px`).not.toThrow();
      unmount();
    }
  });
});
