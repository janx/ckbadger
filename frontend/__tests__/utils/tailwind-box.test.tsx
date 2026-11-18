import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';
import {
  boxPx,
  containerContentPx,
  contentPx,
  gapPx,
  inFlowAt,
  insetsPx,
  outerPx,
} from './tailwind-box';

/**
 * The reader of a row's widths from its classes (`__tests__/utils/tailwind-box.ts`) at the numbers
 * Tailwind's theme gives: its spacing unit 4 px (0.25 rem at the 16 px root), `px` 1 px, a border
 * 1 px, the type scale's 12 / 13 / 14 px and the face's 0.6 em glyph. Each number is written here,
 * so the reader cannot drift with the budgets it checks.
 */

const first = (jsx: React.ReactElement) => render(jsx).container.firstElementChild as HTMLElement;

describe('tailwind-box reads a box from its classes', () => {
  it('reads paddings, borders and margins off the spacing scale, the more specific side winning', () => {
    expect(insetsPx(first(<span className="px-3" />), 1024)).toEqual({ box: 24, margin: 0 });
    expect(insetsPx(first(<span className="px-2.5" />), 1024)).toEqual({ box: 20, margin: 0 });
    expect(insetsPx(first(<span className="border-l px-3" />), 1024)).toEqual({
      box: 25,
      margin: 0,
    });
    expect(insetsPx(first(<span className="border px-2" />), 1024)).toEqual({ box: 18, margin: 0 });
    expect(insetsPx(first(<span className="border-x-2 p-px" />), 1024)).toEqual({
      box: 6,
      margin: 0,
    });
    expect(insetsPx(first(<span className="p-1.5 pr-4" />), 1024)).toEqual({ box: 22, margin: 0 });
    expect(insetsPx(first(<span className="mr-1 border-r pr-2" />), 1024)).toEqual({
      box: 9,
      margin: 4,
    });
    expect(insetsPx(first(<span className="-ml-1" />), 1024).margin).toBe(-4);
    // an auto margin takes what is left over, nothing at max-content
    expect(insetsPx(first(<span className="ml-auto" />), 1024).margin).toBe(0);
    // colours are not widths
    expect(
      insetsPx(
        first(<span className="border-base-border border-jade/40 border-transparent" />),
        1024
      )
    ).toEqual({ box: 0, margin: 0 });
  });

  it('applies a breakpoint from its width up, and the later breakpoint over the earlier', () => {
    const input = first(<input className="well pl-7 pr-3 md:pr-9" />);
    // `.well` draws a 1 px border (app/globals.css)
    expect(insetsPx(input, 767).box).toBe(1 + 28 + 12 + 1);
    expect(insetsPx(input, 768).box).toBe(1 + 28 + 36 + 1);
    const word = first(<span className="hidden sm:inline">x</span>);
    expect(inFlowAt(word, 639)).toBe(false);
    expect(inFlowAt(word, 640)).toBe(true);
    const nav = first(<nav className="flex gap-x-4 lg:gap-x-6" />);
    expect(gapPx(nav, 1023)).toBe(16);
    expect(gapPx(nav, 1024)).toBe(24);
    // a state is no layout at rest
    expect(insetsPx(first(<span className="hover:px-4 focus-visible:border" />), 1024).box).toBe(0);
  });

  it('puts the inline style over every class', () => {
    const el = first(<span className="px-3" style={{ paddingLeft: 5, borderRightWidth: 1 }} />);
    expect(insetsPx(el, 1024).box).toBe(5 + 12 + 1);
    expect(gapPx(first(<span className="gap-2" style={{ columnGap: 6 }} />), 1024)).toBe(6);
  });

  it('measures text at the step it inherits, 0.6 em a glyph, with its tracking', () => {
    const line = first(
      <div className="text-meta">
        <span>ckbadger</span>
        <span className="eyebrow">mainnet</span>
        <span className="text-body tracking-wide">dao</span>
      </div>
    );
    const [plain, eyebrow, body] = Array.from(line.children);
    expect(boxPx(plain, 1024)).toBeCloseTo(8 * 7.2, 9);
    // `.eyebrow` tracks 0.12 em: 1.44 px at 12 px
    expect(boxPx(eyebrow, 1024)).toBeCloseTo(7 * (7.2 + 1.44), 9);
    // Tailwind's tracking-wide is 0.025 em
    expect(boxPx(body, 1024)).toBeCloseTo(3 * (8.4 + 0.35), 9);
  });

  it('lays a flex row with its gaps and a block as its widest line; leaves out what is not drawn', () => {
    const row = first(
      <div className="text-meta flex gap-2">
        <span>ab</span>
        <span className="sr-only">hidden words</span>
        <span className="absolute">over</span>
        <span className="hidden">gone</span>
        <svg className="h-3 w-3" />
        <span className="w-1.5" />
      </div>
    );
    // 14.4 + 12 + 6, two gaps of 8
    expect(contentPx(row, 1024)).toBeCloseTo(14.4 + 12 + 6 + 2 * 8, 9);
    const block = first(
      <div className="text-meta">
        <div>abcd</div>
        <div>ab</div>
      </div>
    );
    expect(boxPx(block, 1024)).toBeCloseTo(4 * 7.2, 9);
    // an inline span is no flex item: its width class does nothing
    expect(boxPx(first(<span className="text-meta w-10">a</span>), 1024)).toBeCloseTo(7.2, 9);
    expect(outerPx(first(<span className="text-meta mx-1 w-10">a</span>), 1024)).toBeCloseTo(
      15.2,
      9
    );
  });

  it('refuses what it cannot read rather than measuring nothing', () => {
    expect(() => insetsPx(first(<span className="px-3 px-4" />), 1024)).toThrow(/both set/);
    expect(() => insetsPx(first(<span className="flex hidden" />), 1024)).toThrow(/both set/);
    expect(() => boxPx(first(<div className="w-full" />), 1024)).toThrow(/room it is given/);
    expect(() => boxPx(first(<div className="grid" />), 1024)).toThrow(/grid/);
    expect(() => insetsPx(first(<span className="w-[clamp(1rem,2vw,3rem)]" />), 1024)).toThrow(
      /cannot read/
    );
    // app/globals.css: `.tier2-ground` sets its bleed in a media query; `.comment-lg` its gap
    expect(() => insetsPx(first(<div className="tier2-ground" />), 1024)).toThrow(/@media/);
    expect(() => insetsPx(first(<div className="comment-lg" />), 1024)).toThrow(/gap/);
    expect(() => insetsPx(first(<span style={{ padding: 4 }} />), 1024)).toThrow(/inline/);
  });
});

describe("tailwind-box reads the container's content width from the theme", () => {
  it('is 1232 / 976 / 736 / 608 px at 1280 / 1024 / 768 / 640, the window less 32 below', () => {
    expect([1280, 1440, 1024, 1279, 768, 640, 639, 390, 360, 320].map(containerContentPx)).toEqual([
      1232, 1232, 976, 976, 736, 608, 607, 358, 328, 288,
    ]);
  });
});
