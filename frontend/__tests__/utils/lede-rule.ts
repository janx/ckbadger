// Consistency round C7: an asset page's lede never restates a number of its metrics row.
// A "number string" is a run of at least two characters of digits with their separators
// ("11,866", "2,885,438.81184468", "#13,159,285" → "13,159,285"); single digits ("1 mint", "DOB/0")
// are too common to be a restatement. The lede is the shell's `[data-role="lede"]` and
// `[data-role="intro"]` (the hero's prose); the metrics row is `[data-role="metrics"]`.

const NUMBER = /\d[\d,]*(?:\.\d+)?/g;

export function numberStrings(text: string): string[] {
  return [...new Set((text.match(NUMBER) ?? []).filter((s) => s.length >= 2))];
}

/** The metrics row's number strings that also occur in the lede; `[]` when the rule holds. */
export function ledeRestatements(root: ParentNode): string[] {
  const metrics = root.querySelector('[data-role="metrics"]');
  if (!metrics) throw new Error('lede rule: the page has no metrics row');
  const lede = [...root.querySelectorAll('[data-role="lede"], [data-role="intro"]')]
    .map((el) => el.textContent ?? '')
    .join(' \n ');
  const ledeNumbers = new Set(numberStrings(lede));
  return numberStrings(metrics.textContent ?? '').filter((n) => ledeNumbers.has(n));
}
