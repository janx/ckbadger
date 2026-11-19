// The fold (lane D3; the transaction board the owner passed at gate D0′, R10): a transaction with
// 158 inputs drew 158 tiles, 30,316 px of page at 1280, its deposit 25 screens down on a phone
// (U-07, PA-21). A sheet draws its first six tiles in the transaction's own order and folds the
// rest into one row that says what it folds, by counting, and opens in place. It never sums: the
// sheets' totals stay the API's. A cell a page keeps (the URL's out point, a cell page's own cell)
// and the continuity partner of a cell it draws are never folded.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import {
  FOLD_HEAD,
  TxStructure,
  type TxContinuity,
  type TxFold,
} from '@/components/transaction/tx-structure';
import type { CellTileData } from '@/components/ui/cell-tile';
import { sealedTile } from '@/__tests__/utils/cell-tile-seal';
import { sealHash } from '@/__tests__/utils/seals';
import { seamLines } from '@/__tests__/utils/tx-lines';

const TX = sealHash('fold');
const OWNER = sealHash('fold owner');

/** A tile; `lock` names its owner's lock (each tile its own lock unless one is given). */
function tile(id: string, lock?: string, owner = `owner of ${id}`): CellTileData {
  const made = sealedTile({
    id,
    testId: id,
    href: `/cell/${id}`,
    label: id,
    owner,
    lockKind: null,
    kind: 'ckb',
    content: 'CKB',
    capacity: '6100000000',
    occupiedBytes: '61',
    state: id.startsWith('in') ? 'consumed' : 'live',
  });
  return lock ? { ...made, seal: { ...made.seal, id: { ...made.seal.id, lock } } } : made;
}
const cells = (side: 'in' | 'out', n: number, lock?: string, owner?: string) =>
  Array.from({ length: n }, (_, i) => tile(`${side}:${i}`, lock, owner));

const closed = { consumed: false, created: false };

function structure({
  inputs = cells('in', 158, OWNER, 'ckb1…sqylvfrs'),
  outputs = cells('out', 2),
  fold,
  continuity,
}: {
  inputs?: CellTileData[];
  outputs?: CellTileData[];
  fold?: TxFold;
  continuity?: TxContinuity[];
}) {
  return render(
    <TxStructure
      txHash={TX}
      inputs={inputs}
      outputs={outputs}
      fee="1572400"
      consumedCapacity="1220006100157240"
      createdCapacity="1220006100000000"
      continuity={continuity}
      continuityList={false}
      fold={fold}
    />
  );
}

/** A sheet's drawn tiles, by id, and its fold row. */
function sheet(container: HTMLElement, side: 'consumed' | 'created') {
  const el = container.querySelector<HTMLElement>(`[data-sheet="${side}"]`)!;
  return {
    el,
    ids: Array.from(el.querySelectorAll<HTMLElement>('[data-cell-id]')).map(
      (t) => t.dataset.cellId
    ),
    fold: el.querySelector<HTMLElement>(`[data-fold="${side}"]`),
  };
}

describe('TxStructure: a sheet folds after six tiles (lane D3)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('draws the first six cells of a sheet in the transaction’s order and one row for the rest', () => {
    expect(FOLD_HEAD).toBe(6);
    const onToggle = vi.fn();
    const { container } = structure({
      fold: { head: FOLD_HEAD, keep: [], open: closed, onToggle },
    });
    const consumed = sheet(container, 'consumed');
    expect(consumed.ids).toEqual(['in:0', 'in:1', 'in:2', 'in:3', 'in:4', 'in:5']);
    // one row after the six, dashed as the fee row is; it says what it folds, by counting
    const rows = Array.from(
      consumed.el.querySelector<HTMLElement>('[data-sheet-rows]')!.children
    ) as HTMLElement[];
    expect(rows.at(-1)).toBe(consumed.fold);
    expect(consumed.fold).toHaveClass('border', 'border-dashed');
    // every folded cell has the one lock: the row names its owner
    expect(consumed.fold!.querySelector('[data-fold-words]')).toHaveTextContent(
      /^152 more cells from ckb1…sqylvfrs$/
    );
    // "show all" is the short list's form, the Pager's words: the whole list's count
    const show = within(consumed.fold!).getByRole('button', { name: /^all 158\scells ↓$/ });
    fireEvent.click(show);
    expect(onToggle).toHaveBeenCalledWith('consumed', true);
    // the created sheet holds two: nothing folds, no row
    const created = sheet(container, 'created');
    expect(created.ids).toEqual(['out:0', 'out:1']);
    expect(created.fold).toBeNull();
  });

  it('keeps the heads’ counts and the API’s totals, never a sum of what it draws', () => {
    const { container } = structure({
      fold: { head: FOLD_HEAD, keep: [], open: closed, onToggle: () => {} },
    });
    expect(screen.getByRole('heading', { name: '158 cells consumed' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: '2 cells created' })).toBeInTheDocument();
    const totals = Array.from(container.querySelectorAll('[data-sheet-total]')).map(
      (t) => t.textContent
    );
    expect(totals).toHaveLength(2);
    expect(totals[0]).toMatch(/^12,200,061\.0015724(0)? CKB$/);
    expect(totals[1]).toMatch(/^12,200,061(\.0+)? CKB$/);
  });

  it('counts alone when the folded cells do not share one lock', () => {
    const inputs = [...cells('in', 157, OWNER, 'ckb1…sqylvfrs'), tile('in:157')];
    const { container } = structure({
      inputs,
      fold: { head: FOLD_HEAD, keep: [], open: closed, onToggle: () => {} },
    });
    expect(sheet(container, 'consumed').fold!.querySelector('[data-fold-words]')).toHaveTextContent(
      /^152 more cells consumed$/
    );
  });

  // An owner is the same when the lock is the same, not when the shortened address reads the same.
  it('tells owners apart by their lock, not by the words that name them', () => {
    const inputs = cells('in', 10).map((t) => ({ ...t, owner: 'ckb1…sqylvfrs' }));
    const { container } = structure({
      inputs,
      fold: { head: FOLD_HEAD, keep: [], open: closed, onToggle: () => {} },
    });
    expect(sheet(container, 'consumed').fold!.querySelector('[data-fold-words]')).toHaveTextContent(
      /^4 more cells consumed$/
    );
  });

  it('opens a sheet in place and folds it again', () => {
    const onToggle = vi.fn();
    const { container } = structure({
      fold: { head: FOLD_HEAD, keep: [], open: { consumed: true, created: false }, onToggle },
    });
    const consumed = sheet(container, 'consumed');
    expect(consumed.ids).toHaveLength(158);
    expect(consumed.ids.at(-1)).toBe('in:157');
    // the way back stands where the rows end, in the rows' fold's words
    const fewer = within(consumed.fold!).getByRole('button', { name: 'fewer ↑' });
    expect(fewer).toHaveAttribute('aria-expanded', 'true');
    expect(consumed.fold!.querySelector('[data-fold-words]')).toBeNull();
    fireEvent.click(fewer);
    expect(onToggle).toHaveBeenCalledWith('consumed', false);
  });

  it('never folds a cell the page keeps, nor the continuity partner of a cell it draws', () => {
    const outputs = cells('out', 10);
    const { container } = structure({
      outputs,
      continuity: [
        // a drawn input's partner past the created sheet's six
        { from: 'in:2', to: 'out:8', label: 'same type id continues', kind: 'type-id' },
        // a kept input's partner
        { from: 'in:57', to: 'out:9', label: 'same type id continues', kind: 'type-id' },
        // two folded cells: neither is drawn
        { from: 'in:100', to: 'out:7', label: 'same type id continues', kind: 'type-id' },
      ],
      fold: { head: FOLD_HEAD, keep: ['in:57'], open: closed, onToggle: () => {} },
    });
    const consumed = sheet(container, 'consumed');
    expect(consumed.ids).toEqual(['in:0', 'in:1', 'in:2', 'in:3', 'in:4', 'in:5', 'in:57']);
    expect(consumed.fold!.querySelector('[data-fold-words]')).toHaveTextContent(
      /^151 more cells from ckb1…sqylvfrs$/
    );
    const created = sheet(container, 'created');
    expect(created.ids).toEqual([
      'out:0',
      'out:1',
      'out:2',
      'out:3',
      'out:4',
      'out:5',
      'out:8',
      'out:9',
    ]);
    expect(created.fold!.querySelector('[data-fold-words]')).toHaveTextContent(
      /^2 more cells created$/
    );
    // a link is drawn only between two drawn tiles
    const kinds = seamLines(container).map((p) => p.getAttribute('d') ?? '');
    expect(kinds).toHaveLength(2);
  });

  it('refuses to keep a cell the transaction does not have', () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      structure({ fold: { head: FOLD_HEAD, keep: ['in:158'], open: closed, onToggle: () => {} } })
    ).toThrow(`${TX}: the fold keeps cell in:158, which the transaction does not have`);
    quiet.mockRestore();
  });

  // The cell page (the board's embed): this cell's tile and its continuity partner; the rest is a
  // count, and the way to it is the link to the transaction, so the rows hold no control.
  // Re-pinned on purpose (lane P1, lane D3's note): a sheet that draws none of its cells counts
  // them, and no longer adds ", none of them this cell", which said what the reader knows.
  it('draws a cell page’s own cell alone, each sheet’s rest a count with no control', () => {
    const { container } = structure({
      fold: { head: 0, keep: ['out:0'], open: closed },
    });
    const consumed = sheet(container, 'consumed');
    expect(consumed.ids).toEqual([]);
    expect(consumed.fold!.querySelector('[data-fold-words]')).toHaveTextContent(
      /^158 cells from ckb1…sqylvfrs$/
    );
    expect(within(consumed.fold!).queryByRole('button')).toBeNull();
    const created = sheet(container, 'created');
    expect(created.ids).toEqual(['out:0']);
    expect(created.fold!.querySelector('[data-fold-words]')).toHaveTextContent(
      /^1 more cell created$/
    );
    expect(within(created.fold!).queryByRole('button')).toBeNull();
  });

  // Re-pinned on purpose, and renamed (lane P1, lane D3's note; it was 'says "not this cell" of a
  // single cell it does not draw'): one cell a sheet does not draw is counted, "1 cell consumed"
  it('counts a single cell it does not draw', () => {
    const { container } = structure({
      inputs: [tile('in:0', OWNER, 'ckb1…sqylvfrs')],
      outputs: cells('out', 2),
      fold: { head: 0, keep: ['out:1'], open: closed },
    });
    expect(sheet(container, 'consumed').fold!.querySelector('[data-fold-words]')).toHaveTextContent(
      /^1 cell consumed$/
    );
  });

  it('draws every cell where the page gives no fold', () => {
    const { container } = structure({});
    expect(sheet(container, 'consumed').ids).toHaveLength(158);
    expect(container.querySelector('[data-fold]')).toBeNull();
  });
});
