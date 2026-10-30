import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { CellTile } from '@/components/ui/cell-tile';

describe('CellTile occupied wording (TX-19)', () => {
  it('states occupied capacity as whole bytes, as the board does ("occupied 195")', () => {
    render(
      <CellTile
        cell={{
          id: 'in:0',
          testId: 'tile',
          href: '/cell/0x-0',
          label: 'input 0',
          owner: 'owner',
          lockKind: 'Default Lock',
          kind: 'other',
          content: 'CKBFS file',
          capacity: '19500000000',
          occupied: '19500000000',
          state: 'consumed',
        }}
      />
    );
    const tile = screen.getByTestId('tile');
    expect(tile).toHaveTextContent('occupied 195');
    expect(tile).not.toHaveTextContent('occupied 195.00000000');
  });

  it('refuses an occupied amount that is not whole bytes', () => {
    expect(() =>
      render(
        <CellTile
          cell={{
            id: 'in:0',
            href: '/cell/0x-0',
            label: 'input 0',
            owner: 'owner',
            lockKind: null,
            kind: 'ckb',
            content: 'CKB',
            capacity: '19500000000',
            occupied: '6100000001',
            state: 'consumed',
          }}
        />
      )
    ).toThrow('in:0: occupied 6100000001 shannons is not whole bytes');
  });
});
