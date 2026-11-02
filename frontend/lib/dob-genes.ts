// F2 group `objects` (round 2): one reading of a DOB pattern for both gels — the Object page's
// sequencing gel (one object's DNA, gene by gene) and the allele gel (a collection's variants per
// gene). Everything here reads the API's pattern, byte map and trait counts; nothing is guessed.
import type { CollectionInsights, CurrentPattern, GeneDefinition, ObjectGenetics } from '@/lib/api';
import { hexBytes } from '@/lib/hex-bytes';
import { GENE_COLORS } from '@/lib/design-tokens';
import { stepGenes } from '@/lib/dob-declared';

export interface DnaTrait {
  name: string;
  value: string;
}

/**
 * Trait names in DNA order: the current pattern's genes by step, then gene index (OB-C2). Steps
 * whose byte map is unsupported contribute nothing; null when no step can be read (not DOB, or no
 * supported step), so the caller can say the order is unknown instead of inventing one.
 */
export function geneOrder(pattern: CurrentPattern): string[] | null {
  if (pattern.status !== 'dob') return null;
  const names: string[] = [];
  for (const step of pattern.steps) {
    if (step.description.status !== 'supported') continue;
    for (const gene of [...step.description.genes].sort((a, b) => a.index - b.index))
      if (!names.includes(gene.name)) names.push(gene.name);
  }
  return names.length ? names : null;
}

/** A rule as the board words it: `options` with one value is "1 option"; others by name. */
export function ruleText(rule: string, args: unknown): string {
  if (rule === 'options' && Array.isArray(args))
    return `${args.length} option${args.length === 1 ? '' : 's'}`;
  return rule;
}

const CONTROL = /[\u0000-\u001f\u007f]/g;
const MEDIA_VALUE = /^(?:btcfs|ipfs|ipns|ar|https?|data):/i;
const SVG_VALUE = /^\s*<svg[\s>]/i;

/** A trait value for people: control bytes dropped, empty named, pointers and markup described. */
export function traitDisplay(value: string): string {
  const v = value.replace(CONTROL, '').trim();
  if (!v) return 'empty';
  if (/^btcfs:\/\//i.test(v)) return `Bitcoin inscription …${v.slice(-6)}`;
  if (SVG_VALUE.test(v)) return `SVG image · ${value.length.toLocaleString('en-US')} characters`;
  return v;
}

/**
 * Whether a trait reads as a property in the hero list (OB-6, OB-16): DOB/1 `prev.*` render
 * hints and media values (a Bitcoin pointer, a URL, SVG source) are not; they stay on the gel.
 */
export function isReadableTrait(name: string, value: string): boolean {
  if (name.startsWith('prev.')) return false;
  const v = value.trim();
  return !MEDIA_VALUE.test(v) && !SVG_VALUE.test(v);
}

export interface SequencingGene {
  /** `step:geneIndex`, unique per gene of the pattern. */
  key: string;
  name: string;
  offset: number;
  length: number;
  /** Board wording of the rule ("btcfs", "1 option"). */
  rule: string;
  /** The gene's bytes of this DNA, hex. */
  raw: string;
  /** Owns at least one byte (the widest gene over a byte owns it); else it reads shared bytes. */
  owns: boolean;
  color: string;
  /** The cached VM trait value(s) for display; null when the VM has given none. */
  value: string | null;
  declared: { offset: string; length: string };
  truncated: boolean;
  ruleName: string;
  ruleArguments: unknown;
  /** Lane E follow-up: read from the description's declared pattern (the API mapped no bytes). */
  source?: 'description';
}
export interface SequencingLayout {
  bytes: string[];
  genes: SequencingGene[];
  /** Per byte: the key of the gene that owns it, or null when no gene covers it. */
  owners: (string | null)[];
  unmapped: { name: string; declared: { offset: string; length: string } }[];
  unsupported: { step: number; reason: string }[];
}

/** One object's DNA mapped gene by gene, coloured for the sequencing gel (OB-7). */
export function sequencingGenes(
  evidence: ObjectGenetics,
  subject: string,
  /**
   * Lane E follow-up, additive: the cluster's stored description; a step the API cannot map reads
   * the pattern it declares (`stepGenes`). `undefined` (the default, or while it loads): such a
   * step reads nothing; `null`: the cluster has no description, which an unmapped step cannot be.
   */
  description?: string | null
): SequencingLayout {
  if (evidence.dnaHex === null) throw new Error(`${subject}: no DNA to sequence`);
  const bytes = hexBytes(evidence.dnaHex, subject);
  const pattern = evidence.currentPattern;
  const genes: SequencingGene[] = [];
  const unmapped: SequencingLayout['unmapped'] = [];
  const unsupported: SequencingLayout['unsupported'] = [];
  if (pattern.status === 'dob')
    for (const step of pattern.steps) {
      if (step.description.status !== 'supported') {
        unsupported.push({ step: step.step, reason: step.description.reason });
        // The declared rows are JSON DOB/0's [name, type, offset, length, rule, args]; a JSON
        // DOB/0 decoder reads each range clipped to the DNA (crates/dob-decoder fixtures README),
        // so the bytes shown are the declared range within this DNA, the clip flagged.
        for (const definition of stepGenes(step, description, subject).genes) {
          const offset = Number(definition.byteOffset);
          const end = Math.min(offset + Number(definition.byteLength), bytes.length);
          const start = Math.min(offset, bytes.length);
          const declared = { offset: definition.byteOffset, length: definition.byteLength };
          if (end === start) {
            unmapped.push({ name: definition.name, declared });
            continue;
          }
          const values = evidence.cachedVmTraits.traits
            .filter((t) => t.name === definition.name)
            .map((t) => traitDisplay(t.value));
          genes.push({
            key: `${step.step}:${definition.index}`,
            name: definition.name,
            offset: start,
            length: end - start,
            rule: ruleText(definition.rule, definition.ruleArguments),
            raw: bytes.slice(start, end).join(''),
            owns: false,
            color: GENE_COLORS.shared,
            value: values.length ? values.join(' · ') : null,
            declared,
            truncated: offset + Number(definition.byteLength) > bytes.length,
            ruleName: definition.rule,
            ruleArguments: definition.ruleArguments,
            source: 'description',
          });
        }
        continue;
      }
      const definitions = step.description.genes;
      for (const map of [...step.bytes].sort((a, b) => a.geneIndex - b.geneIndex)) {
        const definition = definitions.find((g) => g.index === map.geneIndex);
        if (!definition)
          throw new Error(
            `${subject}: decoder step ${step.step} maps missing gene ${map.geneIndex}`
          );
        const raw = map.rawHex.replace(/^0x/, '').toLowerCase();
        if (bytes.slice(map.byteOffset, map.byteOffset + map.byteLength).join('') !== raw)
          throw new Error(`${subject}: gene ${definition.name} bytes disagree with DNA`);
        const declared = { offset: map.declaredByteOffset, length: map.declaredByteLength };
        if (map.byteLength === 0) {
          unmapped.push({ name: definition.name, declared });
          continue;
        }
        const values = evidence.cachedVmTraits.traits
          .filter((t) => t.name === definition.name)
          .map((t) => traitDisplay(t.value));
        genes.push({
          key: `${step.step}:${map.geneIndex}`,
          name: definition.name,
          offset: map.byteOffset,
          length: map.byteLength,
          rule: ruleText(definition.rule, definition.ruleArguments),
          raw,
          owns: false,
          color: GENE_COLORS.shared,
          value: values.length ? values.join(' · ') : null,
          declared,
          truncated: map.truncated,
          ruleName: definition.rule,
          ruleArguments: definition.ruleArguments,
        });
      }
    }
  const owners = bytes.map((_, b) => {
    let owner: SequencingGene | null = null;
    for (const g of genes)
      if (b >= g.offset && b < g.offset + g.length && (!owner || g.length > owner.length))
        owner = g;
    return owner?.key ?? null;
  });
  let next = 0;
  for (const g of genes) {
    g.owns = owners.includes(g.key);
    if (!g.owns) continue;
    g.color = /^btcfs/i.test(g.ruleName)
      ? GENE_COLORS.bitcoin
      : GENE_COLORS.cycle[next++ % GENE_COLORS.cycle.length];
  }
  return { bytes, genes, owners, unmapped, unsupported };
}

export interface AlleleBand {
  value: string;
  /** The value as text on the gel. */
  label: string;
  count: number;
  /** Share of the lane (all bands of a lane sum to 1). */
  share: number;
  isColour: boolean;
  colour: string | null;
}
export interface AlleleLane {
  gene: string;
  bands: AlleleBand[];
  variants: number;
  /** One variant only: every object shares it, so the lane is dimmed. */
  conserved: boolean;
  /** Whether this lane's position came from the pattern (false: no readable pattern). */
  ordered: boolean;
}

const HEX_COLOUR = /^#[0-9a-f]{6}$/i;

/**
 * Traits the decoder outputs that the readable pattern does not declare as genes — a DOB/1
 * renderer's IMAGE, the rendered picture. Empty when no pattern step is readable (`order` null):
 * then the page cannot tell a gene from an output, and every trait is a lane.
 */
export function traitsOutsidePattern(
  genetics: CollectionInsights['genetics'],
  order: readonly string[] | null
): string[] {
  if (order === null) return [];
  return genetics.traits.map((t) => t.name).filter((name) => !order.includes(name));
}

/**
 * The allele gel's lanes (OB-4, OB-C2): one per gene in DNA order, each allele a band as thick as
 * its share, common first. When the pattern is readable its genes are the lanes, so the gene count
 * is the pattern's (V2: a renderer's IMAGE is an output, not a 73rd gene; `traitsOutsidePattern`
 * names such traits for the page to mention). With no readable pattern every trait is a lane, in
 * the API's order.
 */
export function alleleLanes(
  genetics: CollectionInsights['genetics'],
  order: readonly string[] | null,
  subject: string
): AlleleLane[] {
  const names = genetics.traits.map((t) => t.name);
  const lanes = order === null ? names : order.filter((n) => names.includes(n));
  return lanes.map((gene) => {
    const trait = genetics.traits.find((t) => t.name === gene)!;
    let sum = 0;
    for (const v of trait.values) {
      if (!Number.isSafeInteger(v.count) || v.count <= 0)
        throw new Error(
          `${subject}: gene ${gene} variant ${JSON.stringify(v.value)} count ${v.count}`
        );
      sum += v.count;
    }
    if (sum === 0) throw new Error(`${subject}: gene ${gene} has no counted variants`);
    const bands = [...trait.values]
      .sort((a, b) => (b.count === a.count ? a.value.localeCompare(b.value) : b.count - a.count))
      .map((v) => {
        const isColour = HEX_COLOUR.test(v.value);
        return {
          value: v.value,
          label: traitDisplay(v.value),
          count: v.count,
          share: v.count / sum,
          isColour,
          colour: isColour ? v.value : null,
        };
      });
    return {
      gene,
      bands,
      variants: bands.length,
      conserved: bands.length === 1,
      ordered: order !== null,
    };
  });
}

/** Where one object sits on the gel: its band in every lane it carries (genotype overlay). */
export function genotypePoints(
  lanes: readonly AlleleLane[],
  traits: readonly DnaTrait[]
): { points: { lane: number; band: number }[]; absent: string[] } {
  const points: { lane: number; band: number }[] = [];
  const absent: string[] = [];
  lanes.forEach((lane, i) => {
    const own = traits.find((t) => t.name === lane.gene);
    if (!own) return;
    const band = lane.bands.findIndex((b) => b.value === own.value);
    if (band < 0) absent.push(lane.gene);
    else points.push({ lane: i, band });
  });
  return { points, absent };
}

// ── Consistency round C9: which genes get a lane on a gel ──
/** Known DOB/1 render-hint names read as words; every other gene keeps its pattern name. */
const GENE_LABELS: Readonly<Record<string, string>> = {
  'prev.type': 'Type',
  'prev.bg': 'Background',
  'prev.bgcolor': 'Background colour',
  'prev.view': 'View',
};
/** A gene as people read it: a known DOB/1 name as a word, else the pattern's own name. */
export function geneLabel(gene: string): string {
  return GENE_LABELS[gene] ?? gene;
}
/** The URL key a gene's filter is written under (C15): the word when one is known. */
export function geneUrlKey(gene: string): string {
  const label = GENE_LABELS[gene];
  return label ? label.toLowerCase().replace(/\s+/g, '-') : gene;
}

export interface DroppedGene {
  gene: string;
  label: string;
  /** `raw`: a raw-rule gene; `single`: one value for every object; `unique`: ≥ 90 % variants. */
  reason: 'raw' | 'single' | 'unique';
  variants: number;
  /** The one value every object carries (`single`). */
  value?: string;
  /** The pattern's type tag (`raw`: a raw `number` or a raw `string`). */
  typeTag?: string;
}

/** Variants on at least this share of the objects make a gene too unique to draw as a lane. */
const UNIQUE_SHARE = 0.9;

/**
 * C9: a gene with the `raw` rule, a single value, or variants on ≥ 90 % of the objects gets no
 * lane (it cannot sort objects into bands); the others keep their lanes in DNA order.
 */
export function laneRule(
  lanes: readonly AlleleLane[],
  pattern: CurrentPattern,
  objects: number
): { lanes: AlleleLane[]; dropped: DroppedGene[] } {
  if (!Number.isSafeInteger(objects) || objects <= 0)
    throw new Error(`gel lane rule needs a positive object count, got ${objects}`);
  const defs = new Map<string, GeneDefinition>();
  if (pattern.status === 'dob')
    for (const step of pattern.steps)
      if (step.description.status === 'supported')
        for (const g of step.description.genes) if (!defs.has(g.name)) defs.set(g.name, g);
  const kept: AlleleLane[] = [];
  const dropped: DroppedGene[] = [];
  for (const lane of lanes) {
    const def = defs.get(lane.gene);
    const base = { gene: lane.gene, label: geneLabel(lane.gene), variants: lane.variants };
    if (def?.rule === 'raw') dropped.push({ ...base, reason: 'raw', typeTag: def.typeTag });
    else if (lane.variants === 1)
      dropped.push({ ...base, reason: 'single', value: lane.bands[0].label });
    else if (lane.variants >= UNIQUE_SHARE * objects) dropped.push({ ...base, reason: 'unique' });
    else kept.push(lane);
  }
  return { lanes: kept, dropped };
}

const andList = (items: readonly string[]) =>
  items.length <= 1 ? items.join('') : `${items.slice(0, -1).join(', ')} and ${items.at(-1)}`;

/** The one caption under a gel that names the genes without a lane, and why (C9). */
export function droppedGenesCaption(dropped: readonly DroppedGene[], objects: number): string {
  if (dropped.length === 0) return '';
  const n = (v: number) => v.toLocaleString('en-US');
  const singles = dropped.filter((d) => d.reason === 'single');
  const clauses: string[] = [];
  if (singles.length)
    clauses.push(`${andList(singles.map((d) => `${d.label} is ${d.value}`))} for every object`);
  for (const d of dropped) {
    if (d.reason === 'raw')
      clauses.push(
        `${d.label} is a raw ${d.typeTag === 'string' ? 'string' : 'number'}${d.variants === objects ? ' unique to each' : ` with ${n(d.variants)} variants`}`
      );
    if (d.reason === 'unique')
      clauses.push(
        d.variants === objects
          ? `${d.label} is unique to each object`
          : `${d.label} differs for almost every object (${n(d.variants)} variants among ${n(objects)})`
      );
  }
  const tail =
    dropped.length === 1
      ? 'so it gets no lane'
      : dropped.length === 2
        ? 'so neither gets a lane'
        : 'so none of them gets a lane';
  return `${andList(clauses)}, ${tail}.`;
}
