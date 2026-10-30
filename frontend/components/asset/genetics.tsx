'use client';
import { useState, type CSSProperties } from 'react';
import type { CollectionInsights, CurrentPattern, ObjectGenetics } from '@/lib/api';
import { AlleleGel } from '@/components/ui/allele-gel';
import { SequencingGel } from '@/components/ui/sequencing-gel';
import { DnaStrip } from '@/components/ui/dna-strip';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { COMPOSITION_BADGES, HUE_RAMP } from '@/lib/design-tokens';
import { alleleLanes, geneOrder, genotypePoints, sequencingGenes } from '@/lib/dob-genes';
import type { CompositionTier } from '@/lib/asset-utils';
import { cn, formatUnixSecondsUtc8 } from '@/lib/utils';

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
 * The object's DNA: a sequencing gel of its own bytes (gene by gene), then, on the same plate, the
 * allele gel of its collection with this object's genotype drawn across it ("this one").
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
}: ObjectGeneticsViewProps) {
  const [own, setOwn] = useState<string | null>(null);
  const active = controlled === undefined ? own : controlled;
  const setActive = (gene: string | null) => {
    setOwn(gene);
    onActive?.(gene);
  };
  const { dnaHex, cachedVmTraits: cached } = evidence;
  if (dnaHex === null)
    return <p className="text-text-dim text-[11px]">This object has no DOB DNA.</p>;
  const layout = sequencingGenes(evidence, subject);
  const geneNames = [...new Set(layout.genes.map((g) => g.name))];
  const pointer = layout.genes.find((g) => g.owns && /^btcfs/i.test(g.ruleName));
  const bridge = collection?.genetics.complete
    ? alleleLanes(
        collection.genetics,
        geneOrder(collection.currentPattern),
        collectionName ?? subject
      )
    : null;
  const own3 = bridge ? genotypePoints(bridge, cached.traits) : null;
  return (
    <section aria-labelledby="h-dna" className="min-w-0 space-y-3">
      <div className="flex flex-wrap items-baseline justify-between gap-4">
        <h2 id="h-dna" className="text-text-bright text-[15px]">
          DNA
        </h2>
        <span className="text-text-dim text-[12px]">
          {layout.bytes.length} bytes · {geneNames.length} gene{geneNames.length === 1 ? '' : 's'}
          {evidence.number !== null ? ` · number ${evidence.number}` : ''} · hover a gene
        </span>
      </div>
      <p className="text-[13px] leading-relaxed">
        The object’s traits are not stored as text. It carries {layout.bytes.length} bytes of DNA,
        and the collection’s decoder reads each gene to express a trait.
        {pointer && tier === 'btc_ckb'
          ? ` Its ${pointer.name} gene is a pointer to a Bitcoin inscription; that is what makes it BTC+CKB.`
          : ''}
      </p>
      {cached.status === 'pending' && (
        <p role="status" className="text-text-dim">
          Decode pending · awaiting DOB execution.
        </p>
      )}
      {cached.status === 'failed' && (
        <p role="alert">DOB execution failed: {cached.reason ?? 'Failure reason unavailable'}</p>
      )}
      <div
        className={cn('rounded-[10px] p-[5px]', !plateStyle(tier) && 'border-base-border border')}
        style={plateStyle(tier)}
      >
        <div className="space-y-4 rounded-md px-4 py-4 sm:px-5">
          {layout.genes.length ? (
            <SequencingGel layout={layout} subject={subject} active={active} onActive={setActive} />
          ) : (
            <DnaStrip dna={dnaHex} label={`${subject} DNA`} />
          )}
          {layout.unsupported.map((u) => (
            <p key={u.step} role="status" className="text-text-dim text-[11px]">
              Decoder {u.step + 1}: byte mapping unavailable — {u.reason}
            </p>
          ))}
          {collection && (
            <div className="border-base-border space-y-2 border-t pt-3">
              <div className="flex flex-wrap items-baseline justify-between gap-4">
                <span className="text-text text-[12.5px]">
                  Among {collectionName ?? 'its collection'} · its variant in each gene, and how
                  common it is among the {collection.genetics.eligibleItems.toLocaleString('en-US')}
                </span>
                {collectionHref && (
                  <AppLink href={collectionHref} className="text-[12px]">
                    Open the collection →
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
                    height={300}
                    overlays={[
                      { key: 'me', label: 'this one', points: own3.points, persistent: true },
                    ]}
                    activeGene={active}
                    onGeneHover={setActive}
                  />
                  {own3.absent.length > 0 && (
                    <p role="status" className="text-text-dim text-[11px]">
                      Its {own3.absent.join(', ')} variant is not among the collection’s counted
                      variants.
                    </p>
                  )}
                </>
              ) : (
                <p role="status" className="text-text-dim text-[11px]">
                  Collection variant frequencies await complete decoding.
                </p>
              )}
            </div>
          )}
        </div>
      </div>
    </section>
  );
}
/**
 * Tier 2 (OB-8): what each piece of DNA evidence is — the current decoder's byte map, the cached
 * VM result and when it was decoded, every raw trait value — so tier 1 can speak plainly.
 */
export function ObjectGeneticsEvidence({
  evidence,
  subject,
}: {
  evidence: ObjectGenetics;
  subject: string;
}) {
  const { dnaHex, cachedVmTraits: cached } = evidence;
  const layout = dnaHex === null ? null : sequencingGenes(evidence, subject);
  return (
    <section aria-label="DNA evidence" className="min-w-0 space-y-3">
      <h2 className="text-text-bright text-[15px]">DNA evidence</h2>
      <p className="text-text-dim text-[11px]">
        The byte map describes the current decoder. Trait values are cached VM results
        {cached.decodedAt !== null ? `, decoded ${formatUnixSecondsUtc8(cached.decodedAt)}` : ''};
        the original decoder binary hash was not recorded. These are separate pieces of evidence.
      </p>
      {dnaHex !== null && <p className="break-all text-[11px]">DNA {dnaHex}</p>}
      {layout && (
        <dl className="divide-base-elevated divide-y text-[11px]">
          {layout.genes.map((g) => (
            <div key={g.key} className="py-2">
              <dt>
                {g.name}{' '}
                <span className="text-text-dim">
                  bytes {g.offset}–{g.offset + g.length - 1}
                </span>
              </dt>
              <dd className="text-text-dim break-words">
                Declared bytes {g.declared.offset} + {g.declared.length}.{' '}
                {g.truncated ? 'Clipped to the actual DNA shown. ' : ''}
                {g.ruleName}
                {g.ruleArguments !== null && g.ruleArguments !== undefined
                  ? ` ${JSON.stringify(g.ruleArguments)}`
                  : ''}
                ; raw 0x{g.raw}.
              </dd>
            </div>
          ))}
          {layout.unmapped.map((u) => (
            <div key={u.name} className="text-text-dim py-2">
              {u.name}: Declared bytes {u.declared.offset} + {u.declared.length}; no bytes of this
              DNA fall in the declared range.
            </div>
          ))}
        </dl>
      )}
      {cached.traits.length > 0 && (
        <dl aria-label="Cached VM traits" className="grid gap-2 text-[11px] sm:grid-cols-2">
          {cached.traits.map((t, i) => (
            <div key={`${t.name}:${i}`} className="min-w-0">
              <dt className="text-text-dim">{t.name}</dt>
              <dd className="break-all">{t.value === '' ? '(empty)' : t.value}</dd>
            </div>
          ))}
        </dl>
      )}
    </section>
  );
}
/**
 * A trait shows its eight most frequent variants until opened, so one genome cannot push the
 * objects and holders off-screen; "+N more" opens every variant, each still a filter (OB-C1).
 */
const TRAIT_VALUE_PREVIEW = 8;

function TraitValueButtons({
  trait,
  eligibleItems,
  color,
  selected,
  onSelect,
  onFocus,
  onBlur,
}: {
  trait: CollectionInsights['genetics']['traits'][number];
  eligibleItems: number;
  color: string;
  selected: string | undefined;
  onSelect: (value: string | null) => void;
  onFocus: () => void;
  onBlur: () => void;
}) {
  const [open, setOpen] = useState(false);
  const ranked = [...trait.values].sort((a, b) =>
    b.count === a.count ? a.value.localeCompare(b.value) : b.count - a.count
  );
  const preview = ranked.slice(0, TRAIT_VALUE_PREVIEW);
  const selectedExtra =
    selected && !preview.some((v) => v.value === selected)
      ? ranked.find((v) => v.value === selected)
      : undefined;
  const visible = open
    ? ranked
    : selectedExtra
      ? [...ranked.slice(0, TRAIT_VALUE_PREVIEW - 1), selectedExtra]
      : preview;
  const hidden = ranked.length - visible.length;
  return (
    <div className="grid gap-2 sm:grid-cols-2">
      {visible.map((v) => {
        if (!Number.isSafeInteger(v.count) || v.count < 0 || v.count > eligibleItems)
          throw new Error(`Collection trait ${trait.name}: invalid count ${v.count}`);
        return (
          <button
            key={v.value}
            aria-pressed={selected === v.value}
            onClick={() => onSelect(selected === v.value ? null : v.value)}
            onFocus={onFocus}
            onBlur={onBlur}
            className="border-base-border relative min-w-0 border-b px-1 py-2 text-left"
          >
            <span
              aria-hidden
              className="absolute inset-y-0 left-0 opacity-15"
              style={{
                background: color,
                width: `${eligibleItems ? (v.count / eligibleItems) * 100 : 0}%`,
              }}
            />
            <span className="relative flex items-baseline justify-between gap-3">
              <span className="min-w-0 break-words">{v.value}</span>
              <span className="text-text-dim shrink-0 text-[11px]">
                {v.count} / {eligibleItems}
              </span>
            </span>
          </button>
        );
      })}
      {ranked.length > TRAIT_VALUE_PREVIEW && (
        <button
          type="button"
          aria-expanded={open}
          onClick={() => setOpen((current) => !current)}
          className="text-text-dim hover:text-text-bright col-span-full justify-self-start text-[11px]"
        >
          {open
            ? `Show the top ${TRAIT_VALUE_PREVIEW} ${trait.name} variants`
            : `+${hidden} more ${trait.name} variant${hidden === 1 ? '' : 's'}`}
        </button>
      )}
    </div>
  );
}

export interface CollectionGenesProps {
  evidence: CollectionInsights['genetics'];
  selected: Readonly<Record<string, string>>;
  onSelect: (name: string, value: string | null) => void;
}
export function CollectionGenes({ evidence, selected, onSelect }: CollectionGenesProps) {
  const [active, setActive] = useState<string | null>(null);
  const total = evidence.decodedItems + evidence.pendingItems + evidence.failedItems;
  if (
    total !== evidence.eligibleItems ||
    evidence.eligibleItems + evidence.nonDobItems !== evidence.liveItems
  )
    throw new Error('Collection genetics: live/eligible/decode coverage does not close');
  if (evidence.complete && evidence.decodedItems !== evidence.eligibleItems)
    throw new Error(
      'Collection genetics: complete frequencies require every eligible item decoded'
    );
  if (!evidence.complete && evidence.traits.length)
    throw new Error('Collection genetics: partial frequencies must not be presented as complete');
  return (
    <section aria-label="Collection allele frequencies" className="space-y-4">
      <h2 className="text-text-bright text-[15px]">The collection genome</h2>
      <p className="text-text-dim text-[11px]">
        {evidence.decodedItems} decoded / {evidence.eligibleItems} DOB items ·{' '}
        {evidence.pendingItems} pending · {evidence.failedItems} failed · {evidence.nonDobItems}{' '}
        non-DOB items
      </p>
      {!evidence.complete ? (
        <p role="status">Trait frequencies and filtering await complete decoding.</p>
      ) : evidence.traits.length === 0 ? (
        <p className="text-text-dim">No decoded traits.</p>
      ) : (
        <div className="space-y-5">
          {evidence.traits.map((trait, index) => (
            <div
              key={trait.name}
              data-allele={trait.name}
              className={active && active !== trait.name ? 'opacity-35' : ''}
              onMouseEnter={() => setActive(trait.name)}
              onMouseLeave={() => setActive(null)}
            >
              <h3 className="mb-2 text-[13px]" style={{ color: HUE_RAMP[index % HUE_RAMP.length] }}>
                {trait.name}
              </h3>
              <TraitValueButtons
                trait={trait}
                eligibleItems={evidence.eligibleItems}
                color={HUE_RAMP[index % HUE_RAMP.length]}
                selected={selected[trait.name]}
                onSelect={(value) => onSelect(trait.name, value)}
                onFocus={() => setActive(trait.name)}
                onBlur={() => setActive(null)}
              />
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
export interface CurrentPatternEvidenceProps {
  pattern: CurrentPattern;
}
export function CurrentPatternEvidence({ pattern }: CurrentPatternEvidenceProps) {
  if (pattern.status === 'notDob')
    return <p className="text-text-dim text-[11px]">No DOB blueprint in this collection.</p>;
  return (
    <section aria-label="Current decoder blueprint" className="space-y-6">
      <h2 className="text-text-bright text-[15px]">Current decoder blueprint</h2>
      <p className="text-text-dim text-[11px]">
        {pattern.version === null ? 'DOB version not stated' : `DOB/${pattern.version}`}
      </p>
      {pattern.steps.map((step) => (
        <article key={step.step} className="space-y-3">
          <h3>
            Step {step.step + 1} · {step.decoderReference.type}
          </h3>
          <div className="break-all text-[11px]">
            Reference <Hash hash={step.decoderReference.hash} truncate={false} />
          </div>
          <div className="break-all text-[11px]">
            Current binary{' '}
            {step.decoderBinaryHash ? (
              <AppLink href={`/script/${step.decoderBinaryHash}`}>
                <Hash hash={step.decoderBinaryHash} truncate={false} copyable={false} />
              </AppLink>
            ) : (
              'Unresolved'
            )}
          </div>
          {step.description.status === 'unsupported' ? (
            <p role="status">Mapping unavailable: {step.description.reason}</p>
          ) : (
            <>
              <p className="text-text-dim text-[11px]">
                {step.description.format} · {step.description.rangePolicy}
              </p>
              <dl className="space-y-3">
                {step.description.genes.map((g) => (
                  <div key={g.index} className="min-w-0">
                    <dt>
                      {g.name}{' '}
                      <span className="text-text-dim text-[11px]">
                        {g.typeTag} · byte offset {g.byteOffset} · length {g.byteLength}
                      </span>
                    </dt>
                    <dd className="text-text-dim break-words text-[11px]">
                      {g.rule} ·{' '}
                      <code className="break-all">{JSON.stringify(g.ruleArguments)}</code>
                    </dd>
                  </div>
                ))}
              </dl>
            </>
          )}
        </article>
      ))}
    </section>
  );
}
