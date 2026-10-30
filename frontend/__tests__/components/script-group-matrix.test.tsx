import { describe, it, expect, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { ScriptGroupMatrix, type MatrixGroup } from '@/components/transaction/script-group-matrix';
import { txLinks } from '@/lib/tx-detail-model';

const inputs = [
  { id: 'in:0', top: 'CKBFS', label: 'in #0', cap: '195' },
  { id: 'in:1', top: 'plain', label: 'in #1', cap: '42,081.5…' },
];
const outputs = [{ id: 'out:0', top: 'CKBFS', label: 'out #0', cap: '208' }];
const groups: MatrixGroup[] = [
  {
    id: 'lock:run:0xl1',
    kind: 'lock',
    name: 'Default Lock',
    sub: 'args 0x55c5…a0b9',
    ran: true,
    cellIds: ['in:0', 'in:1'],
    witness: '#0 · WitnessArgs · 85 B',
    code: 'dep group 0x71a7…:0',
    cycles: '1694860',
    focusTestId: 'focus-l1',
  },
  {
    id: 'lock:attached:0xo1',
    kind: 'lock',
    name: '0x43b292…895d',
    sub: 'owner ckb1qppm9y…204m4g',
    ran: false,
    cellIds: ['out:0'],
    witness: '—',
    code: '—',
    cycles: null,
    focusTestId: 'focus-o1',
  },
];

describe('ScriptGroupMatrix (TX-13, TX-21)', () => {
  it('draws one column per cell and a ● or ○ where each group touches a cell', () => {
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
    expect(
      within(table)
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual([
      'Script',
      'Witness it reads',
      'CKBFSin #0195plainin #142,081.5…→CKBFSout #0208',
      'Code, from cell dep',
    ]);
    const ran = table.querySelector('[data-group-row="lock:run:0xl1"]')!;
    expect(ran.querySelectorAll('[data-dot="ran"]')).toHaveLength(2);
    expect(ran.querySelector('[data-span]')).toBeInTheDocument();
    expect(ran).toHaveTextContent('1,694,860 cycles');
    const attached = table.querySelector('[data-group-row="lock:attached:0xo1"]')!;
    expect(attached.querySelectorAll('[data-dot="attached"]')).toHaveLength(1);
    expect(attached.querySelector('[data-span]')).toBeNull();
    expect(attached).toHaveTextContent('attached, runs when spent');
  });

  it('dims every group and cell column the hover does not link', () => {
    const lit = txLinks(
      { t: 'group', id: 'lock:attached:0xo1' },
      groups.map((g) => ({ ...g, witnessIndex: null, scriptHash: g.id })),
      [],
      '0xtx'
    );
    render(
      <ScriptGroupMatrix
        txHash="0xtx"
        inputs={inputs}
        outputs={outputs}
        groups={groups}
        lit={lit}
        onHover={() => {}}
      />
    );
    expect(document.querySelector('[data-group-row="lock:run:0xl1"]')).toHaveAttribute(
      'data-dimmed',
      'true'
    );
    expect(document.querySelector('[data-group-row="lock:attached:0xo1"]')).toHaveAttribute(
      'data-lit',
      'true'
    );
    expect(document.querySelector('[data-column="in:0"]')).toHaveAttribute('data-dimmed', 'true');
    expect(document.querySelector('[data-column="out:0"]')).toHaveAttribute('data-dimmed', 'false');
  });

  it('reports group and cell hovers to the page', () => {
    const onHover = vi.fn();
    render(
      <ScriptGroupMatrix
        txHash="0xtx"
        inputs={inputs}
        outputs={outputs}
        groups={groups}
        lit={null}
        onHover={onHover}
      />
    );
    fireEvent.mouseEnter(document.querySelector('[data-group-row="lock:run:0xl1"]')!);
    expect(onHover).toHaveBeenLastCalledWith({ t: 'group', id: 'lock:run:0xl1' });
    fireEvent.mouseEnter(document.querySelector('[data-column="in:1"]')!);
    expect(onHover).toHaveBeenLastCalledWith({ t: 'cell', id: 'in:1' });
    fireEvent.mouseLeave(document.querySelector('[data-column="in:1"]')!);
    expect(onHover).toHaveBeenLastCalledWith(null);
  });

  it('refuses a group that names a cell the transaction does not have', () => {
    expect(() =>
      render(
        <ScriptGroupMatrix
          txHash="0xtx"
          inputs={inputs}
          outputs={outputs}
          groups={[{ ...groups[0], cellIds: ['in:7'] }]}
          lit={null}
          onHover={() => {}}
        />
      )
    ).toThrow('0xtx: script group lock:run:0xl1 references unknown cell in:7');
  });
});

describe('ScriptGroupMatrix column heads (visual pass)', () => {
  it('keeps a long kind word on one line and names it in full on hover', () => {
    render(
      <ScriptGroupMatrix
        txHash="0xtx"
        inputs={[{ id: 'in:0', top: '.bit Time Index State', label: 'in #0', cap: '400' }]}
        outputs={[]}
        groups={[]}
        lit={null}
        onHover={() => {}}
      />
    );
    const top = screen.getByTitle('.bit Time Index State');
    expect(top).toHaveClass('truncate');
  });
});
