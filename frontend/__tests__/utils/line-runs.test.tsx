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
