/**
 * Consistency round C10 — "one hint per section", as the gate measures it
 * (`survey-extra.cjs` `hintsPerSection`): for every `section` / `[role="region"]` under `root`, the
 * number of text nodes that say "tap" or "hover". A nested section counts for its parents too, so
 * a parent holding two one-hint sections fails as the survey does.
 */
export function hintsPerSection(root: ParentNode): { name: string; hints: string[] }[] {
  return [...root.querySelectorAll<HTMLElement>('section, [role="region"]')].map((section) => {
    const walker = document.createTreeWalker(section, NodeFilter.SHOW_TEXT);
    const hints: string[] = [];
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const t = (n.nodeValue ?? '').replace(/\s+/g, ' ').trim();
      if (t && /\b(tap|hover)\b/i.test(t)) hints.push(t);
    }
    const name =
      section.getAttribute('aria-label') ??
      section.querySelector('h2,h3')?.textContent ??
      section.tagName.toLowerCase();
    return { name, hints };
  });
}

/** The sections that say "tap or hover" more than once (the gate wants none). */
export function sectionsWithManyHints(root: ParentNode) {
  return hintsPerSection(root).filter((s) => s.hints.length > 1);
}
