import type { MatcherFunction } from '@testing-library/react';

const norm = (s: string) => s.replace(/\s+/g, ' ').trim();

/**
 * A text matcher for a sentence that carries units (A4 round 2): a date, a shortened id or a count
 * is a span of its own (components/ui/units.tsx), so the sentence is no longer one text node and
 * Testing Library's default matcher, which reads an element's own text nodes, cannot find it. This
 * matches the innermost element whose whole text — its descendants' included, white space
 * collapsed, a no-break space as a space — is `text`, or matches it.
 */
export function fullText(text: string | RegExp): MatcherFunction {
  const hit = (el: Element | null | undefined) => {
    if (!el) return false;
    const whole = norm(el.textContent ?? '');
    return typeof text === 'string' ? whole === norm(text) : text.test(whole);
  };
  return (_content, element) => hit(element) && ![...(element?.children ?? [])].some(hit);
}
