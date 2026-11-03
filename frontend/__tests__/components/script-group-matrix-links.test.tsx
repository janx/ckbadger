// Lane L (L1): a matrix column names a cell ("in #0", "out #0"); when the cell has a page the
// column label links to it, its hover title the full out point. A pool output is not a cell until
// the transaction is committed, so its label stays text.
import { describe, expect, it } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { ScriptGroupMatrix, type MatrixGroup } from '@/components/transaction/script-group-matrix';

const PREV = `0x0dde1c03${'00'.repeat(26)}fc4657`;
const inputs = [
  {
    id: 'in:0',
    top: 'plain',
    label: 'in #0',
    cap: '400',
    href: `/cell/${PREV}-2`,
    title: `${PREV}:2`,
  },
];
const outputs = [{ id: 'out:0', top: 'plain', label: 'out #0', cap: '400' }];
const groups: MatrixGroup[] = [
  {
    id: 'lock:run:0xl1',
    kind: 'lock',
    name: 'Default Lock',
    sub: 'args 0x55c5…a0b9',
    ran: true,
    cellIds: ['in:0'],
    witness: null,
    code: '—',
    cycles: null,
  },
];

describe('ScriptGroupMatrix column labels link to their cells (L1)', () => {
  it('links a column with a cell page, and leaves one without as text', () => {
    render(
      <ScriptGroupMatrix
        txHash="0xtx"
        inputs={inputs}
        outputs={outputs}
        groups={groups}
        lit={null}
        onHover={() => {}}
      />
    );
    const table = screen.getByRole('table', { name: 'Script groups' });
    const link = within(table).getByRole('link', { name: 'in #0' });
    expect(link).toHaveAttribute('href', `/mainnet/cell/${PREV}-2`);
    expect(link).toHaveAttribute('title', `${PREV}:2`);
    expect(within(table).queryByRole('link', { name: 'out #0' })).toBeNull();
    expect(within(table).getByText('out #0')).toBeInTheDocument();
  });
});
