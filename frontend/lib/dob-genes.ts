// F2 group `objects` (round 2): one reading of a DOB pattern for both gels — the Object page's
// sequencing gel (one object's DNA, gene by gene) and the allele gel (a collection's variants per
// gene). Everything here reads the API's pattern, byte map and trait counts; nothing is guessed.
import type { CollectionInsights, CurrentPattern, ObjectGenetics } from '@/lib/api';
import { hexBytes } from '@/lib/hex-bytes';
import { GENE_COLORS } from '@/lib/design-tokens';

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
export function sequencingGenes(evidence: ObjectGenetics, subject: string): SequencingLayout {
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
  /** Whether this lane's position came from the pattern (false: appended, order unknown). */
  ordered: boolean;
}

const HEX_COLOUR = /^#[0-9a-f]{6}$/i;

/**
 * The allele gel's lanes (OB-4, OB-C2): one per gene in DNA order, each allele a band as thick as
 * its share, common first. Genes the pattern does not order are appended, never dropped.
 */
export function alleleLanes(
  genetics: CollectionInsights['genetics'],
  order: readonly string[] | null,
  subject: string
): AlleleLane[] {
  const names = genetics.traits.map((t) => t.name);
  const ordered = (order ?? []).filter((n) => names.includes(n));
  const rest = names.filter((n) => !ordered.includes(n));
  return [...ordered, ...rest].map((gene) => {
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
      ordered: ordered.includes(gene),
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
