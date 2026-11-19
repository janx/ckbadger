/**
 * A field a route sends from this page's API on (lane P2's fifth round, item 3): where an API older
 * than this page sends none, the reader refuses it by name, as the chart models do, and never draws
 * a word or a figure that is not on chain ("an undefined lock"; an out point read from undefined).
 */
export function sentField<T>(value: T | undefined, what: string, field: string): T {
  if (value === undefined)
    throw new Error(`${what}: the API sends no ${field}: it is older than this page`);
  return value;
}
