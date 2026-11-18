import { describe, expect, it } from 'vitest';
import { cellFaults, faultLines } from '@/__tests__/utils/list-table-cells';

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

  it('names a cell or a head with a padding, a margin, a width or a side border, or without its own', () => {
    expect(faults(`<td class="${OK} px-2">a</td>`)).toEqual([
      'Fixture · row 1, Head · padding: "+[px-2] −[]"',
    ]);
    expect(faults(`<td class="min-w-0 py-3 pr-4 last:pr-0">a</td>`)).toEqual([
      'Fixture · row 1, Head · padding: "+[] −[pl-0]"',
    ]);
    expect(faults(`<td class="${OK}">a</td>`, `<th class="${OK} border-l w-24">Head</th>`)).toEqual(
      ['Fixture · head Head · padding: "+[border-l w-24] −[]"']
    );
  });
});

describe('cellFaults: a figure never breaks inside itself', () => {
  it('names a figure where a word may break where it overflows', () => {
    // the 608 px blocks table before 073d3e2c: a non-numeric cell lets a word break, the block
    // number is a plain link
    expect(faults(`<td class="${OK} break-words"><a>#21,045,845</a></td>`)).toEqual([
      'Fixture · row 1, Head · figure breaks: "#21,045,845"',
    ]);
    expect(
      faults(`<td class="${OK}"><span class="[overflow-wrap:anywhere]">0.00000005 USDI</span></td>`)
    ).toEqual(['Fixture · row 1, Head · figure breaks: "0.00000005"']);
  });

  it('passes a figure held on one line, or where nothing lets a word break', () => {
    expect(
      faults(`<td class="${OK} break-words"><a class="whitespace-nowrap">#21,045,845</a></td>`)
    ).toEqual([]);
    expect(faults(`<td class="${OK}"><a>#21,045,845</a></td>`)).toEqual([]);
    expect(faults(`<td class="${NUMERIC}">999.99999999</td>`)).toEqual([]);
    // a unit that resets the break: CountUnit's [overflow-wrap:normal]
    expect(
      faults(
        `<td class="${OK} break-words"><span class="[overflow-wrap:normal]">12 participants</span></td>`
      )
    ).toEqual([]);
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
    // a breakpoint's class is not read: a table is drawn at every width it fits
    expect(faults(`<td class="${OK} md:whitespace-nowrap">two words</td>`)).toEqual([]);
  });
});
