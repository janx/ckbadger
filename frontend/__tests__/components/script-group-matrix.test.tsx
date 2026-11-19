import { afterEach, describe, it, expect, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import {
  cellsInWords,
  ScriptGroupMatrix,
  type MatrixGroup,
} from '@/components/transaction/script-group-matrix';
import { txLinks } from '@/lib/tx-detail-model';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { TYPE_SCALE, type TypeStep } from '@/lib/type-scale';
import { typeStepOf } from '@/__tests__/utils/type-step-of';

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
    sub: () => 'args 0x55c5…a0b9',
    ran: true,
    cellIds: ['in:0', 'in:1'],
    witness: { index: 0, detail: { kind: 'WitnessArgs', size: 85 } },
    code: () => 'dep group 0x71a7…:0',
    cycles: '1694860',
    focusTestId: 'focus-l1',
  },
  {
    id: 'lock:attached:0xo1',
    kind: 'lock',
    name: '0x43b292…895d',
    sub: () => 'owner ckb1qppm9y…204m4g',
    ran: false,
    cellIds: ['out:0'],
    witness: null,
    code: () => '—',
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

  // Review of phases C and D, NOTE 10 (lane P1): where the page has no inspector (the cell page's
  // developer tier, no `onHover`) nothing in the matrix answers a focus, so nothing in it is a tab
  // stop of its own: a column head and a row took one each and did nothing. The links in it stay.
  it('makes no tab stop of a column head or a row where the page has no inspector', () => {
    const { container } = render(
      <ScriptGroupMatrix
        txHash="0xtx"
        inputs={inputs}
        outputs={outputs}
        groups={groups}
        lit={null}
      />
    );
    expect(container.querySelectorAll('[tabindex]')).toHaveLength(0);
    // with an inspector, each head and each row is one
    const inspected = render(
      <ScriptGroupMatrix
        txHash="0xtx"
        inputs={inputs}
        outputs={outputs}
        groups={groups}
        lit={null}
        onHover={() => {}}
      />
    );
    expect(inspected.container.querySelectorAll('[tabindex="0"]')).toHaveLength(
      inputs.length + outputs.length + groups.length
    );
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

/**
 * Responsive plan T2 (board T2-script-groups.dc.html, review A#3/A#4): the matrix only when every
 * cell column fits the measured width (1,128 px for three inputs and three outputs); otherwise one
 * entry per group saying in words where it ran and which witness it read. Never a scroller.
 */
describe('ScriptGroupMatrix form by measured width (T2)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  const six = {
    inputs: [0, 1, 2].map((i) => ({ id: `in:${i}`, top: 'plain', label: `in #${i}`, cap: '61' })),
    outputs: [0, 1, 2].map((i) => ({
      id: `out:${i}`,
      top: 'plain',
      label: `out #${i}`,
      cap: '61',
    })),
  };
  const sixGroups: MatrixGroup[] = [
    {
      id: 'lock:run:0xl1',
      kind: 'lock',
      name: 'Default Lock',
      href: '/scripts/default-lock',
      sub: () => 'pubkey hash 0x55c5…a0b9',
      ran: true,
      cellIds: ['in:0', 'in:1', 'in:2'],
      witness: { index: 0, detail: { kind: 'WitnessArgs', size: 85 } },
      code: () => 'dep group 0x71a7…:0',
      cycles: '1658503',
      focusTestId: 'focus-l1',
    },
    {
      id: 'type:run:0xt1',
      kind: 'type',
      name: '.bit Time Index State',
      sub: () => 'args 0x00',
      ran: true,
      cellIds: ['in:0', 'out:0'],
      witness: { index: 1, detail: null },
      code: () => 'code 0x96b1…:0',
      cycles: '32977',
    },
    {
      id: 'lock:attached:0xo1',
      kind: 'lock',
      name: 'Default Lock',
      sub: () => 'pubkey hash 0x77aa…0102',
      ran: false,
      cellIds: ['out:0', 'out:1', 'out:2'],
      witness: null,
      code: () => 'code 0xd00a…:0 · not run here',
      cycles: null,
    },
  ];
  const draw = (width: number, onHover = () => {}) => {
    stubChartWidth(width);
    return render(
      <ScriptGroupMatrix
        txHash="0xtx"
        inputs={six.inputs}
        outputs={six.outputs}
        groups={sixGroups}
        lit={null}
        onHover={onHover}
      />
    );
  };

  // U-09: a group's args line and its code dep take the size of the form they are drawn in
  it('asks for the row size in the matrix and the tight size in the list', () => {
    const sized = sixGroups.map((g) => ({
      ...g,
      sub: (size: string) => `args at ${size}`,
      code: (size: string) => `code at ${size}`,
    }));
    for (const [width, size] of [
      [1128, 'row'],
      [358, 'tight'],
    ] as const) {
      stubChartWidth(width);
      const { unmount } = render(
        <ScriptGroupMatrix
          txHash="0xtx"
          inputs={six.inputs}
          outputs={six.outputs}
          groups={sized}
          lit={null}
          onHover={() => {}}
        />
      );
      expect(screen.getAllByText(`args at ${size}`)).toHaveLength(3);
      expect(screen.getAllByText(`code at ${size}`).length).toBeGreaterThan(0);
      unmount();
    }
  });

  it('draws the matrix only when every cell column fits (1,128 px for six cells)', () => {
    draw(1128);
    expect(screen.getByRole('table', { name: 'Script groups' })).toBeInTheDocument();
    expect(document.querySelector('.overflow-x-auto')).toBeNull();
  });

  it('lists the groups one entry each when a cell column would not fit, never scrolling', () => {
    draw(1127);
    expect(screen.queryByRole('table')).toBeNull();
    const list = screen.getByRole('list', { name: 'Script groups' });
    expect(list.querySelectorAll('[data-group-row]')).toHaveLength(3);
    expect(document.querySelector('.overflow-x-auto')).toBeNull();
  });

  it('says where a group ran and which witness it read, with its name, kind and cycles', () => {
    draw(358);
    const lock = document.querySelector('[data-group-row="lock:run:0xl1"]')!;
    const head = lock.querySelector('[data-line="head"]')!;
    expect(within(head as HTMLElement).getByRole('link', { name: 'Default Lock' })).toHaveAttribute(
      'href',
      '/mainnet/scripts/default-lock'
    );
    expect(head).toHaveTextContent('Default Lock · lock');
    expect(head).toHaveTextContent('1,658,503 cycles');
    expect(lock.querySelector('[data-line="runs"]')).toHaveTextContent(
      'runs in input 0, 1, 2 · reads witness #0 (WitnessArgs 85 B)'
    );
    // A type group that runs on both sides reads input → output.
    expect(
      document.querySelector('[data-group-row="type:run:0xt1"] [data-line="runs"]')
    ).toHaveTextContent('runs in input 0 → output 0 · reads witness #1');
    // Nothing the matrix shows is dropped: the args line and the code dep stay, quieter.
    expect(lock.querySelector('[data-line="evidence"]')).toHaveTextContent(
      'pubkey hash 0x55c5…a0b9'
    );
    expect(lock.querySelector('[data-line="evidence"]')).toHaveTextContent('dep group 0x71a7…:0');
    // lane P1, the ink and size roles: the cycles figure, the runs sentence and the evidence line's
    // ids and code link take the list's 13 px (each set the meta step, 12 px)
    for (const line of ['head', 'runs', 'evidence'])
      for (const el of lock.querySelectorAll(`[data-line="${line}"], [data-line="${line}"] *`))
        expect(el).not.toHaveClass('text-meta');
    expect(typeStepOf(within(head as HTMLElement).getByText(/cycles$/))).toBe('small');
  });

  it('keeps a group that did not run, dimmer, saying which outputs it guards', () => {
    draw(358);
    const attached = document.querySelector('[data-group-row="lock:attached:0xo1"]')!;
    expect(attached).toHaveAttribute('data-ran', 'false');
    expect(attached).toHaveClass('text-text-dim');
    expect(attached.querySelector('[data-line="head"]')).toHaveTextContent(
      'Default Lock · lock of the outputs'
    );
    expect(attached.querySelector('[data-line="head"]')).toHaveTextContent('not run');
    expect(attached.querySelector('[data-line="runs"]')).toHaveTextContent(
      'guards output 0, 1, 2 for the next spend · no witness'
    );
    const ran = document.querySelector('[data-group-row="lock:run:0xl1"]')!;
    expect(ran).toHaveAttribute('data-ran', 'true');
    expect(ran).not.toHaveClass('text-text-dim');
  });

  it('reports a listed group to the page on hover and focuses it on click', () => {
    const onHover = vi.fn();
    const onFocus = vi.fn();
    stubChartWidth(358);
    render(
      <ScriptGroupMatrix
        txHash="0xtx"
        inputs={six.inputs}
        outputs={six.outputs}
        groups={[{ ...sixGroups[0], onFocus }]}
        lit={null}
        onHover={onHover}
      />
    );
    fireEvent.mouseEnter(document.querySelector('[data-group-row="lock:run:0xl1"]')!);
    expect(onHover).toHaveBeenLastCalledWith({ t: 'group', id: 'lock:run:0xl1' });
    fireEvent.click(screen.getByTestId('focus-l1'));
    expect(onFocus).toHaveBeenCalled();
  });

  it('marks a group with no code dep "—" in the matrix and leaves it out of the list', () => {
    const noDep = { ...sixGroups[2], code: () => null };
    stubChartWidth(1128);
    const { unmount } = render(
      <ScriptGroupMatrix
        txHash="0xtx"
        inputs={six.inputs}
        outputs={six.outputs}
        groups={[noDep]}
        lit={null}
        onHover={() => {}}
      />
    );
    const cells = document.querySelectorAll('[data-group-row="lock:attached:0xo1"] [role="cell"]');
    expect(cells[cells.length - 1]).toHaveTextContent(/^—$/);
    unmount();
    vi.restoreAllMocks();
    stubChartWidth(358);
    render(
      <ScriptGroupMatrix
        txHash="0xtx"
        inputs={six.inputs}
        outputs={six.outputs}
        groups={[noDep]}
        lit={null}
        onHover={() => {}}
      />
    );
    expect(
      document.querySelector('[data-group-row="lock:attached:0xo1"] [data-line="evidence"]')
    ).toHaveTextContent(/^pubkey hash 0x77aa…0102$/);
  });

  it('never prints a matrix column head under 11 px', () => {
    draw(1128);
    const header = screen.getAllByRole('columnheader')[2];
    const sizes = [header, ...header.querySelectorAll('*')].flatMap((el) =>
      [...el.classList].flatMap((c) => {
        const m = /^text-(meta|small|body|section|hero|index|entity|display)$/.exec(c);
        return m ? [parseFloat(TYPE_SCALE[m[1] as TypeStep][0])] : [];
      })
    );
    expect(sizes.length).toBeGreaterThan(0);
    expect(sizes.every((s) => s >= 11)).toBe(true);
    // lane P1, the ink and size roles: a cell's name in its head (a link to the cell once it is
    // one) at 13 px (it was the head row's 12); its side and capacity stay the head's 12 px labels
    const cells = Array.from(header.querySelectorAll('[data-column]'));
    expect(cells.length).toBeGreaterThan(0);
    for (const cell of cells)
      expect(Array.from(cell.children).map(typeStepOf)).toEqual(['meta', 'small', 'meta']);
  });
});

// Lane P1 (lane D3's note: the long transaction's default lock "runs in input 0, 1, …, 157", about
// ten lines long): a group's cells beyond three on a side are said as their count and their
// ranges, the count a unit (its number and noun joined by a no-break space, as every count's);
// three or fewer stay listed.
describe('cellsInWords: a long side as its count and its ranges (P1)', () => {
  const cols = (side: 'in' | 'out', n: number) =>
    Array.from({ length: n }, (_, i) => ({ id: `${side}:${i}`, top: '', label: '', cap: '' }));
  const ids = (side: 'in' | 'out', at: readonly number[]) => at.map((i) => `${side}:${i}`);
  const range = (from: number, to: number) =>
    Array.from({ length: to - from + 1 }, (_, i) => from + i);
  it('lists three cells or fewer, as before', () => {
    expect(cellsInWords(ids('in', [0, 1, 2]), cols('in', 3), [])).toBe('input 0, 1, 2');
    expect(
      cellsInWords([...ids('in', [0]), ...ids('out', [0])], cols('in', 1), cols('out', 1))
    ).toBe('input 0 → output 0');
  });
  it('says 158 inputs as their count and one range', () => {
    expect(cellsInWords(ids('in', range(0, 157)), cols('in', 158), [])).toBe(
      '158\u00a0inputs: 0 to 157'
    );
  });
  it('joins the ranges and the lone cells of a broken run, each side alike', () => {
    expect(
      cellsInWords(
        [...ids('in', [0, 1, 2, 3, 7, 9, 10, 11, 12]), ...ids('out', [1])],
        cols('in', 13),
        cols('out', 2)
      )
    ).toBe('9\u00a0inputs: 0 to 3, 7, 9 to 12 → output 1');
    expect(cellsInWords(ids('out', [0, 2, 4, 6]), [], cols('out', 7))).toBe(
      '4\u00a0outputs: 0, 2, 4, 6'
    );
  });
});
