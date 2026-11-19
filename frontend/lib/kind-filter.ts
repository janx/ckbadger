import type { FilterValue } from '@/components/ui/filter-chips';
import { CLASS_KEYS, DATA_KEY } from '@/lib/design-tokens';
import { color, type PaletteKey } from '@/lib/palette';

/**
 * The kind filter (UI review 2026-11-14, PA-15: the same filter is the same control on sibling
 * pages): the activities stream and the transactions list filter by kind with one set of words and
 * marks, the controls board's (gate D0′) — lowercase words, a kind's 7 px mark in its colour:
 * celadon for a CKB transfer, the slate of a script call, the dim slate of a cellbase, the class
 * colours of the DAO, tokens, objects and identities; "all" and the protocols carry none. Each list
 * names the kinds it filters by, in its order, and the filter chips draw them (a select after
 * "kind" on a phone, where they do not fit its line).
 */
const KINDS = {
  all: { label: 'all' },
  ckb: { label: 'ckb transfers', mark: DATA_KEY },
  script: { label: 'script calls', mark: CLASS_KEYS.ckb },
  cellbase: { label: 'cellbase', mark: CLASS_KEYS.other },
  dao: { label: 'nervos dao', mark: CLASS_KEYS.dao },
  token: { label: 'tokens', mark: CLASS_KEYS.tokens },
  object: { label: 'objects', mark: CLASS_KEYS.objects },
  identity: { label: 'identities', mark: CLASS_KEYS.identities },
  protocol: { label: 'protocols' },
} as const satisfies Record<string, { label: string; mark?: PaletteKey }>;

/** A kind a list can filter by. */
export type FilterKind = keyof typeof KINDS;

/** A kind's mark, where it has one. */
function markOf(kind: FilterKind): PaletteKey | undefined {
  const entry: { label: string; mark?: PaletteKey } = KINDS[kind];
  return entry.mark;
}

/** The filter's name, the label its phone select stands after. */
export const KIND_FILTER_LABEL = 'kind';

/**
 * A list's kind filter values: its kinds in its order, each in the filter's words with its mark,
 * and with its count where the list has one (yesterday's, on both lists).
 */
export function kindFilterValues<K extends FilterKind>(
  kinds: readonly K[],
  counts: Partial<Record<K, number>>
): FilterValue<K>[] {
  return kinds.map((key) => ({
    key,
    label: KINDS[key].label,
    mark: markOf(key),
    count: counts[key],
  }));
}

/** A kind in the filter's words: a figure of the kinds names its parts by them. */
export function kindWords(kind: FilterKind): string {
  return KINDS[kind].label;
}

/**
 * A kind's colour in the figure of yesterday's kinds under the filter (a bar of their shares): its
 * mark's colour; a kind with no mark, the slate of the other scripts, as the figure drew it.
 */
export function kindColor(kind: FilterKind): string {
  return color(markOf(kind) ?? CLASS_KEYS.other);
}
