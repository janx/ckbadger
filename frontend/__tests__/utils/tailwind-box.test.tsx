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
  targetHeightPx,
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
    // a state that sets no width is no layout at rest; re-pinned on purpose (review fb-tests
    // SHOULD-2, T2): one that changes a width under the state is refused (it read `hover:px-4` as
    // no layout), below
    expect(
      insetsPx(first(<span className="hover:text-jade px-1 focus-visible:underline" />), 1024).box
    ).toBe(8);
    // an arbitrary breakpoint, as a screen
    const narrow = first(<span className="max-[600px]:px-2 min-[900px]:px-4" />);
    expect([599, 600, 899, 900].map((v) => insetsPx(narrow, v).box)).toEqual([16, 0, 0, 32]);
  });

  // Review fb-tests SHOULD-2 (lane rd/fb-fixes-2, T2): a structural variant is read where the
  // element is what it names (every table cell carries `last:pr-0`)
  it('reads first: and last: by where the element stands among its parent’s', () => {
    const row = first(
      <div>
        <span className="pr-4 last:pr-0" />
        <span className="pr-4 last:pr-0" />
      </div>
    );
    const [a, b] = Array.from(row.children);
    expect([insetsPx(a, 1024).box, insetsPx(b, 1024).box]).toEqual([16, 0]);
  });

  it('reads Tailwind’s container from the theme: the room it is given, its gutters', () => {
    const main = first(<main className="container mx-auto" />);
    expect([1280, 1024, 768, 390].map((v) => insetsPx(main, v).box)).toEqual([48, 48, 32, 32]);
    expect(() => boxPx(main, 1280)).toThrow(/room it is given/);
  });

  it('reads a ch length, a size, and a flex column as its widest item', () => {
    // 1ch is the "0" of the monospaced face, 0.6 em: 7.8 px at 13 px
    expect(insetsPx(first(<span className="text-small pl-[2ch]" />), 1024).box).toBeCloseTo(
      15.6,
      9
    );
    expect(boxPx(first(<span className="flex size-6" />), 1024)).toBe(24);
    const column = first(
      <div className="text-meta flex flex-col gap-2">
        <span>abcd</span>
        <span>ab</span>
      </div>
    );
    expect(contentPx(column, 1024)).toBeCloseTo(4 * 7.2, 9);
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

  // Review fb-tests SHOULD-2 (lane rd/fb-fixes-2, T2): what it skipped silently it refuses, naming
  // the class. Each case passed before as no layout.
  it('refuses a variant it cannot place, a class it does not know and a var() value', () => {
    const at = (className: string) => () => insetsPx(first(<span className={className} />), 1024);
    // a variant that is not a screen: a state that changes a width, a selector, an unknown one
    expect(at('hover:px-4')).toThrow('hover:px-4 changes a width under the state hover:');
    expect(at('group-hover:ml-2')).toThrow('group-hover:ml-2 changes a width');
    expect(at('*:px-2')).toThrow('cannot read the variant *: of *:px-2');
    expect(at('[&>a]:px-3')).toThrow('cannot read the variant [&>a]: of [&>a]:px-3');
    expect(at('before:px-3')).toThrow('before:px-3 sets a width of the before box');
    // a utility it does not classify
    expect(at('aspect-square')).toThrow('cannot read aspect-square');
    expect(at('float-left')).toThrow('cannot read float-left');
    // an arbitrary value written with var()
    expect(at('px-[var(--gap)]')).toThrow('cannot read [var(--gap)] as a padding');
    expect(at('w-[var(--w)]')).toThrow('cannot read [var(--w)] as a width');
    expect(at('border-l-[var(--w)]')).toThrow('cannot read [var(--w)] as a borderWidth');
    // an unknown key of a size scale
    expect(at('px-banana')).toThrow('no padding banana in the theme');
  });
});

// The controls lane (C1): a control's hit area is a height, read from the same theme (h-7 is 28 px,
// h-8 32, min-h-8 32, a [18px] box 18) and refused where the classes do not set it.
describe('tailwind-box reads a target’s height from its classes', () => {
  it('reads a height and a minimum off the spacing scale, by breakpoint', () => {
    expect(targetHeightPx(first(<button className="h-7" />), 1280)).toBe(28);
    // mobile first, as the width reader reads a breakpoint: 32 px, 28 from sm
    const box = first(<button className="h-8 px-2.5 sm:h-7" />);
    expect([1280, 640, 639, 390, 360].map((w) => targetHeightPx(box, w))).toEqual([
      28, 28, 32, 32, 32,
    ]);
    expect(targetHeightPx(first(<a className="min-h-8 py-1" />), 390)).toBe(32);
    expect(targetHeightPx(first(<a className="h-6 min-h-8" />), 390)).toBe(32);
    expect(targetHeightPx(first(<a className="h-[18px]" />), 1280)).toBe(18);
    // a state or a pseudo box that sets no height leaves it alone
    expect(
      targetHeightPx(first(<a className="hover:text-jade h-7 before:content-['']" />), 1280)
    ).toBe(28);
  });

  it('adds the reach of a pseudo box drawn absolute over the element', () => {
    const mark = first(
      <button className="relative h-[18px] w-[18px] before:absolute before:-inset-[7px] before:content-['']" />
    );
    expect(targetHeightPx(mark, 1280)).toBe(32);
    expect(
      targetHeightPx(
        first(
          <button className="relative h-6 after:absolute after:-inset-y-1 after:content-['']" />
        ),
        1280
      )
    ).toBe(32);
  });

  it('refuses a height it cannot read', () => {
    const at =
      (className: string, viewport = 1280) =>
      () =>
        targetHeightPx(first(<button className={className} />), viewport);
    expect(at('px-2')).toThrow('sets no height of its own');
    expect(at('h-full')).toThrow('cannot read the length "100%"');
    expect(at('h-auto')).toThrow('cannot read the length "auto"');
    expect(at('h-[var(--h)]')).toThrow('cannot read [var(--h)] as a height');
    expect(at('h-7 h-8')).toThrow('h-7 and h-8 both set height');
    // a max- breakpoint is placed as no breakpoint, as the width reader places it
    expect(at('h-7 max-sm:h-8', 390)).toThrow('h-7 and max-sm:h-8 both set height');
    expect(at('h-7 hover:h-8')).toThrow('hover:h-8 sets a height under a state');
    expect(at('h-7 [&>a]:h-8')).toThrow('cannot read the variant [&>a]: of [&>a]:h-8');
    expect(at('h-7 max-h-6')).toThrow('max-h-6 bounds a height, which is not read');
    expect(at('h-6 before:absolute before:-inset-2')).toThrow('not relative');
    expect(at('relative h-6 before:absolute before:-top-2')).toThrow(
      'before:-top-2 sets an inset of the before box, which is not read'
    );
    expect(at('relative h-6 before:h-10')).toThrow(
      'before:h-10 sets a height of the before box, which is not read'
    );
  });
});

describe("tailwind-box reads the container's content width from the theme", () => {
  it('is 1232 / 976 / 736 / 608 px at 1280 / 1024 / 768 / 640, the window less 32 below', () => {
    expect([1280, 1440, 1024, 1279, 768, 640, 639, 390, 360, 320].map(containerContentPx)).toEqual([
      1232, 1232, 976, 976, 736, 608, 607, 358, 328, 288,
    ]);
  });
});
