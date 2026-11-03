// Lane L (L1): every input and output tile of a transaction names its cell by its out point, and
// the out point is the link to the cell page (the position label "input 0" / "output 0" stays,
// as text; the owner and the content keep their own links). The hover title is the full out point.
import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@/__tests__/utils/test-utils';
import { CellTile } from '@/components/ui/cell-tile';
import { cellTiles } from '@/components/transaction/transaction-cell-tiles';
import type { TransactionDetail } from '@/lib/api';

const PREV = `0x9486132e${'00'.repeat(26)}87d667`;
const HASH = `0xdcd14a81${'11'.repeat(26)}1a334d`;
const ADDRESS = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqwyt2p74pg74c';
const cell = {
  capacity: '40000000000',
  commonKnowledgeSize: '97',
  display: null,
  address: ADDRESS,
  lock: { codeHash: `0x${'9b'.repeat(32)}`, hashType: 'type', args: '0x' },
};
const tx = {
  hash: HASH,
  isCellbase: false,
  inputs: [{ ...cell, previousOutput: { txHash: PREV, index: 3 } }],
  outputs: [{ ...cell, status: 'live', isChange: false }],
} as unknown as TransactionDetail;

describe('transaction cell tiles name their cell by its out point (L1)', () => {
  it('links an input tile to the cell it spends, by that cell’s out point', () => {
    const { inputs } = cellTiles(tx, undefined);
    render(<CellTile cell={inputs[0]} />);
    const tile = screen.getByTestId('tx-io-input-0');
    const link = within(tile).getByRole('link', { name: '0x9486...d667:3' });
    expect(link).toHaveAttribute('href', `/mainnet/cell/${PREV}-3`);
    expect(link).toHaveAttribute('title', `${PREV}:3`);
    // the position stays, as text: one link to the cell, not two
    expect(tile).toHaveTextContent(/^input 0 0x9486\.\.\.d667:3/);
    expect(within(tile).queryByRole('link', { name: 'input 0' })).toBeNull();
    // the owner keeps its own link
    expect(within(tile).getByRole('link', { name: /^ckb1qzda0c/ })).toHaveAttribute(
      'href',
      `/mainnet/address/${ADDRESS}`
    );
  });

  it('links an output tile to its own cell, by this transaction’s out point', () => {
    const { outputs } = cellTiles(tx, undefined);
    render(<CellTile cell={outputs[0]} />);
    const tile = screen.getByTestId('tx-io-output-0');
    const link = within(tile).getByRole('link', { name: '0xdcd1...334d:0' });
    expect(link).toHaveAttribute('href', `/mainnet/cell/${HASH}-0`);
    expect(link).toHaveAttribute('title', `${HASH}:0`);
    expect(tile).toHaveTextContent(/^output 0 0xdcd1\.\.\.334d:0/);
  });

  it('leaves a click on a link inside the tile to the link: the tile’s own click does not fire', () => {
    // Review 2026-11-03: on the tx page a hovered tile is lit, which armed its clear-focus click;
    // that click replaced the URL right after the out point link had navigated, so the cell page
    // never opened. A click inside a link is the link's; the tile's click is for the tile itself.
    const onClick = vi.fn();
    const { outputs } = cellTiles(tx, undefined);
    render(<CellTile cell={outputs[0]} onClick={onClick} />);
    const tile = screen.getByTestId('tx-io-output-0');
    // outside a router the anchors would ask jsdom to navigate, which it cannot
    tile.addEventListener('click', (event) => event.preventDefault());
    fireEvent.click(within(tile).getByRole('link', { name: '0xdcd1...334d:0' }));
    fireEvent.click(within(tile).getByRole('link', { name: /^ckb1qzda0c/ }));
    expect(onClick).not.toHaveBeenCalled();
    fireEvent.click(tile);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('keeps the label as the link for a tile without an out point (old default)', () => {
    render(
      <CellTile
        cell={{
          id: 'in:0',
          testId: 'tile',
          href: '/cell/0x-0',
          label: 'input 0',
          owner: 'owner',
          lockKind: null,
          kind: 'ckb',
          content: 'CKB',
          capacity: '6100000000',
          occupied: '6100000000',
          state: 'consumed',
        }}
      />
    );
    expect(screen.getByRole('link', { name: 'input 0' })).toHaveAttribute(
      'href',
      '/mainnet/cell/0x-0'
    );
  });
});
