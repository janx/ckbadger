/**
 * The one reader of a field this plan added to a route (the third read's La N6, with S5's rule: a
 * page on an API one version older refuses by name or draws truthfully). An older API sends no
 * such field: read as `undefined`, it threw a TypeError, wrote "undefined", or was drawn as no
 * figure at all. Every such field is read here, and its absence refused in one sentence, the chart
 * models' own: "<what>: the API sends no <field>: it is older than this page". React-free: the AI
 * view's renderer reads it too.
 */

/** The sentence of a field an older API does not send. */
export function olderApi(what: string, field: string): Error {
  return new Error(`${what}: the API sends no ${field}: it is older than this page`);
}

/**
 * `record[field]`, which this page's API sends on every record (a `null` included, where the route
 * states none); refused by name where the API does not send it.
 */
export function sentField<T extends object, K extends keyof T & string>(
  record: T,
  field: K,
  what: string
): T[K] {
  const value = (record as Partial<T>)[field];
  if (value === undefined) throw olderApi(what, field);
  return value as T[K];
}
