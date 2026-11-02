// Consistency lane E (C5, Main board "How an object is read"): a DOB pattern as the cluster page's
// tier 2 draws it — the DNA ruler (one part per gene that owns bytes), the gene table (numbered
// one-byte runs folded into one row) and what the decoded objects show for each gene. Every value
// comes from the API's pattern and trait counts; a range that cannot be drawn exactly throws.
import type { CollectionInsights, CurrentPattern, GeneDefinition } from '@/lib/api';
import { stepGenes, type GeneSource } from '@/lib/dob-declared';
import { traitDisplay } from '@/lib/dob-genes';

const fmt = (n: number) => n.toLocaleString('en-US');

/** A gene of one decoder step: `key` is `<step>:<gene index>`. */
export interface PatternGene {
  key: string;
  gene: GeneDefinition;
  /** Lane E follow-up: the API's reviewed byte map, or the cluster description's declaration. */
  source?: GeneSource;
}

/**
 * Every gene of a DOB pattern, step by step (`<step>:<gene index>`): a mapped step's genes from the
 * API, an unmapped step's from the pattern its cluster's description declares (`stepGenes`).
 */
export function patternGenes(
  pattern: CurrentPattern,
  description: string | null | undefined,
  context: string
): PatternGene[] {
  if (pattern.status !== 'dob') return [];
  return pattern.steps.flatMap((step) => {
    const { source, genes } = stepGenes(step, description, context);
    return genes.map((gene) => ({ key: `${step.step}:${gene.index}`, gene, source }));
  });
}

/** A gene's declared range as exact integers (decimal strings in the API). */
function declared(g: GeneDefinition): { start: bigint; end: bigint } {
  if (!/^(0|[1-9]\d*)$/.test(g.byteOffset) || !/^(0|[1-9]\d*)$/.test(g.byteLength))
    throw new Error(`DOB pattern: gene ${g.name} has an invalid byte range`);
  const start = BigInt(g.byteOffset);
  return { start, end: start + BigInt(g.byteLength) };
}

/** The end of the furthest declared range, when it is a drawable number of bytes. */
export function patternExtent(genes: readonly PatternGene[]): number | null {
  const end = genes.reduce((max, { gene }) => {
    const e = declared(gene).end;
    return e > max ? e : max;
  }, BigInt(0));
  return end <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(end) : null;
}

export interface DnaRulerPart {
  /** The gene that owns these bytes; null for bytes no gene reads. */
  key: string | null;
  start: number;
  end: number;
}

/**
 * The DNA ruler over `total` bytes: every byte belongs to the widest gene that reads it (the
 * sequencing gel's rule), ranges clipped to the DNA; runs of one owner are one part. A gene whose
 * bytes all belong to a wider one has no part and is listed in `inside`, so parts + inside name
 * every gene.
 */
export function dnaRulerParts(
  genes: readonly PatternGene[],
  total: number
): { parts: DnaRulerPart[]; inside: { key: string; name: string; start: number; end: number }[] } {
  if (!Number.isSafeInteger(total) || total < 0)
    throw new Error(`DOB pattern: invalid DNA length ${total}`);
  const ranges = genes.map(({ key, gene }) => {
    const { start, end } = declared(gene);
    const clip = (v: bigint) => Number(v < BigInt(total) ? v : BigInt(total));
    return { key, name: gene.name, start: clip(start), end: clip(end) };
  });
  const owner: (string | null)[] = Array.from({ length: total }, (_, b) => {
    let best: (typeof ranges)[number] | null = null;
    for (const r of ranges)
      if (b >= r.start && b < r.end && (!best || r.end - r.start > best.end - best.start)) best = r;
    return best?.key ?? null;
  });
  const parts: DnaRulerPart[] = [];
  owner.forEach((key, b) => {
    const last = parts.at(-1);
    if (last && last.key === key) last.end = b + 1;
    else parts.push({ key, start: b, end: b + 1 });
  });
  const owning = new Set(owner);
  const inside = ranges
    .filter((r) => !owning.has(r.key) && r.end > r.start)
    .map(({ key, name, start, end }) => ({ key, name, start, end }));
  return { parts, inside };
}

/** "bytes 0–39 · 40" / "byte 63 · 1": the declared span (inclusive) and its length, exactly. */
export function geneSpan(g: GeneDefinition): string {
  const { start, end } = declared(g);
  const length = end - start;
  if (length === BigInt(0)) return 'no bytes · 0';
  return length === BigInt(1)
    ? `byte ${start} · 1`
    : `bytes ${start}–${end - BigInt(1)} · ${g.byteLength}`;
}

/** A rule with its domain: "options · lime, amber, …", "range 0 – 255", "utf8 · String". */
export function ruleWords(g: GeneDefinition): string {
  const args = g.ruleArguments;
  if (g.rule === 'options' && Array.isArray(args) && args.length) {
    const shown = args.slice(0, 3).map((a) => traitDisplay(String(a)));
    return `options · ${shown.join(', ')}${args.length > 3 ? ', …' : ''}`;
  }
  if (
    g.rule === 'range' &&
    Array.isArray(args) &&
    args.length === 2 &&
    args.every((a) => typeof a !== 'object')
  )
    return `range ${String(args[0])} – ${String(args[1])}`;
  return `${g.rule} · ${g.typeTag}`;
}

export interface GeneRow {
  /** The gene, or "_m0 – _m65" for a folded run. */
  label: string;
  genes: PatternGene[];
  span: string;
  rule: string;
}

const NUMBERED = /^(.*?)(\d+)$/;

/**
 * One row per gene, except a run of three or more numbered genes (`_m0`, `_m1`, …) that follow
 * each other byte for byte with one width and one rule: that run is one row ("_m0 – _m65",
 * "bytes 64–129 · 66 × 1"); its genes stay listed in the row.
 */
export function geneRows(genes: readonly PatternGene[]): GeneRow[] {
  const rows: GeneRow[] = [];
  const same = (a: GeneDefinition, b: GeneDefinition) => {
    const [ma, mb] = [a.name.match(NUMBERED), b.name.match(NUMBERED)];
    return (
      !!ma &&
      !!mb &&
      ma[1] === mb[1] &&
      Number(mb[2]) === Number(ma[2]) + 1 &&
      a.byteLength === b.byteLength &&
      declared(b).start === declared(a).end &&
      a.rule === b.rule &&
      a.typeTag === b.typeTag &&
      JSON.stringify(a.ruleArguments) === JSON.stringify(b.ruleArguments)
    );
  };
  let run: PatternGene[] = [];
  const flush = () => {
    if (run.length >= 3) {
      const [first, last] = [run[0].gene, run[run.length - 1].gene];
      rows.push({
        label: `${first.name} – ${last.name}`,
        genes: run,
        span: `bytes ${declared(first).start}–${declared(last).end - BigInt(1)} · ${fmt(run.length)} × ${first.byteLength}`,
        rule: ruleWords(first),
      });
    } else
      for (const g of run)
        rows.push({
          label: g.gene.name,
          genes: [g],
          span: geneSpan(g.gene),
          rule: ruleWords(g.gene),
        });
    run = [];
  };
  for (const g of genes) {
    if (run.length && !same(run[run.length - 1].gene, g.gene)) flush();
    run.push(g);
  }
  flush();
  return rows;
}

/**
 * What the decoded objects show for one gene: one value for all, a value unique to each object,
 * up to three values with their counts, else how many values. Counts are the API's, complete only.
 */
export function foundAcross(name: string, genetics: CollectionInsights['genetics']): string {
  if (!genetics.complete) return 'awaiting decoding';
  const trait = genetics.traits.find((t) => t.name === name);
  if (!trait || !trait.values.length) return 'not among the decoded traits';
  const values = trait.values;
  if (values.length === 1 && values[0].count === genetics.eligibleItems)
    return `${traitDisplay(values[0].value)}, for all`;
  if (values.length === genetics.eligibleItems && values.every((v) => v.count === 1))
    return 'unique to each object';
  if (values.length <= 3)
    return [...values]
      .sort((a, b) => b.count - a.count)
      .map((v) => `${traitDisplay(v.value)} for ${fmt(v.count)}`)
      .join(' · ');
  return `${fmt(values.length)} values`;
}

/**
 * What the decoded objects show for a folded run: the one reading when every gene of the run has
 * it, else how many values each gene takes ("1–2 values each").
 */
export function foundAcrossRun(
  names: readonly string[],
  genetics: CollectionInsights['genetics']
): string {
  const readings = names.map((name) => foundAcross(name, genetics));
  if (readings.every((r) => r === readings[0])) return readings[0];
  const counts = names.map(
    (name) => genetics.traits.find((t) => t.name === name)?.values.length ?? 0
  );
  const [min, max] = [Math.min(...counts), Math.max(...counts)];
  return `${min === max ? fmt(min) : `${fmt(min)}–${fmt(max)}`} values each`;
}
