import { describe, it, expect, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { TxStructure } from '@/components/transaction/tx-structure';
import type { CellTileData } from '@/components/ui/cell-tile';

const tile = (id: string, capacity: string): CellTileData => ({
  id,
  testId: id,
  href: `/cell/${id}`,
  label: id,
  owner: 'owner',
  lockKind: null,
  kind: 'ckb',
  content: 'CKB',
  capacity,
  occupied: '6100000000',
  state: id.startsWith('in') ? 'consumed' : 'live',
});

describe('TxStructure page-owned linking (F2-tx, TX-14)', () => {
  it('lights only the cells the page links, never the continuity partner on its own', () => {
    const onCellHover = vi.fn();
    const { rerender } = render(
      <TxStructure
        txHash="0xtx"
        inputs={[tile('in:0', '20000000000'), tile('in:1', '10000000000')]}
        outputs={[tile('out:0', '29999990000')]}
        fee="10000"
        continuity={[
          { from: 'in:0', to: 'out:0', label: 'same file cell continues', kind: 'file' },
        ]}
        linking="page"
        focusedCellIds={[]}
        onCellHover={onCellHover}
      />
    );
    fireEvent.mouseEnter(screen.getByTestId('in:0'));
    expect(onCellHover).toHaveBeenLastCalledWith('in:0');
    // The page has not linked anything yet: nothing lights, nothing dims.
    expect(screen.getByTestId('out:0')).toHaveAttribute('data-highlighted', 'false');
    expect(screen.getByTestId('in:1')).not.toHaveClass('opacity-30');
    rerender(
      <TxStructure
        txHash="0xtx"
        inputs={[tile('in:0', '20000000000'), tile('in:1', '10000000000')]}
        outputs={[tile('out:0', '29999990000')]}
        fee="10000"
        continuity={[
          { from: 'in:0', to: 'out:0', label: 'same file cell continues', kind: 'file' },
        ]}
        linking="page"
        focusedCellIds={['in:0']}
        onCellHover={onCellHover}
      />
    );
    expect(screen.getByTestId('in:0')).toHaveAttribute('data-highlighted', 'true');
    expect(screen.getByTestId('out:0')).toHaveAttribute('data-highlighted', 'false');
    expect(screen.getByTestId('out:0')).toHaveClass('opacity-30');
  });

  it('can leave the continuity legend to the page', () => {
    render(
      <TxStructure
        txHash="0xtx"
        inputs={[tile('in:0', '20000000000')]}
        outputs={[tile('out:0', '19999990000')]}
        fee="10000"
        continuity={[
          { from: 'in:0', to: 'out:0', label: 'same file cell continues', kind: 'file' },
        ]}
        continuityList={false}
      />
    );
    expect(screen.queryByRole('list', { name: 'Cell continuity' })).not.toBeInTheDocument();
  });
});

describe('TxStructure inflow source (D4)', () => {
  it('draws DAO compensation as a dashed gold source flowing into the pool', () => {
    render(
      <TxStructure
        txHash="0xtx"
        inputs={[tile('in:0', '781583043832627')]}
        outputs={[tile('out:0', '784155903390101')]}
        fee="936"
        inflow={{ tone: 'dao', label: 'Nervos DAO compensation', amount: '2572859558410' }}
      />
    );
    const source = screen.getByTestId('tx-inflow');
    expect(source).toHaveAttribute('data-inflow', 'dao');
    expect(source).toHaveTextContent('Nervos DAO compensation');
    expect(source).toHaveTextContent('25,728.5955841');
    expect(source).toHaveClass('border-dashed', 'border-gold/60');
  });

  it('draws a cellbase block reward as a neutral source with no consumed cell', () => {
    render(
      <TxStructure
        txHash="0xtx"
        inputs={[]}
        outputs={[tile('out:0', '55961739927')]}
        fee="0"
        inflow={{ tone: 'reward', label: 'block reward', amount: '55961739927' }}
      />
    );
    const source = screen.getByTestId('tx-inflow');
    expect(source).toHaveAttribute('data-inflow', 'reward');
    expect(source).toHaveTextContent('block reward');
    expect(source).not.toHaveClass('border-gold/60');
  });
});
