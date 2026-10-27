'use client';
import { useState } from 'react';
import type { CollectionInsights, CurrentPattern, ObjectGenetics } from '@/lib/api';
import { AlleleGel, type DnaGene } from '@/components/ui/allele-gel';
import { DnaStrip } from '@/components/ui/dna-strip';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { hexBytes } from '@/lib/hex-bytes';
import { HUE_RAMP } from '@/lib/design-tokens';

export interface ObjectGeneticsViewProps {
  evidence: ObjectGenetics;
  subject: string;
  collection?: CollectionInsights['genetics'];
}
export function ObjectGeneticsView({ evidence, subject, collection }: ObjectGeneticsViewProps) {
  const { dnaHex, cachedVmTraits: cached, currentPattern: pattern } = evidence;
  if (dnaHex === null)
    return <p className="text-text-dim text-[11px]">This object has no DOB DNA.</p>;
  const dna = hexBytes(dnaHex, subject);
  const genes: DnaGene[] = [];
  const unmapped: string[] = [];
  if (pattern.status === 'dob')
    for (const step of pattern.steps) {
      if (step.description.status !== 'supported') continue;
      for (const map of step.bytes) {
        const definition = step.description.genes.find((g) => g.index === map.geneIndex);
        if (!definition)
          throw new Error(
            `${subject}: decoder step ${step.step} maps missing gene ${map.geneIndex}`
          );
        if (
          dna.slice(map.byteOffset, map.byteOffset + map.byteLength).join('') !==
          map.rawHex.replace(/^0x/, '').toLowerCase()
        )
          throw new Error(`${subject}: gene ${definition.name} bytes disagree with DNA`);
        const declared = `Declared bytes ${map.declaredByteOffset} + ${map.declaredByteLength}`;
        if (map.byteLength === 0) {
          unmapped.push(
            `${definition.name}: ${declared}; no bytes of this DNA fall in the declared range.`
          );
          continue;
        }
        const traits = cached.traits.filter((t) => t.name === definition.name);
        genes.push({
          id: `${step.step}:${map.geneIndex}`,
          label: definition.name,
          offset: map.byteOffset,
          length: map.byteLength,
          value: traits.length
            ? `Cached VM result: ${traits.map((t) => t.value).join(' · ')}`
            : 'No cached VM value for this gene',
          description: `${declared}. ${map.truncated ? 'Clipped to the actual DNA shown. ' : ''}${definition.rule}; raw 0x${map.rawHex.replace(/^0x/, '')}.`,
          color: HUE_RAMP[genes.length % HUE_RAMP.length],
        });
      }
    }
  return (
    <section aria-label="DNA and expression" className="min-w-0 space-y-4">
      <div className="flex flex-wrap items-baseline gap-3">
        <h2 className="text-text-bright text-[15px]">DNA & expression</h2>
        <span className="text-text-dim text-[11px]">
          {dna.length} bytes{evidence.number !== null ? ` · number ${evidence.number}` : ''}
        </span>
      </div>
      <p className="text-text-dim text-[11px]">
        The byte map describes the current decoder. Trait values are cached VM results
        {cached.decodedAt !== null ? `, decoded at Unix second ${cached.decodedAt}` : ''}; the
        original decoder binary hash was not recorded. These are separate pieces of evidence.
      </p>
      {cached.status === 'pending' && (
        <p role="status" className="text-text-dim">
          Decode pending · awaiting DOB execution.
        </p>
      )}
      {cached.status === 'failed' && (
        <p role="alert">DOB execution failed: {cached.reason ?? 'Failure reason unavailable'}</p>
      )}
      {genes.length ? (
        <AlleleGel dna={dnaHex} genes={genes} subject={subject} />
      ) : (
        <DnaStrip dna={dnaHex} label={`${subject} DNA`} />
      )}
      {unmapped.map((line) => (
        <p key={line} className="text-text-dim text-[11px]">
          {line}
        </p>
      ))}
      {pattern.status === 'dob' &&
        pattern.steps.map((step) =>
          step.description.status === 'unsupported' ? (
            <p key={step.step} role="status" className="text-text-dim text-[11px]">
              Decoder {step.step + 1}: byte mapping unavailable — {step.description.reason}
            </p>
          ) : null
        )}
      {cached.traits.length > 0 && (
        <dl aria-label="Cached VM traits" className="grid grid-cols-2 gap-3 sm:grid-cols-3">
          {cached.traits.map((t, i) => (
            <div key={`${t.name}:${i}`}>
              <dt className="text-text-dim text-[11px]">{t.name}</dt>
              <dd className="text-text-bright break-words">
                {t.value}
                {collection?.complete && (
                  <span className="text-text-dim block text-[11px]">
                    {(() => {
                      const count = collection.traits
                        .find((g) => g.name === t.name)
                        ?.values.find((v) => v.value === t.value)?.count;
                      return count === undefined
                        ? 'Variant absent from the current collection result'
                        : `${count} of ${collection.eligibleItems} live DOB items`;
                    })()}
                  </span>
                )}
              </dd>
            </div>
          ))}
        </dl>
      )}
    </section>
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
              <div className="grid gap-2 sm:grid-cols-2">
                {trait.values.map((v) => {
                  if (
                    !Number.isSafeInteger(v.count) ||
                    v.count < 0 ||
                    v.count > evidence.eligibleItems
                  )
                    throw new Error(`Collection trait ${trait.name}: invalid count ${v.count}`);
                  return (
                    <button
                      key={v.value}
                      aria-pressed={selected[trait.name] === v.value}
                      onClick={() =>
                        onSelect(trait.name, selected[trait.name] === v.value ? null : v.value)
                      }
                      onFocus={() => setActive(trait.name)}
                      onBlur={() => setActive(null)}
                      className="border-base-border relative min-w-0 border-b px-1 py-2 text-left"
                    >
                      <span
                        aria-hidden
                        className="absolute inset-y-0 left-0 opacity-15"
                        style={{
                          background: HUE_RAMP[index % HUE_RAMP.length],
                          width: `${evidence.eligibleItems ? (v.count / evidence.eligibleItems) * 100 : 0}%`,
                        }}
                      />
                      <span className="relative flex items-baseline justify-between gap-3">
                        <span className="min-w-0 break-words">{v.value}</span>
                        <span className="text-text-dim shrink-0 text-[11px]">
                          {v.count} / {evidence.eligibleItems}
                        </span>
                      </span>
                    </button>
                  );
                })}
              </div>
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
