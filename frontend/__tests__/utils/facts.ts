/**
 * The facts of a `DetailFacts` list (the detail pages' `dl` rows, components/ui/detail-layout.tsx)
 * as [label, value] pairs, in order, each text with its whitespace collapsed: what a reader reads,
 * a label beside its value. Fails when the element holds no such list, or more than one.
 */
export function factsOf(root: Element): [string, string][] {
  const lists = root.matches('dl') ? [root] : Array.from(root.querySelectorAll('dl'));
  if (lists.length !== 1) throw new Error(`factsOf: ${lists.length} fact lists, expected one`);
  const text = (el: Element | null) => (el?.textContent ?? '').replace(/\s+/g, ' ').trim();
  return Array.from(lists[0].children).map((row) => {
    const term = row.querySelector(':scope > dt');
    const value = row.querySelector(':scope > dd');
    if (!term || !value) throw new Error('factsOf: a row without its dt and dd');
    return [text(term), text(value)];
  });
}
