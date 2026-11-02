// Consistency lane E follow-up: the genes a DOB decoder step reads. A step the API maps (a reviewed
// decoder dialect, `crates/dob-decoder/src/pattern.rs`) gives its genes; a step it cannot map gives
// none, but its cluster's stored description still declares the pattern — DOB/0 `dob.pattern`,
// DOB/1 `dob.decoders[step].pattern` — as rows `[name, type, offset, length, rule, args?]`. That
// declaration is chain data and is read for that step only, named as a declaration where it is
// shown. A step with neither throws with the step and its decoder.
import type { CurrentPattern, GeneDefinition } from '@/lib/api';

type DobStep = Extract<CurrentPattern, { status: 'dob' }>['steps'][number];

/** Where a step's genes come from: the API's reviewed byte map, or the description's declaration. */
export type GeneSource = 'decoder' | 'description' | 'none';

const isByteRangeRow = (
  row: unknown
): row is [string, string, number, number, string, ...unknown[]] =>
  Array.isArray(row) &&
  typeof row[0] === 'string' &&
  row[0] !== '' &&
  typeof row[1] === 'string' &&
  Number.isSafeInteger(row[2]) &&
  (row[2] as number) >= 0 &&
  Number.isSafeInteger(row[3]) &&
  (row[3] as number) >= 0 &&
  typeof row[4] === 'string';

const stepName = (step: DobStep) =>
  `decoder step ${step.step + 1} (${step.decoderReference.type} ${step.decoderReference.hash})`;

/**
 * The byte-range pattern the cluster's description declares for `step`, as gene definitions in
 * declaration order; null when that step's declared pattern is not byte ranges (a DOB/1 renderer's
 * composition rows). Throws when the description is absent, not JSON, declares no DOB pattern for
 * the step, names another decoder, or mixes byte-range rows with other rows.
 */
export function declaredStepGenes(
  description: string | null,
  step: DobStep,
  context: string
): GeneDefinition[] | null {
  if (description === null)
    throw new Error(
      `${context}: ${stepName(step)} has no byte mapping and the cluster has no description to declare its pattern`
    );
  let parsed: unknown;
  try {
    parsed = JSON.parse(description);
  } catch {
    throw new Error(
      `${context}: the cluster’s description is not JSON, but ${stepName(step)} needs its pattern`
    );
  }
  const dob = (parsed as { dob?: unknown } | null)?.dob as
    | { decoder?: unknown; pattern?: unknown; decoders?: unknown }
    | undefined;
  const entry = !dob
    ? undefined
    : Array.isArray(dob.decoders)
      ? (dob.decoders[step.step] as { decoder?: unknown; pattern?: unknown } | undefined)
      : step.step === 0 && dob.pattern !== undefined
        ? { decoder: dob.decoder, pattern: dob.pattern }
        : undefined;
  if (!entry || entry.pattern === undefined)
    throw new Error(
      `${context}: decoder step ${step.step + 1} has no byte mapping and the cluster’s description declares no DOB pattern for it`
    );
  const decoder = entry.decoder as { type?: unknown; hash?: unknown } | undefined;
  if (
    !decoder ||
    decoder.type !== step.decoderReference.type ||
    typeof decoder.hash !== 'string' ||
    decoder.hash.toLowerCase() !== step.decoderReference.hash.toLowerCase()
  )
    throw new Error(
      `${context}: the cluster’s description for decoder step ${step.step + 1} names decoder ${String(decoder?.type)} ${String(decoder?.hash)}, not the step’s ${step.decoderReference.type} ${step.decoderReference.hash}`
    );
  if (!Array.isArray(entry.pattern)) return null;
  const rows: unknown[] = entry.pattern;
  const ranged = rows.map(isByteRangeRow);
  if (!ranged.some(Boolean)) return null;
  const odd = ranged.indexOf(false);
  if (odd >= 0)
    throw new Error(
      `${context}: decoder step ${step.step + 1}’s declared pattern mixes byte ranges with other rows (row ${odd + 1})`
    );
  return (rows as [string, string, number, number, string, ...unknown[]][]).map((row, index) => ({
    index,
    traitIndex: index,
    valueIndex: 0,
    name: row[0],
    typeTag: row[1],
    byteOffset: String(row[2]),
    byteLength: String(row[3]),
    rule: row[4],
    ruleArguments: row[5] === undefined ? null : row[5],
  }));
}

/**
 * The genes one decoder step reads, and where they come from. `description` is the cluster's
 * stored description; `undefined` while it is still loading (an unmapped step then reads nothing
 * yet), `null` when the cluster has none.
 */
export function stepGenes(
  step: DobStep,
  description: string | null | undefined,
  context: string
): { source: GeneSource; genes: GeneDefinition[] } {
  if (step.description.status === 'supported')
    return {
      source: 'decoder',
      genes: [...step.description.genes].sort((a, b) => a.index - b.index),
    };
  if (description === undefined) return { source: 'none', genes: [] };
  const declared = declaredStepGenes(description, step, context);
  return declared ? { source: 'description', genes: declared } : { source: 'none', genes: [] };
}
