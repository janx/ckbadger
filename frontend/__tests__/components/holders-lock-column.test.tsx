// Readability lane B4, the lead's ruling on LockKind (second round): a lock beside a party is
// silent for the default lock (R9), but a table column headed "Lock" never shows an empty cell.
// In the two holders tables the default lock is written by its name in the quiet ink, the words
// the tag used, without the tag's box; every other lock keeps its tag.
import { seededAddress } from '@/__tests__/utils/ckb-address';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { atContainerWidth } from '@/__tests__/utils/phone-row';
import { CollectionHolders } from '@/components/asset/collection-holders';
import { TokenHolders } from '@/components/asset/token-holders';
import { api, type CollectionHolder } from '@/lib/api';
import { DEFAULT_LOCK_CODE_HASH } from '@/lib/lock-script';
import { fontPx, textPx } from '@/lib/type-metrics';

const JOYID = '0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323';

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

/** Each body row's cell under the column headed "Lock". */
function lockCells(table: HTMLElement): HTMLElement[] {
  const heads = within(table).getAllByRole('columnheader');
  const at = heads.findIndex((h) => h.textContent === 'Lock');
  expect(at).toBeGreaterThanOrEqual(0);
  return within(table)
    .getAllByRole('row')
    .slice(1)
    .map((r) => within(r).getAllByRole('cell')[at]);
}

/** The default lock in its column: its name, the quiet ink, no box; another lock keeps its tag. */
function expectNamed(defaultCell: HTMLElement, otherCell: HTMLElement) {
  const plain = defaultCell.querySelector('[data-lock-kind]') as HTMLElement;
  expect(plain).toHaveTextContent(/^Default Lock$/);
  expect(plain).toHaveClass('text-text-dim');
  expect(plain).not.toHaveClass('border');
  expect(plain).toHaveAttribute('title', DEFAULT_LOCK_CODE_HASH);
  const tag = otherCell.querySelector('[data-lock-kind]') as HTMLElement;
  expect(tag).toHaveTextContent(/^JoyID$/);
  expect(tag).toHaveClass('border');
}

describe('a "Lock" column never shows an empty cell (B4, the lead’s ruling)', () => {
  it('keeps the column’s budget: the default lock’s name is narrower than the tag it is budgeted for', () => {
    // the column holds a tag of 12 characters at the meta step with its 5 px sides and 1 px border
    // (98.4 px); "Default Lock" is 12 characters with no box: 86.4 px
    expect(textPx('Default Lock'.length, fontPx('meta'))).toBeCloseTo(86.4, 10);
    expect(textPx(12, fontPx('meta')) + 2 * 5 + 2 * 1).toBeCloseTo(98.4, 10);
  });

  it('in the token holders: the default lock by its name, another lock by its tag', async () => {
    atContainerWidth(1248);
    vi.spyOn(api, 'getTokenHolders').mockResolvedValue({
      data: [
        {
          lockScriptHash: `0x${'11'.repeat(32)}`,
          // B4 fix round: real addresses, the lock read off them
          address: seededAddress('x7kq8p'),
          balance: '300',
          lockCodeHash: DEFAULT_LOCK_CODE_HASH,
          lockKind: 'Default Lock',
        },
        {
          lockScriptHash: `0x${'22'.repeat(32)}`,
          address: seededAddress('tllakf', JOYID),
          balance: '200',
          lockCodeHash: JOYID,
          lockKind: 'JoyID',
        },
      ],
      total: 2,
      limit: 10,
      hasMore: false,
      nextCursor: null,
      totalSupply: '500',
      anchor: null,
    });
    vi.spyOn(api, 'getTokenDistribution').mockRejectedValue(new Error('not needed'));
    render(<TokenHolders typeHash="0xabc" symbol="T" decimals={0} />);
    const table = await screen.findByRole('table', { name: 'Top 10 holders' });
    const cells = lockCells(table);
    for (const cell of cells) expect(cell.textContent?.trim()).not.toBe('');
    expectNamed(cells[0], cells[1]);
  });

  it('in the collection holders: the default lock by its name, another lock by its tag', async () => {
    atContainerWidth(1248);
    const holder = (n: string, lockCodeHash: string, lockKind: string): CollectionHolder => ({
      lockScriptHash: `0x${n.repeat(32)}`,
      address: null,
      itemCount: 2,
      lockCodeHash,
      lockKind,
    });
    const load = vi.fn().mockResolvedValue({
      // B4 fix round: the default lock is the pair, read off the holder's address
      data: [
        {
          ...holder('11', DEFAULT_LOCK_CODE_HASH, 'Default Lock'),
          address: seededAddress('x7kq8p'),
        },
        holder('22', JOYID, 'JoyID'),
      ],
      limit: 10,
      total: 2,
      hasMore: false,
      nextCursor: null,
    });
    render(<CollectionHolders assetClass="objects" load={load} queryKey="lock-column" />);
    const table = await screen.findByRole('table', { name: 'Top 10 collection holders' });
    const cells = lockCells(table);
    for (const cell of cells) expect(cell.textContent?.trim()).not.toBe('');
    expectNamed(cells[0], cells[1]);
  });
});
