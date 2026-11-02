import { expect } from 'vitest';

/**
 * Tier-1 prose budget (hierarchy plan 2026-11-02, §0 and §4): a topic page's first tier carries
 * at most a few paragraph-shaped blocks, each one caption-length. A block is paragraph-shaped when
 * its own text is at least {@link PROSE_MIN} characters — the rule of the browser measurement
 * script `measure-hierarchy.cjs` (`proseTier1`), so a page test and the gate count alike.
 *
 * jsdom applies no stylesheet, so an element's display is its tag's default. Tailwind's preflight
 * makes `svg`, `img`, `canvas` and the other replaced elements blocks, and that is mirrored here so a
 * chart's tick labels never count as its parent's text. A `<span>` styled `block` still reads as
 * inline here and its text counts toward its parent: that only makes the budget stricter than the
 * browser's count, never looser.
 */

/** A block whose own text reaches this many characters is paragraph-shaped (prose). */
export const PROSE_MIN = 140;

/** The elements the measurement script considers as text blocks. */
const BLOCKS = 'p, li, dd, dt, span, div, td, th';

/** Tailwind preflight sets these to `display: block`; jsdom, without the stylesheet, does not. */
const PREFLIGHT_BLOCK = new Set([
  'IMG',
  'SVG',
  'VIDEO',
  'CANVAS',
  'AUDIO',
  'IFRAME',
  'EMBED',
  'OBJECT',
]);

/** jsdom leaves `display` empty where no default rule sets it; the CSS initial value is inline. */
const isInline = (el: Element) =>
  !PREFLIGHT_BLOCK.has(el.tagName.toUpperCase()) &&
  (getComputedStyle(el).display || 'inline').startsWith('inline');

export interface ProseBlock {
  chars: number;
  text: string;
}

/** An element's own text: its text nodes and its inline children's text, whitespace collapsed. */
export function ownText(el: Element): string {
  return Array.from(el.childNodes)
    .filter(
      (c) =>
        c.nodeType === Node.TEXT_NODE ||
        (c.nodeType === Node.ELEMENT_NODE && isInline(c as Element))
    )
    .map((c) => c.textContent ?? '')
    .join('')
    .replace(/\s+/g, ' ')
    .trim();
}

/** Paragraph-shaped blocks of tier 1: inside `[data-tier="1"]`, or the whole container without tiers. */
export function tier1ProseBlocks(container: Element): ProseBlock[] {
  const tier1 = Array.from(container.querySelectorAll('[data-tier="1"]'));
  const roots = tier1.length > 0 ? tier1 : [container];
  const seen = new Set<Element>();
  const blocks: ProseBlock[] = [];
  for (const root of roots) {
    const candidates = [...(root.matches(BLOCKS) ? [root] : []), ...root.querySelectorAll(BLOCKS)];
    for (const el of candidates) {
      if (seen.has(el)) continue;
      seen.add(el);
      if (el.closest('svg, [data-tier="2"], [hidden]')) continue;
      const text = ownText(el);
      if (text.length >= PROSE_MIN) blocks.push({ chars: text.length, text });
    }
  }
  return blocks;
}

/**
 * Fails when tier 1 holds more than `maxProseBlocks` paragraph-shaped blocks or any one of them is
 * longer than `maxChars`; the message lists every prose block with its length and opening words.
 */
export function expectTier1Budget(
  container: Element,
  { maxProseBlocks = 3, maxChars = 160 }: { maxProseBlocks?: number; maxChars?: number } = {}
): void {
  const blocks = tier1ProseBlocks(container);
  const violations = [
    ...(blocks.length > maxProseBlocks
      ? [`${blocks.length} prose blocks in tier 1, budget ${maxProseBlocks}`]
      : []),
    ...blocks
      .filter((b) => b.chars > maxChars)
      .map((b) => `${b.chars} chars, budget ${maxChars}: "${b.text.slice(0, 60)}…"`),
  ];
  const listing = blocks.map((b) => `  ${b.chars} chars: "${b.text.slice(0, 60)}…"`).join('\n');
  expect(
    violations,
    `tier-1 prose budget exceeded: ${violations.join('; ')}\nprose blocks in tier 1:\n${listing}`
  ).toEqual([]);
}
