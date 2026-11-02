import { describe, expect, it } from 'vitest';
import { expectTier1Budget, ownText, tier1ProseBlocks } from '@/__tests__/utils/hierarchy';

const words = (n: number, seed = 'a') => `${seed}${'x'.repeat(n - 1)}`;

function dom(html: string): HTMLElement {
  const root = document.createElement('div');
  root.innerHTML = html;
  return root;
}

describe('ownText (the measurement script rule)', () => {
  it('counts text nodes and inline children, never a block child', () => {
    const el = dom(
      `<div id="b">lead <a href="#">link</a> <em>stress</em><p>a block of its own</p></div>`
    ).querySelector('#b')!;
    expect(ownText(el)).toBe('lead link stress');
  });

  it('reads an SVG beside the text as a block, as the preflight sets it, not as inline text', () => {
    const el = dom(`<div id="c">caption <svg><text>${words(300)}</text></svg></div>`).querySelector(
      '#c'
    )!;
    expect(ownText(el)).toBe('caption');
  });
});

describe('expectTier1Budget', () => {
  it('passes caption-length blocks in tier 1 and ignores tier 2, SVG text and short lines', () => {
    const container = dom(`
      <div data-tier="1">
        <p>${words(150, 'A')}</p>
        <div><span>${words(80)}</span> <span>${words(20)}</span></div>
        <svg><text>${words(400, 'S')}</text></svg>
        <p>${words(160, 'B')}</p>
      </div>
      <div data-tier="2"><p>${words(600, 'E')}</p></div>`);
    expect(tier1ProseBlocks(container).map((b) => b.chars)).toEqual([150, 160]);
    expectTier1Budget(container);
  });

  it('fails on a block longer than the caption budget and names it', () => {
    const container = dom(`<div data-tier="1"><p>Hero paragraph ${words(200)}</p></div>`);
    expect(() => expectTier1Budget(container)).toThrowError(
      /215 chars, budget 160: "Hero paragraph/
    );
  });

  it('fails on too many prose blocks and lists each one by its opening words', () => {
    const container = dom(
      `<div data-tier="1">${['One', 'Two', 'Three', 'Four']
        .map((w) => `<p>${w} ${words(145)}</p>`)
        .join('')}</div>`
    );
    let message = '';
    try {
      expectTier1Budget(container);
    } catch (error) {
      message = String(error);
    }
    expect(message).toMatch(/4 prose blocks in tier 1, budget 3/);
    for (const w of ['One', 'Two', 'Three', 'Four']) expect(message).toContain(`chars: "${w} axx`);
    expect(() => expectTier1Budget(container, { maxProseBlocks: 4, maxChars: 160 })).not.toThrow();
  });

  it('reads the whole container when the page marks no tiers, and counts inline runs together', () => {
    const container = dom(
      `<section><div>${words(90)} <a href="#">${words(60)}</a></div><p>${words(30)}</p></section>`
    );
    expect(tier1ProseBlocks(container)).toHaveLength(1);
    expect(() => expectTier1Budget(container, { maxProseBlocks: 0 })).toThrowError(
      /1 prose blocks in tier 1, budget 0/
    );
  });
});
