import { describe, expect, it, vi } from 'vitest';
import { cleanup, render } from '@testing-library/react';
import {
  assertListTables,
  cellFaults,
  faultLines,
  listTables,
} from '@/__tests__/utils/list-table-cells';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { ListTable, type ListColumn } from '@/components/ui/list-table';

/**
 * The reading every ListTable table the suite renders is held to (`__tests__/setup.ts`), on tables
 * written out by hand, detached from the page so the after-test check does not read them: what it
 * finds and what it lets through, rule by rule.
 */
const OK = 'min-w-0 py-3 align-top pl-0 pr-4 last:pr-0';
const NUMERIC = `${OK} whitespace-nowrap text-right tabular-nums`;

function table(cells: string, head = `<th class="${OK}">Head</th>`): HTMLTableElement {
  const box = document.createElement('div');
  box.innerHTML = `<table aria-label="Fixture"><thead><tr>${head}</tr></thead><tbody><tr>${cells}</tr></tbody></table>`;
  return box.querySelector('table')!;
}
const faults = (cells: string, head?: string) => faultLines(cellFaults(table(cells, head)));

describe('cellFaults: a cell’s horizontal space is the table’s', () => {
  it('passes a head and a cell with exactly pl-0, the gap and last:pr-0', () => {
    expect(faults(`<td class="${OK}">a</td>`)).toEqual([]);
  });

  // Re-pinned on purpose (review fb-logic N2 with fb-tests SHOULD-3, the lead's ruling): the check
  // read a cell's classes by a second copy of the table's rule, a regex of what takes a horizontal
  // pixel, which disagreed with the table's own; it reads them now by the table's one rule
  // (`cellClassOwner`): a class is the table's own or one a column may set, and anything else is a
  // fault (renamed: the rule "padding" is "cell class").
  it('names a cell or a head with a class that is neither the table’s nor a column’s, or without its own', () => {
    expect(faults(`<td class="${OK} px-2">a</td>`)).toEqual([
      'Fixture · row 1, Head · cell class: "+[px-2] −[]"',
    ]);
    expect(faults(`<td class="min-w-0 py-3 pr-4 last:pr-0">a</td>`)).toEqual([
      'Fixture · row 1, Head · cell class: "+[] −[pl-0]"',
    ]);
    expect(faults(`<td class="${OK}">a</td>`, `<th class="${OK} border-l w-24">Head</th>`)).toEqual(
      ['Fixture · head Head · cell class: "+[border-l w-24] −[]"']
    );
    // what the second copy let through: a utility it did not list, a border width it did not read
    expect(faults(`<td class="${OK} space-x-2 [border-left-width:2px]">a</td>`)).toEqual([
      'Fixture · row 1, Head · cell class: "+[space-x-2 [border-left-width:2px]] −[]"',
    ]);
  });
});

// Re-pinned on purpose (review fb-logic N2, the lead's ruling): a cell may no longer carry
// `break-words` itself (no column may set it, and the table does not: a cell class fault), so the
// break these fixtures read is drawn by the cell's content, a span, as a page draws it today.
describe('cellFaults: a figure never breaks inside itself', () => {
  it('names a figure where a word may break where it overflows', () => {
    // the 608 px blocks table before 073d3e2c: a non-numeric cell lets a word break, the block
    // number is a plain link
    expect(
      faults(`<td class="${OK}"><span class="break-words"><a>#21,045,845</a></span></td>`)
    ).toEqual(['Fixture · row 1, Head · figure breaks: "#21,045,845"']);
    expect(
      faults(`<td class="${OK}"><span class="[overflow-wrap:anywhere]">0.00000005 USDI</span></td>`)
    ).toEqual(['Fixture · row 1, Head · figure breaks: "0.00000005"']);
  });

  it('passes a figure held on one line, or where nothing lets a word break', () => {
    expect(
      faults(
        `<td class="${OK}"><span class="break-words"><a class="whitespace-nowrap">#21,045,845</a></span></td>`
      )
    ).toEqual([]);
    expect(faults(`<td class="${OK}"><a>#21,045,845</a></td>`)).toEqual([]);
    expect(faults(`<td class="${NUMERIC}">999.99999999</td>`)).toEqual([]);
    // a unit that resets the break: CountUnit's [overflow-wrap:normal]
    expect(
      faults(
        `<td class="${OK}"><span class="break-words"><span class="[overflow-wrap:normal]">12 participants</span></span></td>`
      )
    ).toEqual([]);
  });

  // Review fb-tests SHOULD-3 (e): the check knew four spellings of a word that may break; a page
  // writes others (ReadoutRow's inline `overflowWrap: 'anywhere'`).
  it('reads every spelling of a word that may break: the arbitrary properties and an inline style', () => {
    for (const span of [
      '<span class="[overflow-wrap:break-word]">',
      '<span class="[word-break:break-word]">',
      '<span style="overflow-wrap: anywhere">',
      '<span style="word-break: break-all">',
    ])
      expect(faults(`<td class="${OK}">${span}#21,045,845</span></td>`), span).toEqual([
        'Fixture · row 1, Head · figure breaks: "#21,045,845"',
      ]);
  });
});

describe('cellFaults: a whole id stands where it can', () => {
  const hash = `0x${'ab'.repeat(32)}`;
  it('names a whole id held on one line, or where it cannot break, outside a box that clips it', () => {
    expect(faults(`<td class="${OK}"><span class="whitespace-nowrap">${hash}</span></td>`)).toEqual(
      [`Fixture · row 1, Head · long run does not break: "${hash}"`]
    );
    expect(faults(`<td class="${OK}">${hash}</td>`)).toEqual([
      `Fixture · row 1, Head · long run does not break: "${hash}"`,
    ]);
  });

  it('passes a whole id that wraps where its box lets it, or in a box that scrolls or clips', () => {
    expect(faults(`<td class="${OK}"><span class="break-all">${hash}</span></td>`)).toEqual([]);
    expect(
      faults(
        `<td class="${OK}"><span class="overflow-x-auto whitespace-nowrap">${hash}</span></td>`
      )
    ).toEqual([]);
    expect(faults(`<td class="${OK} truncate">${hash}</td>`)).toEqual([]);
  });
});

describe('cellFaults: words may always break between them', () => {
  it('names a phrase held on one line in a column that is not numeric', () => {
    // review fb-logic 3: the age column's sentence
    expect(
      faults(
        `<td class="${OK} whitespace-nowrap"><span class="whitespace-nowrap"><time>this device's clock is behind</time></span></td>`
      )
    ).toEqual([`Fixture · row 1, Head · words do not break: "this device's clock is behind"`]);
    // the lead's Chrome reading of /transactions at 1280: a 234 px headline in 217 px
    expect(
      faults(
        `<td class="${OK}"><span class="text-text-bright whitespace-nowrap">.bit · recycle expired account</span></td>`
      )
    ).toEqual([`Fixture · row 1, Head · words do not break: ".bit · recycle expired account"`]);
  });

  it('passes words that may break, a figure with its one word, a chip, a numeric cell, a clipped box', () => {
    expect(faults(`<td class="${OK}"><time>this device's clock is behind</time></td>`)).toEqual([]);
    expect(
      faults(`<td class="${OK}"><span class="whitespace-nowrap">10y 364d ago</span></td>`)
    ).toEqual([]);
    expect(
      faults(
        `<td class="${OK}"><span data-row-tag="default" class="inline-block whitespace-nowrap">Composition unavailable</span></td>`
      )
    ).toEqual([]);
    expect(
      faults(
        `<td class="${OK}"><span data-composition="pure_ckb" class="whitespace-nowrap">Pure CKB</span></td>`
      )
    ).toEqual([]);
    expect(faults(`<td class="${NUMERIC}">Not yet run</td>`)).toEqual([]);
    expect(faults(`<td class="${OK} truncate">A Mining Pool Whose Name Is Long</td>`)).toEqual([]);
    // a sentence that resets its own white-space under a nowrap run breaks (Utc8Age's)
    expect(
      faults(
        `<td class="${OK}"><span class="whitespace-nowrap">#1 · <time class="whitespace-normal">this device's clock is behind</time></span></td>`
      )
    ).toEqual([]);
  });

  // Re-pinned on purpose (review fb-tests SHOULD-3 (a), the lead's ruling): a breakpoint's class
  // was not read ("a table is drawn at every width it fits"), which argues the other way: a wide
  // table is drawn from md up, so a phrase made unbreakable from md up is a fault there. Every
  // breakpoint's white-space is read; one that holds a phrase on one line at any width is a fault.
  it('reads a breakpoint’s white-space: a phrase unbreakable at any width is a fault', () => {
    expect(
      faults(`<td class="${OK}"><span class="md:whitespace-nowrap">two words</span></td>`)
    ).toEqual(['Fixture · row 1, Head · words do not break: "two words"']);
    expect(
      faults(
        `<td class="${OK}"><span class="whitespace-nowrap md:whitespace-normal">two words</span></td>`
      )
    ).toEqual(['Fixture · row 1, Head · words do not break: "two words"']);
    // a state's white-space is not a width's: the check cannot say where it holds, and refuses it
    expect(() =>
      faults(`<td class="${OK}"><span class="hover:whitespace-nowrap">two words</span></td>`)
    ).toThrow(/hover:whitespace-nowrap/);
  });

  // Review fb-tests SHOULD-3 (b): a phrase was judged one text node at a time, so a nowrap run of
  // one-word nodes was never two words. The run is the text its nowrap element holds.
  it('judges a phrase across the text nodes of its nowrap run', () => {
    expect(
      faults(
        `<td class="${OK}"><span class="whitespace-nowrap">20s<b> behind</b> <i>clock</i></span></td>`
      )
    ).toEqual(['Fixture · row 1, Head · words do not break: "20s behind clock"']);
  });
});

// Review fb-tests SHOULD-3 (d): the after-test check reads `[data-list-form] > table`, and nothing
// held that it finds the tables the suite draws: a wrapper around the <table> (a scroll box) would
// have blinded it with every test green.
describe('the after-test check finds every ListTable table and fails on a fault', () => {
  const columns: ListColumn<{ id: string; name: string }>[] = [
    { key: 'id', label: 'Id', minWidth: 80, render: (r) => r.id },
    { key: 'name', label: 'Name', minWidth: 120, render: (r) => r.name },
  ];
  const rows = [{ id: '1', name: 'Nervape' }];

  it('finds a mounted ListTable table and passes it', () => {
    stubChartWidth(608);
    render(
      <ListTable
        label="Mounted"
        columns={columns}
        rows={rows}
        rowKey={(r) => r.id}
        mobileRow={() => null}
      />
    );
    expect(listTables().map((t) => t.getAttribute('aria-label'))).toEqual(['Mounted']);
    expect(() => assertListTables()).not.toThrow();
    cleanup();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('fails on a mounted ListTable whose cell breaks the rules', () => {
    stubChartWidth(608);
    const phrase: ListColumn<{ id: string; name: string }> = {
      ...columns[1],
      render: (r) => <span className="whitespace-nowrap">{r.name} the ape</span>,
    };
    render(
      <ListTable
        label="Faulty"
        columns={[columns[0], phrase]}
        rows={rows}
        rowKey={(r) => r.id}
        mobileRow={() => null}
      />
    );
    expect(() => assertListTables()).toThrow(
      'Faulty · row 1, Name · words do not break: "Nervape the ape"'
    );
    // the check's own failure must not reach the after-test hook: the page is emptied here
    cleanup();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('refuses a list form whose table it cannot reach', () => {
    const box = document.createElement('div');
    box.innerHTML =
      '<div data-list-form="table"><div class="overflow-x-auto"><table aria-label="Wrapped"><tbody><tr><td>a</td></tr></tbody></table></div></div>';
    document.body.append(box);
    try {
      expect(() => listTables()).toThrow(/data-list-form="table".*no <table> of its own/);
    } finally {
      box.remove();
    }
    // an empty list draws its sentence, not a table
    box.innerHTML = '<div data-list-form="table"><p>No records.</p></div>';
    expect(listTables(box)).toEqual([]);
  });
});
