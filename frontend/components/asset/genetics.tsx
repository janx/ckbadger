'use client';
import { WIDEST_DNA_SPAN } from '@/lib/list-bounds';
import { Fragment, useState, type CSSProperties, type ReactNode } from 'react';
import type {
  CollectionInsights,
  CurrentPattern,
  ObjectGenetics,
  SporeDobDecoded,
} from '@/lib/api';
import { apiBaseFor, resolveActiveNetwork } from '@/lib/active-network';
import { AlleleGel, NotGenesNote } from '@/components/ui/allele-gel';
import { SectionDisclosure } from '@/components/ui/section-disclosure';
import { SequencingGel } from '@/components/ui/sequencing-gel';
import { TerminalPanel, TerminalPanelContent } from '@/components/ui/terminal-panel';
import { DnaStrip } from '@/components/ui/dna-strip';
import { AppLink } from '@/components/ui/app-link';
import { PromptHeading } from '@/components/ui/prompt-heading';
import { useDetailTier } from '@/components/ui/detail-layout';
import { ByteScaleMap } from '@/components/ui/hex-map';
import { ReadoutRow } from '@/components/ui/readout-row';
import { CELL_GAP_PX, ListTable } from '@/components/ui/list-table';
import { columnPx, fontPx, textPx } from '@/lib/type-metrics';
import { PhoneRow } from '@/components/ui/phone-row';
import { usePinnedReadout } from '@/hooks/usePinnedReadout';
import { hexBytes } from '@/lib/hex-bytes';
import { COMPOSITION_BADGES, GENE_COLORS } from '@/lib/design-tokens';
import type { PaletteKey } from '@/lib/palette';
import {
  alleleLanes,
  geneLabel,
  geneOrder,
  genotypePoints,
  laneRule,
  sequencingGenes,
  traitDisplay,
  traitsOutsidePattern,
} from '@/lib/dob-genes';
import {
  dnaRulerParts,
  foundAcross,
  foundAcrossRun,
  geneRows,
  geneSpan,
  patternExtent,
  patternGenes,
  ruleWords,
  type GeneRow,
  type PatternGene,
} from '@/lib/dob-pattern';
import type { CompositionTier } from '@/lib/asset-utils';
import { cn, formatUnixSecondsUtc8 } from '@/lib/utils';
import { DroppedGenesCaption } from '@/components/asset/dropped-genes-caption';
import { TraitText } from '@/components/asset/trait-text';
import { formatCount, countNoun } from '@/lib/format-count';
import { ShortId } from '@/components/ui/short-id';
import { unitText } from '@/components/ui/units';

/**
 * The gene tables' columns (readability F3-e), the 13 px cells at 0.6 em a glyph and the 16 px
 * gap after every column but the last. A gene's name, its rule and what an object shows are
 * pattern text that ends in the one ellipsis (the row's title holds it): each holds 12 characters
 * at its minimum, the last column is the flexible one and takes what the container gives beyond
 * the sum. A byte range is whole: a DNA under 1,000 bytes, "bytes 999–999" (WIDEST_DNA_SPAN).
 */
const GENE_TEXT_CHARS = 12;
const geneText = (last = false) =>
  columnPx(textPx(GENE_TEXT_CHARS, fontPx('small')) + (last ? 0 : CELL_GAP_PX));
const geneSpanPx = (text: string) => columnPx(textPx(text.length, fontPx('small')) + CELL_GAP_PX);

// ── F2 objects (round 2): the Object board's DNA section (OB-4, OB-5, OB-7, OB-8) ──
export interface ObjectGeneticsViewProps {
  evidence: ObjectGenetics;
  subject: string;
  /** The object's collection insights, for the allele-gel bridge ("Among <collection>"). */
  collection?: CollectionInsights;
  collectionName?: string | null;
  collectionHref?: string;
  tier?: CompositionTier;
  /** The gene lit across trait list, sequencing gel and allele gel (three-way hover). */
  active?: string | null;
  onActive?: (gene: string | null) => void;
  /** Lane E follow-up: the cluster's description, for a pattern it declares (`sequencingGenes`). */
  description?: string | null;
}
function plateStyle(tier: CompositionTier | undefined): CSSProperties | undefined {
  if (tier !== 'btc_ckb' && tier !== 'pure_ckb') return undefined;
  return {
    backgroundImage: COMPOSITION_BADGES[tier].frame,
    backgroundClip: 'padding-box, border-box',
    backgroundOrigin: 'padding-box, border-box',
    border: '1px solid transparent',
  };
}
/**
 * The colour a gel is drawn on inside its plate: the copper plate's inner fill, else the instrument
 * frame's glass, which is the surface in both themes. A band's label is inked for it.
 */
function plateGround(tier: CompositionTier | undefined): PaletteKey {
  if (tier === 'btc_ckb') return 'badge-inner-btc';
  if (tier === 'pure_ckb') return 'badge-inner-pure';
  return 'base-surface';
}
/**
 * The plate a gel sits on. A BTC+CKB or pure-CKB object keeps its copper plate (the object pages'
 * copper is not touched); any other gel sits in the one instrument frame (Midnight, continued,
 * move 3). `className` lays out the gel inside either.
 */
function GelPlate({
  tier,
  className,
  children,
}: {
  tier: CompositionTier | undefined;
  className: string;
  children: ReactNode;
}) {
  const copper = plateStyle(tier);
  if (copper)
    return (
      <div data-gel-plate="copper" className="p-[5px]" style={copper}>
        <div className={className}>{children}</div>
      </div>
    );
  return (
    <TerminalPanel data-gel-plate="frame">
      <TerminalPanelContent padding="none" className={className}>
        {children}
      </TerminalPanelContent>
    </TerminalPanel>
  );
}
/**
 * What the DNA section says in words, from its sequencing layout: how many bytes of DNA, and the
 * gene that points to a Bitcoin inscription when that is what makes the object BTC+CKB.
 */
function dnaWords(
  evidence: ObjectGenetics,
  subject: string,
  tier: CompositionTier | undefined,
  description: string | null | undefined
): { bytes: number; pointer: string | null } {
  const layout = sequencingGenes(evidence, subject, description);
  const pointer = layout.genes.find((g) => g.owns && /^btcfs/i.test(g.ruleName));
  return {
    bytes: layout.bytes.length,
    pointer: pointer && tier === 'btc_ckb' ? pointer.name : null,
  };
}

/** Tier 2 (hierarchy v3, F4): the DNA section's full explanation, which tier 1 keeps as a caption. */
export function DnaParagraph({
  evidence,
  subject,
  tier,
  description,
}: {
  evidence: ObjectGenetics;
  subject: string;
  tier?: CompositionTier;
  /** Lane E follow-up: the cluster's description, for a pattern it declares (`sequencingGenes`). */
  description?: string | null;
}) {
  if (evidence.dnaHex === null) return null;
  const { bytes, pointer } = dnaWords(evidence, subject, tier, description);
  // Set in the object page's "in words" Prose: the paragraph carries no classes of its own.
  return (
    <p>
      The object’s traits are not stored as text. It carries {unitText(formatCount(bytes, 'byte'))}{' '}
      of DNA, and the collection’s decoder reads each gene to express a trait.
      {pointer
        ? ` Its ${pointer} gene is a pointer to a Bitcoin inscription; that is what makes it BTC+CKB.`
        : ''}
    </p>
  );
}

/**
 * The object's DNA in tier 1: what its traits are read from, in one caption, then the allele gel of
 * its collection with this object's genotype drawn across it ("this one"): its variant in each
 * gene and how common it is. Lane P2's follow-up (item 1): the byte-level plot, the sequencing gel
 * of its own bytes gene by gene, stands in one place, the evidence tier (`ObjectGeneticsEvidence`),
 * beside the byte map and the gene table; a trait's hover lights its gene's lane here and its
 * gene in the gel there.
 */
export function ObjectGeneticsView({
  evidence,
  subject,
  collection,
  collectionName,
  collectionHref,
  tier,
  active: controlled,
  onActive,
  description,
}: ObjectGeneticsViewProps) {
  const [own, setOwn] = useState<string | null>(null);
  const active = controlled === undefined ? own : controlled;
  const setActive = (gene: string | null) => {
    setOwn(gene);
    onActive?.(gene);
  };
  const { dnaHex, cachedVmTraits: cached } = evidence;
  if (dnaHex === null) return <p className="text-text text-small">This object has no DOB DNA.</p>;
  const layout = sequencingGenes(evidence, subject, description);
  const geneNames = [...new Set(layout.genes.map((g) => g.name))];
  const words = dnaWords(evidence, subject, tier, description);
  // C9: on the object page too, only the genes that vary among the collection get a lane.
  const ruled =
    collection?.genetics.complete && collection.genetics.traits.length
      ? laneRule(
          alleleLanes(
            collection.genetics,
            geneOrder(collection.currentPattern),
            collectionName ?? subject
          ),
          collection.currentPattern,
          collection.genetics.eligibleItems
        )
      : null;
  const bridge = ruled && ruled.lanes.length ? ruled.lanes : null;
  const own3 = bridge ? genotypePoints(bridge, cached.traits) : null;
  const among = collectionName ?? 'its collection';
  return (
    <>
      <section aria-labelledby="h-dna" className="min-w-0 space-y-3">
        {/* Rubbing × Song print, phase 5 (A13): the DNA, run on its size, its genes and its
            number; the section is named by it, args and all. */}
        <PromptHeading
          id="h-dna"
          command="DNA"
          args={`${formatCount(layout.bytes.length, 'byte')} · ${formatCount(geneNames.length, 'gene')}${evidence.number !== null ? ` · number ${evidence.number}` : ''}`}
        />
        {cached.status === 'pending' && (
          <p role="status" className="text-text">
            Decode pending · awaiting DOB execution.
          </p>
        )}
        {cached.status === 'failed' && (
          <p className="max-w-measure" role="alert">
            DOB execution failed: {cached.reason ?? 'Failure reason unavailable'}
          </p>
        )}
        {/* Hierarchy v3 (F4): one caption; the paragraph and the bytes, gene by gene, are tier 2
            (the caption's 160-character budget holds no pointer to them). */}
        <p data-role="caption" className="text-text text-body max-w-measure">
          Its traits are read from {unitText(formatCount(words.bytes, 'byte'))} of DNA
          {words.pointer
            ? `; its ${words.pointer} gene points to a Bitcoin inscription, which makes it BTC+CKB`
            : ''}
          .
        </p>
      </section>
      {/* C10: the collection's gel is its own section (and plate), so each section keeps one hint. */}
      {collection && (
        <section aria-label={`Among ${among}`} className="min-w-0 space-y-3">
          <GelPlate tier={tier} className="space-y-2 px-4 py-4 sm:px-5">
            <div className="flex flex-wrap items-baseline justify-between gap-4">
              <span className="text-text text-body">
                Among {among} · its variant in each gene, and how common it is among the{' '}
                {collection.genetics.eligibleItems.toLocaleString('en-US')}
              </span>
              {collectionHref && (
                <AppLink look="onward" href={collectionHref} className="text-small">
                  Open the collection
                </AppLink>
              )}
            </div>
            {bridge && own3 ? (
              <>
                <AlleleGel
                  lanes={bridge}
                  total={collection.genetics.eligibleItems}
                  subject={collectionName ?? subject}
                  ink={tier === 'btc_ckb' || tier === 'pure_ckb' ? tier : 'other'}
                  ground={plateGround(tier)}
                  bandHeight={170}
                  overlays={[
                    { key: 'me', label: 'this one', points: own3.points, persistent: true },
                  ]}
                  activeGene={active}
                  onGeneHover={setActive}
                  labelOf={geneLabel}
                  idle="Its own band is outlined in every lane; a band reads how many share it."
                />
                <NotGenesNote
                  traits={traitsOutsidePattern(
                    collection.genetics,
                    geneOrder(collection.currentPattern)
                  )}
                />
                {own3.absent.length > 0 && (
                  <p role="status" className="text-text text-small max-w-measure">
                    Its {own3.absent.join(', ')} variant is not among the collection’s counted
                    variants.
                  </p>
                )}
              </>
            ) : ruled ? null : (
              <p role="status" className="text-text text-small">
                Collection variant frequencies await complete decoding.
              </p>
            )}
            {ruled && (
              <DroppedGenesCaption
                dropped={ruled.dropped}
                objects={collection.genetics.eligibleItems}
              />
            )}
          </GelPlate>
        </section>
      )}
    </>
  );
}
/**
 * Tier 2 (OB-8): what each piece of DNA evidence is — the current decoder's byte map, the cached
 * VM result and when it was decoded, every raw trait value — so tier 1 can speak plainly.
 */
export function ObjectGeneticsEvidence({
  evidence,
  subject,
  decoded,
  description,
  active,
  onActive,
  tier,
}: {
  evidence: ObjectGenetics;
  subject: string;
  /** The object's composition: the gel's plate keeps its copper for BTC+CKB and pure CKB. */
  tier?: CompositionTier;
  /** The gene lit by a trait's hover in tier 1, and the gel's own hover reported back. */
  active?: string | null;
  onActive?: (gene: string | null) => void;
  /**
   * F2 objects (OB-14): the decoder's own output files and the picture drawn from the traits, if
   * any, from `/spore/objects/{id}/decode`.
   */
  decoded?: SporeDobDecoded;
  /** Lane E follow-up: the cluster's description, for a pattern it declares (`sequencingGenes`). */
  description?: string | null;
}) {
  const { dnaHex, cachedVmTraits: cached } = evidence;
  const layout = dnaHex === null ? null : sequencingGenes(evidence, subject, description);
  const detailTier = useDetailTier();
  return (
    <section aria-label="DNA evidence" className="min-w-0 space-y-3">
      <h2
        className={
          detailTier === 2 ? 'text-body text-text font-medium' : 'text-text-bright text-section'
        }
      >
        DNA evidence
      </h2>
      <p className="text-text text-small max-w-measure">
        The byte map describes the current decoder. Trait values are cached VM results
        {cached.decodedAt !== null
          ? unitText(`, decoded ${formatUnixSecondsUtc8(cached.decodedAt)}`)
          : ''}
        ; the original decoder binary hash was not recorded. These are separate pieces of evidence.
      </p>
      {layout && (
        <ObjectDnaReading
          key={`${dnaHex}#${layout.genes.map((g) => g.key).join(',')}`}
          layout={layout}
          // Lane P2's follow-up (item 1): the byte-level plot moved here from tier 1. The byte map
          // above it draws the DNA to scale, a part per gene; the gel reads the mechanism, each
          // hex digit a band in the lane of its value, a gene's bytes, its rule and the trait they
          // express in its readout; the table under it gives this one object's every gene.
          gel={
            <GelPlate tier={tier} className="space-y-4 px-4 py-4 sm:px-5">
              {layout.genes.length ? (
                <SequencingGel
                  layout={layout}
                  subject={subject}
                  active={active ?? null}
                  onActive={(gene) => onActive?.(gene)}
                  idle="A gene's bytes, its rule and the trait they express."
                />
              ) : (
                <DnaStrip dna={dnaHex!} label={`${subject} DNA`} />
              )}
              {layout.unsupported.map((u) => (
                <p
                  key={u.step}
                  role="status"
                  title={u.reason}
                  className="text-text text-small max-w-measure"
                >
                  Decoder {u.step + 1}: the decoder binary describes no byte mapping; the pattern
                  alone is used.
                </p>
              ))}
            </GelPlate>
          }
        />
      )}
      {dnaHex !== null && (
        // the library's inner disclosure (lane cd-fixes-controls, the disclosure sweep)
        <SectionDisclosure
          form="inner"
          summary={
            <>
              All {layout ? unitText(formatCount(layout.bytes.length, 'byte')) : 'bytes'} of DNA as
              hex
            </>
          }
        >
          <pre className="text-text text-small max-w-full whitespace-pre-wrap break-all">
            {dnaHex}
          </pre>
        </SectionDisclosure>
      )}
      {decoded && (decoded.media.length > 0 || decoded.render !== null) && (
        <ul aria-label="Decoder output" className="text-small space-y-1">
          {decoded.media.map((m) => (
            // words break between them; a media type anyone can mint breaks anywhere if it must
            <li key={m.step} className="[overflow-wrap:anywhere]">
              <span className="text-text">
                {m.mediaType} · {unitText(formatCount(m.size, 'byte'))} · step {m.step + 1}
              </span>{' '}
              <span>
                <span className="text-text-dim text-meta">sha256</span>{' '}
                <ShortId value={m.hash} kind="hash" size="row" />
              </span>{' '}
              <a
                href={`${apiBaseFor(resolveActiveNetwork())}${m.url}`}
                target="_blank"
                rel="noopener noreferrer"
                className="text-aqua"
              >
                Open the decoded file ↗
              </a>
            </li>
          ))}
          {/* The picture /render draws holds no stored byte: it has no byte count and no sha256. */}
          {decoded.render !== null && (
            <li className="[overflow-wrap:anywhere]">
              <span className="text-text">
                {decoded.render.mediaType} · drawn from the traits on request, not a stored file
              </span>{' '}
              <a
                href={`${apiBaseFor(resolveActiveNetwork())}${decoded.render.url}`}
                target="_blank"
                rel="noopener noreferrer"
                className="text-aqua"
              >
                Open the picture ↗
              </a>
            </li>
          )}
        </ul>
      )}
      {cached.traits.length > 0 && (
        // a trait's value at 13 px, its name the label at 12 (ink and size roles)
        <dl aria-label="Cached VM traits" className="text-small grid gap-2 sm:grid-cols-2">
          {cached.traits.map((t, i) => (
            <div key={`${t.name}:${i}`} className="min-w-0">
              <dt className="text-text-dim text-meta">{t.name}</dt>
              {/* a raw trait value anyone can mint: words stay whole, a long run breaks anywhere */}
              <dd className="[overflow-wrap:anywhere]">{t.value === '' ? '(empty)' : t.value}</dd>
            </div>
          ))}
        </dl>
      )}
    </section>
  );
}
/**
 * One object's DNA in tier 2 (lane E, C5): its bytes drawn to scale, one part per gene that owns
 * them (the sequencing gel's rule and inks), then one row per gene — the bytes it reads here, its
 * rule, its raw bytes and value; the declared range in words is the row's title. Genes clipped to
 * this DNA, and genes it has no bytes for, are named under the rows.
 */
function ObjectDnaReading({
  layout,
  gel,
}: {
  layout: ReturnType<typeof sequencingGenes>;
  /** The sequencing gel's plate, drawn between the byte map and the gene table. */
  gel: ReactNode;
}) {
  const readout = usePinnedReadout<string>();
  const byKey = new Map(layout.genes.map((g) => [g.key, g]));
  const span = (offset: number, length: number) =>
    length === 1 ? `byte ${offset}` : `bytes ${offset}–${offset + length - 1}`;
  const parts: { key: string | null; start: number; end: number }[] = [];
  layout.owners.forEach((key, b) => {
    const last = parts.at(-1);
    if (last && last.key === key) last.end = b + 1;
    else parts.push({ key, start: b, end: b + 1 });
  });
  const inside = layout.genes
    .filter((g) => !g.owns)
    .map((g) => ({ name: g.name, start: g.offset, end: g.offset + g.length }));
  const declaredOnly = layout.genes.some((g) => g.source === 'description');
  const clipped = layout.genes.filter((g) => g.truncated);
  const title = (g: (typeof layout.genes)[number]) =>
    `${g.name} · ${span(g.offset, g.length)} · Declared bytes ${g.declared.offset} + ${g.declared.length}.${g.truncated ? ' Clipped to the actual DNA shown.' : ''} ${g.ruleName}${g.ruleArguments !== null && g.ruleArguments !== undefined ? ` ${JSON.stringify(g.ruleArguments)}` : ''}; raw 0x${g.raw}.`;
  return (
    <div className="min-w-0 space-y-3">
      {layout.bytes.length > 0 && (
        <ByteScaleMap
          label="Object DNA"
          total={layout.bytes.length}
          segments={parts.map((part) => {
            const n = part.end - part.start;
            if (part.key === null)
              return {
                key: `none@${part.start}`,
                start: part.start,
                end: part.end,
                label: count(n),
                short: count(n),
                color: 'var(--c-ink-quiet)',
                quiet: true,
                title: `${span(part.start, n)} · no gene reads ${countNoun(n, 'it', 'them')}`,
              };
            const g = byKey.get(part.key)!;
            return {
              key: `${part.key}@${part.start}`,
              target: part.key,
              start: part.start,
              end: part.end,
              label: `${g.name} ${count(n)}`,
              // too narrow for its name, a part keeps its byte count (Main board: "Entity 8", "1")
              short: count(n),
              color: g.color,
              title: `${g.name} · ${span(part.start, n)}`,
            };
          })}
          active={readout.active}
          onHover={readout.hover}
          onSelect={readout.toggle}
          note={inside.length > 0 ? insideNote(inside) : undefined}
        />
      )}
      {gel}
      {layout.genes.length > 0 && (
        <ListTable
          label="DNA genes"
          density="row"
          rows={layout.genes}
          rowKey={(g) => g.key}
          rowTitle={title}
          rowClassName={(g) => (readout.active === g.key ? 'bg-base-surface' : '')}
          columns={[
            {
              key: 'gene',
              label: 'gene',
              minWidth: geneText(),
              header: <span className="text-meta">gene</span>,
              render: (g) => (
                <span
                  className="leading-row block truncate"
                  style={{ color: g.owns ? g.color : undefined }}
                >
                  {g.name}
                </span>
              ),
            },
            {
              key: 'bytes',
              label: 'bytes',
              // the widest range with its " · clipped" note
              minWidth: geneSpanPx(`${WIDEST_DNA_SPAN} · clipped`),
              header: <span className="text-meta">bytes</span>,
              render: (g) => (
                <span className="text-text leading-row block truncate tabular-nums">
                  {span(g.offset, g.length)}
                  {g.truncated ? ' · clipped' : ''}
                </span>
              ),
            },
            {
              key: 'rule',
              label: 'rule',
              minWidth: geneText(),
              header: <span className="text-meta">rule</span>,
              render: (g) => <span className="text-text leading-row block truncate">{g.rule}</span>,
            },
            {
              key: 'value',
              label: 'this object',
              minWidth: geneText(true),
              width: 'auto',
              header: <span className="text-meta">this object</span>,
              render: (g) => (
                // a cut value carries the whole of it in its title (the lead's finding)
                <span
                  className="text-text leading-row block truncate"
                  title={g.value ?? `0x${g.raw}`}
                >
                  {g.value ?? `0x${g.raw}`}
                </span>
              ),
            },
          ]}
          mobileRow={(g) => (
            <PhoneRow
              lines={[
                {
                  key: 'gene',
                  tone: 'head',
                  left: g.name,
                  right: `${span(g.offset, g.length)}${g.truncated ? ' · clipped' : ''}`,
                },
                {
                  key: 'value',
                  tone: 'body',
                  left: <span className="[overflow-wrap:anywhere]">{g.value ?? `0x${g.raw}`}</span>,
                  right: g.rule,
                },
              ]}
            />
          )}
        />
      )}
      {declaredOnly && (
        <p className="text-text text-small max-w-measure">
          These ranges are the pattern’s own declaration, in the cluster’s description; the decoder
          binary names no byte mapping to check them against.
        </p>
      )}
      {clipped.length > 0 && (
        <p className="text-text text-small max-w-measure">
          Clipped to the actual DNA shown: {clipped.map((g) => g.name).join(', ')} declare
          {clipped.length === 1 ? 's' : ''} bytes past its {count(layout.bytes.length)}.
        </p>
      )}
      {layout.unmapped.map((u) => (
        <p key={u.name} className="text-text text-small max-w-measure">
          {u.name}: Declared bytes {u.declared.offset} + {u.declared.length}; no bytes of this DNA
          fall in the declared range.
        </p>
      ))}
    </div>
  );
}

// F2 objects (round 2, OB-C1/C2): the per-variant button lists were replaced by the allele gel
// (`ui/allele-gel.tsx`, `collection-objects.tsx`); a band click is the filter.

/** One real object read through the pattern (lane E, C5): the gallery's DNA and VM traits. */
export interface PatternSample {
  id: string;
  href: string;
  dnaHex: string | null;
  traits: readonly { name: string; value: string }[];
  decodeStatus: string;
}

export interface CurrentPatternEvidenceProps {
  pattern: CurrentPattern;
  /** Lane E (C5), additive: the collection's decode counts, for "found across N". */
  genetics?: CollectionInsights['genetics'];
  /** Lane E (C5), additive: one real object, read through the pattern. */
  sample?: PatternSample | null;
  /** Lane E (C5), additive: the status line under the reading (decode coverage). */
  status?: ReactNode;
  /**
   * Lane E follow-up, additive: the cluster's stored description, whose declared pattern a step
   * the API cannot map is read from; `undefined` while it loads, `null` when there is none.
   */
  description?: string | null;
}

const RULED_UNMAPPED = 'The decoder binary describes no byte mapping; the pattern alone is used.';
const FORMAT_WORDS: Record<string, string> = {
  moleculeDob0: 'molecule DOB/0',
  jsonDob0: 'JSON DOB/0',
  nervapeJsonDob0: 'Nervape JSON DOB/0',
};
const count = (n: number) => n.toLocaleString('en-US');

/**
 * "How an object is read" (consistency C5, Main board tier 2): each object's DNA → the pattern in
 * the cluster's description → the decoder, as three tiles; the DNA ruler with a part per gene; the
 * gene table with what the decoded objects show; one real object read through it; the status line.
 */
export function CurrentPatternEvidence(props: CurrentPatternEvidenceProps) {
  // Another pattern (a decoder upgrade) or another object starts the reading over: no stale pin.
  const key =
    props.pattern.status === 'dob'
      ? props.pattern.steps
          .map((step) =>
            step.description.status === 'supported'
              ? `${step.step}:${step.description.genes.map((g) => g.index).join(',')}`
              : `${step.step}:-`
          )
          .join('|') +
        `#${props.sample?.dnaHex ?? ''}#${props.description === undefined ? 'unread' : 'read'}`
      : 'notDob';
  return <PatternReading key={key} {...props} />;
}

function PatternReading({
  pattern,
  genetics,
  sample,
  status,
  description,
}: CurrentPatternEvidenceProps) {
  const tier = useDetailTier();
  const readout = usePinnedReadout<string>();
  if (pattern.status === 'notDob')
    return <p className="text-text text-small">No DOB blueprint in this collection.</p>;
  const genes: PatternGene[] = patternGenes(pattern, description, 'How an object is read');
  const declared = genes.some((g) => g.source === 'description');
  const dna = sample?.dnaHex ? hexBytes(sample.dnaHex, `${sample.id} DNA`) : null;
  const extent = dna ? dna.length : patternExtent(genes);
  const ruler = genes.length && extent ? dnaRulerParts(genes, extent) : null;
  const byKey = new Map(genes.map((g) => [g.key, g]));
  const ink = new Map<string, string>();
  ruler?.parts.forEach((part) => {
    if (part.key === null || ink.has(part.key)) return;
    const gene = byKey.get(part.key)!.gene;
    ink.set(
      part.key,
      /^btcfs/i.test(gene.rule)
        ? GENE_COLORS.bitcoin
        : GENE_COLORS.cycle[ink.size % GENE_COLORS.cycle.length]
    );
  });
  const inkOf = (key: string) => ink.get(key) ?? 'var(--c-text-dim)';
  const rows = geneRows(genes);
  const found = (row: GeneRow) =>
    genetics
      ? row.genes.length === 1
        ? foundAcross(row.genes[0].gene.name, genetics)
        : foundAcrossRun(
            row.genes.map((g) => g.gene.name),
            genetics
          )
      : null;
  const activeGene = readout.active;
  const rowTitle = (row: GeneRow) =>
    [
      row.genes.length > 1
        ? `${row.label}: ${row.genes.map((g) => g.gene.name).join(', ')}`
        : row.label,
      row.span,
      row.rule,
      found(row),
    ]
      .filter(Boolean)
      .join(' · ');
  const steps = pattern.steps;
  const foundHead = genetics?.complete ? `found across ${count(genetics.eligibleItems)}` : 'found';
  const cell = 'block truncate leading-row';
  // Responsive R1: a table while its columns fit the measured column, else one phone row a gene.
  const geneTable = (shown: readonly GeneRow[], label: string) => (
    <ListTable
      label={label}
      density="row"
      rows={shown}
      rowKey={(row) => row.genes[0].key}
      rowTitle={rowTitle}
      rowClassName={(row) =>
        activeGene !== null && row.genes.some((g) => g.key === activeGene) ? 'bg-base-surface' : ''
      }
      columns={[
        {
          key: 'gene',
          label: 'gene',
          minWidth: geneText(),
          header: <span className="text-meta">gene</span>,
          render: (row) => <span className={cn(cell, 'text-text-bright')}>{row.label}</span>,
        },
        {
          key: 'bytes',
          label: 'bytes',
          minWidth: geneSpanPx(WIDEST_DNA_SPAN),
          header: <span className="text-meta">bytes</span>,
          render: (row) => <span className={cn(cell, 'text-text tabular-nums')}>{row.span}</span>,
        },
        {
          key: 'rule',
          label: 'rule',
          minWidth: geneText(),
          header: <span className="text-meta">rule</span>,
          render: (row) => <span className={cn(cell, 'text-text')}>{row.rule}</span>,
        },
        {
          key: 'found',
          label: foundHead,
          minWidth: geneText(true),
          width: 'auto',
          header: <span className="text-meta">{foundHead}</span>,
          render: (row) => <span className={cn(cell, 'text-text')}>{found(row) ?? '—'}</span>,
        },
      ]}
      mobileRow={(row) => (
        <PhoneRow
          lines={[
            { key: 'gene', tone: 'head', left: row.label, right: row.span },
            { key: 'rule', tone: 'body', left: <TraitText text={row.rule} /> },
            // what the objects show can be long: it wraps on its own line, never a right part
            { key: 'found', tone: 'quiet', left: <TraitText text={found(row) ?? '—'} /> },
          ]}
        />
      )}
    />
  );
  const folded = rows.length < genes.length;
  // the folded table's rows, one a gene
  const everyGene: GeneRow[] = genes.map((g) => ({
    label: g.gene.name,
    genes: [g],
    span: geneSpan(g.gene),
    rule: ruleWords(g.gene),
  }));
  const sampleTraits = sample?.traits ?? [];
  const valueOf = (row: GeneRow) => {
    const values = row.genes.map((g) =>
      sampleTraits
        .filter((t) => t.name === g.gene.name)
        .map((t) => traitDisplay(t.value))
        .join(' · ')
    );
    if (row.genes.length === 1) return values[0] || '—';
    return values.every((v) => [...v].length === 1)
      ? values.join('')
      : values.map((v) => v || '—').join(' · ');
  };
  const geneNames = new Set(genes.map((g) => g.gene.name));
  const outside = sampleTraits.filter((t) => !geneNames.has(t.name));
  return (
    <section aria-label="How an object is read" className="min-w-0 space-y-4">
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <h2
          className={cn(
            'font-medium',
            tier === 2 ? 'text-body text-text' : 'text-text-bright text-section'
          )}
        >
          How an object is read
        </h2>
        <span className="text-text-dim text-meta">
          {pattern.version === null ? 'DOB version not stated' : `DOB/${pattern.version}`}
          {genetics
            ? ` · ${count(genetics.decodedItems)} of ${count(genetics.eligibleItems)} decoded`
            : ''}
        </span>
      </div>
      <ol
        aria-label="From DNA to traits"
        className="flex flex-col gap-1.5 sm:flex-row sm:items-stretch"
      >
        {[
          {
            title: 'Each object',
            body: unitText(
              dna
                ? `${formatCount(dna.length, 'byte')} of DNA in its cell data.`
                : 'its DNA, in its cell data.'
            ),
          },
          {
            title: 'The pattern',
            body: unitText(
              `stored in this cluster’s description: ${formatCount(genes.length, 'gene')}, each a byte range and a rule.`
            ),
          },
          { title: steps.length > 1 ? 'The decoders' : 'The decoder', body: null },
        ].map((tile, i) => (
          <li key={tile.title} className="flex min-w-0 flex-col gap-1.5 sm:flex-1 sm:flex-row">
            {i > 0 && (
              <span
                aria-hidden="true"
                className="text-text-ghost text-center sm:w-5 sm:self-center"
              >
                <span className="sm:hidden">↓</span>
                <span className="hidden sm:inline">→</span>
              </span>
            )}
            <div className="text-small text-text border-base-border-subtle min-w-0 flex-1 border px-3 py-2.5 [overflow-wrap:anywhere]">
              <b className="text-text-bright mb-0.5 block font-medium">{tile.title}</b>
              {tile.body ??
                steps.map((step) => (
                  <span key={step.step} className="block">
                    {steps.length > 1 && <span className="text-text">{step.step + 1}. </span>}
                    by {step.decoderReference.type}{' '}
                    <span title={step.decoderReference.hash}>
                      {/* a tile of the mechanism chain, under 200 px: tight (U-09) */}
                      <ShortId value={step.decoderReference.hash} kind="hash" size="tight" />
                    </span>
                    {step.decoderBinaryHash ? (
                      step.decoderBinaryHash === step.decoderReference.hash ? (
                        <>
                          {' '}
                          <AppLink
                            look="inline"
                            href={`/script/${step.decoderBinaryHash}`}
                            title={step.decoderBinaryHash}
                          >
                            →
                          </AppLink>
                        </>
                      ) : (
                        <>
                          {', binary '}
                          <AppLink
                            look="inline"
                            href={`/script/${step.decoderBinaryHash}`}
                            title={step.decoderBinaryHash}
                          >
                            <ShortId value={step.decoderBinaryHash} kind="hash" size="tight" />
                          </AppLink>
                        </>
                      )
                    ) : (
                      ', binary unresolved'
                    )}
                    {step.description.status === 'unsupported' ? (
                      <span
                        role="status"
                        title={step.description.reason}
                        className="block [overflow-wrap:anywhere]"
                      >
                        {RULED_UNMAPPED}
                      </span>
                    ) : (
                      <span className="block">
                        reads the genes ·{' '}
                        {FORMAT_WORDS[step.description.format] ?? step.description.format}
                        {step.description.rangePolicy === 'clipToDna'
                          ? ' · clipped to the DNA'
                          : ' · exact ranges'}
                      </span>
                    )}
                  </span>
                ))}
            </div>
          </li>
        ))}
      </ol>
      {ruler && extent !== null ? (
        <div className="space-y-1.5">
          <ByteScaleMap
            label="DNA"
            total={extent}
            segments={ruler.parts.map((part) => {
              const n = part.end - part.start;
              const span = n === 1 ? `byte ${part.start}` : `bytes ${part.start}–${part.end - 1}`;
              if (part.key === null)
                return {
                  key: `none@${part.start}`,
                  start: part.start,
                  end: part.end,
                  label: count(n),
                  short: count(n),
                  color: 'var(--c-ink-quiet)',
                  quiet: true,
                  title: `${span} · no gene reads ${countNoun(n, 'it', 'them')}`,
                };
              const gene = byKey.get(part.key)!.gene;
              return {
                key: `${part.key}@${part.start}`,
                target: part.key,
                start: part.start,
                end: part.end,
                label: `${gene.name} ${count(n)}`,
                // too narrow for its name, a part keeps its byte count (Main board: "Entity 8", "1")
                short: count(n),
                color: inkOf(part.key),
                title: `${gene.name} · ${span}`,
              };
            })}
            active={readout.active}
            onHover={readout.hover}
            onSelect={readout.toggle}
            note={ruler.inside.length > 0 ? insideNote(ruler.inside) : undefined}
          />
          <ReadoutRow
            label="DNA ruler readout"
            pinned={readout.showingPin}
            idle={`${formatCount(extent, 'byte')} of DNA · ${formatCount(genes.length, 'gene')}`}
          >
            {readout.active === null ? null : (
              <TraitText
                text={(() => {
                  const g = byKey.get(readout.active)!;
                  const row = rows.find((r) => r.genes.includes(g))!;
                  return (
                    [
                      g.gene.name,
                      geneSpan(g.gene),
                      ruleWords(g.gene),
                      genetics ? foundAcross(g.gene.name, genetics) : null,
                    ]
                      .filter(Boolean)
                      .join(' · ') + (row.genes.length > 1 ? ` (in ${row.label})` : '')
                  );
                })()}
              />
            )}
          </ReadoutRow>
        </div>
      ) : (
        genes.length > 0 && (
          <p className="text-text text-small max-w-measure">
            Its ranges run past what a ruler can draw; the table gives them exactly.
          </p>
        )
      )}
      {genes.length > 0 && geneTable(rows, 'DOB blueprint')}
      {declared && (
        <p className="text-text text-small max-w-measure">
          These ranges are the pattern’s own declaration, in the cluster’s description; the decoder
          binary names no byte mapping to check them against.
        </p>
      )}
      {folded && (
        <SectionDisclosure
          form="inner"
          summary={<>All {unitText(formatCount(genes.length, 'gene'))}, one row each</>}
        >
          {geneTable(everyGene, 'DOB blueprint, every gene')}
        </SectionDisclosure>
      )}
      {sample && (
        <div
          role="group"
          aria-label="One object, read this way"
          className="border-base-border-subtle min-w-0 border px-3.5 py-3"
        >
          {/* the label and the byte count quiet at 12 px, the object a link in its look at 13 px
              (ink and size roles: the link stood in the label's quiet ink, at 12 px) */}
          <div className="flex flex-wrap items-baseline justify-between gap-x-3">
            <span>
              <span className="text-meta text-text-dim">one object, read this way ·</span>{' '}
              <AppLink look="inline" href={sample.href} title={sample.id} className="text-small">
                <ShortId value={sample.id} kind="hash" size="row" />
              </AppLink>
            </span>
            {dna && (
              <span className="text-meta text-text-dim">
                {unitText(formatCount(dna.length, 'byte'))}
              </span>
            )}
          </div>
          {sample.decodeStatus !== 'decoded' ? (
            <p className="text-text text-small max-w-measure mt-2">
              Not decoded yet ({sample.decodeStatus}); its values appear once the decoder has run.
            </p>
          ) : (
            <dl className="text-small mt-2 grid grid-cols-[minmax(0,140px)_minmax(0,1fr)] gap-x-4 gap-y-1">
              {rows.map((row) => (
                <Fragment key={row.genes[0].key}>
                  <dt
                    className="truncate"
                    style={{ color: inkOf(row.genes[0].key) }}
                    title={row.label}
                  >
                    {row.label}
                  </dt>
                  <dd className="text-text m-0 min-w-0 [overflow-wrap:anywhere]">
                    <TraitText text={valueOf(row)} />
                  </dd>
                </Fragment>
              ))}
            </dl>
          )}
          {outside.length > 0 && (
            <p className="text-text text-small max-w-measure mt-2 [overflow-wrap:anywhere]">
              Also from the decoder, outside the pattern:{' '}
              <TraitText
                text={outside.map((t) => `${t.name} (${traitDisplay(t.value)})`).join(' · ')}
              />
            </p>
          )}
        </div>
      )}
      {status}
    </section>
  );
}

/** "prev.type and relief read byte 0 too": genes whose bytes all sit inside a wider gene. */
function insideNote(inside: readonly { name: string; start: number; end: number }[]): string {
  const groups = new Map<string, string[]>();
  for (const g of inside) {
    const span = g.end - g.start === 1 ? `byte ${g.start}` : `bytes ${g.start}–${g.end - 1}`;
    groups.set(span, [...(groups.get(span) ?? []), g.name]);
  }
  return [...groups]
    .map(([span, names]) => {
      const who =
        names.length === 1 ? names[0] : `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`;
      return `${who} read${names.length === 1 ? 's' : ''} ${span} too`;
    })
    .join(' · ');
}
