import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';
import { lineRuns } from './line-runs';

/**
 * The reader of what a line cannot break inside (`__tests__/utils/line-runs.ts`), at the numbers
 * written here: 13 px text (`text-small`), 7.8 px a glyph; Tailwind's 4 px spacing unit.
 */

const first = (jsx: React.ReactElement) => render(jsx).container.firstElementChild as HTMLElement;
const runs = (jsx: React.ReactElement, viewport = 390) =>
  lineRuns(first(jsx), viewport).map((r) => [r.text, Number(r.px.toFixed(2))]);

describe('line-runs reads the runs a line cannot break inside', () => {
  it('breaks at a space that wraps, never inside a nowrap run', () => {
    expect(
      runs(
        <span className="text-small">
          <span className="whitespace-nowrap">#21,045,845</span> · this clock
        </span>
      )
    ).toEqual([
      ['#21,045,845', 85.8],
      ['·', 7.8],
      ['this', 31.2],
      ['clock', 39],
    ]);
    // a nowrap ancestor holds every word under it, a `normal` child lets its own spaces wrap
    expect(
      runs(
        <span className="text-small whitespace-nowrap">
          #1 · <time className="whitespace-normal">is behind</time>
        </span>
      )
    ).toEqual([
      ['#1 · is', 54.6],
      ['behind', 46.8],
    ]);
  });

  it('reads a breakpoint variant at its viewport', () => {
    const jsx = <span className="text-small md:whitespace-nowrap">two words</span>;
    expect(runs(jsx, 390)).toEqual([
      ['two', 23.4],
      ['words', 39],
    ]);
    expect(runs(jsx, 768)).toEqual([['two words', 70.2]]);
  });

  it('breaks between the items of a wrapping flex row; joins a row that does not wrap by its gap', () => {
    expect(
      runs(
        <span className="text-small flex flex-wrap gap-2">
          <span>ab</span>
          <span>cd</span>
        </span>
      )
    ).toEqual([
      ['ab', 15.6],
      ['cd', 15.6],
    ]);
    // a 7 px dot, the 6 px gap, the word: one run
    expect(
      runs(
        <span className="text-small inline-flex items-center gap-1.5">
          <span className="h-[7px] w-[7px]" />
          <span>Pending</span>
        </span>
      )
    ).toEqual([['▯Pending', 7 + 6 + 54.6]]);
  });

  // Lane B4 fix round (review-rules-tests, the coordinator's item 5): a flex item that cannot
  // shrink (`shrink-0`, `flex-none`) stands at its max-content width, so a line breaks nowhere
  // inside it, its spaces and its own wrapping rows included: one run (a block inside it still
  // starts a line of its own). The reader split it at its spaces, a run narrower than the browser's.
  it('reads a flex item that cannot shrink as one run', () => {
    for (const rigid of ['shrink-0', 'flex-shrink-0', 'flex-none'])
      expect(
        runs(
          <span className="text-small flex flex-wrap gap-2">
            <span>ab</span>
            <span className={rigid}>
              cd ef{' '}
              <span className="inline-flex flex-wrap gap-2">
                <span>gh</span>
                <span>ij</span>
              </span>
            </span>
          </span>
        )
      ).toEqual([
        ['ab', 15.6],
        // "cd ef " and "gh", the inner row's 8 px gap, "ij"
        ['cd ef ghij', 10 * 7.8 + 8],
      ]);
    // read alone, it is still its parent's item
    const { container } = render(
      <span className="text-small flex flex-wrap gap-2">
        <span className="shrink-0">cd ef</span>
      </span>
    );
    expect(lineRuns(container.querySelector('.shrink-0')!, 390).map((r) => [r.text, r.px])).toEqual(
      [['cd ef', 5 * 7.8]]
    );
    // a block inside it is a line of its own, at its max-content width too
    expect(
      runs(
        <span className="text-small flex flex-wrap gap-2">
          <span className="shrink-0">
            one two<div>three four</div>
          </span>
        </span>
      )
    ).toEqual([
      ['one two', 7 * 7.8],
      ['three four', 10 * 7.8],
    ]);
    // an item that may shrink (the default, `shrink`) breaks at its spaces; in a column, an item's
    // shrink is along the column and its width is the column's
    expect(
      runs(
        <span className="text-small flex flex-wrap gap-2">
          <span className="shrink">cd ef</span>
        </span>
      )
    ).toEqual([
      ['cd', 15.6],
      ['ef', 15.6],
    ]);
    expect(
      runs(
        <span className="text-small flex flex-col">
          <span className="shrink-0">cd ef</span>
        </span>
      )
    ).toEqual([
      ['cd', 15.6],
      ['ef', 15.6],
    ]);
    // capped at its container (`max-w-full`), it never stands wider than its line: it breaks
    expect(
      runs(
        <span className="text-small flex flex-wrap gap-2">
          <span className="max-w-full shrink-0">cd ef</span>
        </span>
      )
    ).toEqual([
      ['cd', 15.6],
      ['ef', 15.6],
    ]);
    // a rigid item of a set width: its box, not its text, stands in the line; not read
    expect(() =>
      runs(
        <span className="text-small flex">
          <span className="w-14 shrink-0">cd ef</span>
        </span>
      )
    ).toThrow(/cannot shrink and has a set width/);
  });

  // B4 second fix round (review-rules-fixes-tests NOTE-3, the coordinator's item 16): what the
  // reader would misread it refuses, by name: an item behind `display: contents` (its flex row is
  // its grandparent's, and the wrapper drops the row's gap), an item that cannot shrink below its
  // max-content width by its minimum (`min-w-max`, `min-w-fit`), and an item with two width
  // classes (which wins is the cascade's, not the first one read).
  it('refuses an item behind display: contents, a max-content minimum, two width classes', () => {
    expect(() =>
      runs(
        <span className="text-small flex flex-wrap gap-2">
          <span className="contents">
            <span className="shrink-0">cd ef</span>
          </span>
        </span>
      )
    ).toThrow(/display: contents/);
    for (const min of ['min-w-max', 'min-w-fit'])
      expect(() =>
        runs(
          <span className="text-small flex flex-wrap gap-2">
            <span className={min}>cd ef</span>
          </span>
        )
      ).toThrow(/cannot shrink below its max-content/);
    expect(() =>
      runs(
        <span className="text-small flex">
          <span className="max-w-full shrink-0 basis-0">cd ef</span>
        </span>
      )
    ).toThrow(/two width classes/);
  });

  it('starts a run at a block, and leaves out what is not drawn', () => {
    expect(
      runs(
        <div className="text-small">
          ab<div>cd</div>
          <span className="sr-only">gone</span>ef
        </div>
      )
    ).toEqual([
      ['ab', 15.6],
      ['cd', 15.6],
      ['ef', 15.6],
    ]);
  });

  it('refuses what it cannot read', () => {
    expect(() => runs(<span className="break-words">abc</span>)).toThrow(/break inside its words/);
    expect(() => runs(<span style={{ overflowWrap: 'anywhere' }}>abc</span>)).toThrow(
      /break inside its words/
    );
    expect(() => runs(<span>蚂蚁</span>)).toThrow(/outside the served Latin subset/);
    expect(() => runs(<div className="grid">a</div>)).toThrow(/grid/);
  });
});
