/**
 * The facts of a `DetailFacts` list (the detail pages' `dl` rows, components/ui/detail-layout.tsx)
 * as [label, value] pairs, in order, each text with its whitespace collapsed: what a reader reads,
 * a label beside its value; a reserved row (unseen, hidden from assistive technology) is not read.
 * Fails when the element holds no such list, or more than one.
 */
export function factsOf(root: Element): [string, string][] {
  const lists = root.matches('dl') ? [root] : Array.from(root.querySelectorAll('dl'));
  if (lists.length !== 1) throw new Error(`factsOf: ${lists.length} fact lists, expected one`);
  const text = (el: Element | null) => (el?.textContent ?? '').replace(/\s+/g, ' ').trim();
  const read = Array.from(lists[0].children).filter(
    (row) => row.getAttribute('aria-hidden') !== 'true'
  );
  return read.map((row) => {
    const term = row.querySelector(':scope > dt');
    const value = row.querySelector(':scope > dd');
    if (!term || !value) throw new Error('factsOf: a row without its dt and dd');
    return [text(term), text(value)];
  });
}

/**
 * The rows of a fact list as its grid lays them out: one class list per row (a reserved row's
 * `invisible` set aside, since it takes its room unseen), the rows below md (one column) and from md
 * (two). Two readings with the same rows stand at the same height at every width, each row being
 * one line of a label and one of a value (jsdom lays nothing out; this is the arithmetic).
 */
export function factRows(root: Element): { rows: string[]; belowMd: number; fromMd: number } {
  const lists = root.matches('dl') ? [root] : Array.from(root.querySelectorAll('dl'));
  if (lists.length !== 1) throw new Error(`factRows: ${lists.length} fact lists, expected one`);
  const dl = lists[0];
  if (!dl.classList.contains('grid-cols-1') || !dl.classList.contains('md:grid-cols-2'))
    throw new Error(
      `factRows: the list is not one column below md and two from it: ${dl.className}`
    );
  const rows = Array.from(dl.children).map((row) =>
    Array.from(row.classList)
      .filter((c) => c !== 'invisible')
      .join(' ')
  );
  return { rows, belowMd: rows.length, fromMd: Math.ceil(rows.length / 2) };
}
