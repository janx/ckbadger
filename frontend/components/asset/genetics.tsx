'use client';
import { useState, type CSSProperties } from 'react';
import type {
  CollectionInsights,
  CurrentPattern,
  ObjectGenetics,
  SporeDobDecoded,
} from '@/lib/api';
import { apiBaseFor, resolveActiveNetwork } from '@/lib/active-network';
import { AlleleGel } from '@/components/ui/allele-gel';
import { SequencingGel } from '@/components/ui/sequencing-gel';
import { DnaStrip } from '@/components/ui/dna-strip';
import { AppLink } from '@/components/ui/app-link';
import { useDetailTier } from '@/components/ui/detail-layout';
import { Hash } from '@/components/ui/hash';
import { COMPOSITION_BADGES } from '@/lib/design-tokens';
import {
  alleleLanes,
  geneOrder,
  genotypePoints,
  sequencingGenes,
  traitDisplay,
} from '@/lib/dob-genes';
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
  decoded,
}: {
  evidence: ObjectGenetics;
  subject: string;
  /** F2 objects (OB-14): the decoder's own output files, from `/spore/objects/{id}/decode`. */
  decoded?: SporeDobDecoded;
}) {
  const { dnaHex, cachedVmTraits: cached } = evidence;
  const layout = dnaHex === null ? null : sequencingGenes(evidence, subject);
  const tier = useDetailTier();
  return (
    <section aria-label="DNA evidence" className="min-w-0 space-y-3">
      <h2
        className={
          tier === 2 ? 'text-[13px] font-medium text-[#a0a8be]' : 'text-text-bright text-[15px]'
        }
      >
        DNA evidence
      </h2>
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
                  {g.length === 1
                    ? `byte ${g.offset}`
                    : `bytes ${g.offset}–${g.offset + g.length - 1}`}
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
      {decoded && decoded.media.length > 0 && (
        <ul aria-label="Decoder output" className="space-y-1 text-[11px]">
          {decoded.media.map((m) => (
            <li key={`${m.hash}:${m.step ?? 'x'}`} className="break-all">
              <span className="text-text">
                {m.mediaType} · {m.size.toLocaleString('en-US')} bytes
                {m.step !== null ? ` · step ${m.step + 1}` : ''}
                {m.role ? ` · ${m.role}` : ''}
              </span>{' '}
              <span className="text-text-dim">sha256 {m.hash.slice(0, 12)}…</span>{' '}
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
        </ul>
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
// F2 objects (round 2, OB-C1/C2): the per-variant button lists were replaced by the allele gel
// (`ui/allele-gel.tsx`, `collection-objects.tsx`); a band click is the filter.
/** "bytes 0–31" from the exact declared offset and length (never converted to a float). */
function byteSpan(offset: string, length: string): string {
  if (!/^(0|[1-9]\d*)$/.test(offset) || !/^(0|[1-9]\d*)$/.test(length))
    throw new Error(`DOB blueprint: invalid byte range ${offset} + ${length}`);
  const o = BigInt(offset),
    l = BigInt(length);
  if (l === BigInt(0)) return 'no bytes';
  return l === BigInt(1) ? `byte ${o}` : `bytes ${o}–${o + l - BigInt(1)}`;
}
/** A gene's value domain in words: its options, its range, or how many arguments it takes. */
function valueDomain(rule: string, args: unknown): string {
  if (args === null || args === undefined) return '—';
  if (!Array.isArray(args)) return 'see raw';
  if (rule === 'options') {
    const shown = args.slice(0, 3).map((a) => traitDisplay(String(a)));
    return `${args.length} option${args.length === 1 ? '' : 's'}: ${shown.join(', ')}${args.length > 3 ? ', …' : ''}`;
  }
  if (rule === 'range' && args.length === 2 && args.every((a) => typeof a !== 'object'))
    return `${String(args[0])}–${String(args[1])}`;
  return `${args.length} argument${args.length === 1 ? '' : 's'}`;
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
              {/* F2 objects (OB-C12): the blueprint as a table of genes and value domains. */}
              <div className="min-w-0 overflow-x-auto">
                <table aria-label="DOB blueprint" className="w-full min-w-[480px] text-[11px]">
                  <thead className="text-text-dim text-left">
                    <tr>
                      <th className="py-1 pr-3 font-normal">Gene</th>
                      <th className="py-1 pr-3 font-normal">Bytes</th>
                      <th className="py-1 pr-3 font-normal">Rule</th>
                      <th className="py-1 font-normal">Values</th>
                    </tr>
                  </thead>
                  <tbody>
                    {step.description.genes.map((g) => (
                      <tr key={g.index} className="border-base-elevated border-t align-top">
                        <th scope="row" className="text-text-bright py-1.5 pr-3 font-normal">
                          {g.name}
                        </th>
                        <td className="text-text-dim py-1.5 pr-3">
                          byte offset {g.byteOffset} · length {g.byteLength}
                          <span className="block">{byteSpan(g.byteOffset, g.byteLength)}</span>
                        </td>
                        <td className="py-1.5 pr-3">
                          {g.rule} <span className="text-text-dim">· {g.typeTag}</span>
                        </td>
                        <td
                          className="py-1.5 [overflow-wrap:anywhere]"
                          title={
                            g.ruleArguments === null ? undefined : JSON.stringify(g.ruleArguments)
                          }
                        >
                          {valueDomain(g.rule, g.ruleArguments)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </>
          )}
        </article>
      ))}
    </section>
  );
}
