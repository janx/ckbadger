import type { ItemProtocol, ProtocolVersion } from '@/lib/api';
import type { TenurePeriod } from '@/components/ui/tenure-bar';
import { formatUtc8Date } from '@/lib/utils';

/**
 * How long an identity is held (ID-C7), read from the API's own per-version expiries: a period
 * for the paid term at registration, then one per version whose expiry moved later (a renewal).
 * Nothing here is estimated; every boundary is a decoded `expiredAt` or a version's block time.
 */
export type IdentityTerm =
  | {
      kind: 'term';
      periods: TenurePeriod[];
      /** The current expiry, ms. */
      expiresAt: number;
      /** `.cell`: end of the grace period after expiry, ms (the API's `graceEndsAt`). */
      graceEnd?: number;
      /** Each version that moved the expiry later: when, and the expiry before and after (ms). */
      renewals: { at: number; from: number; to: number }[];
    }
  /** did:ckb has no expiry: the protocol never ends its identifiers. */
  | { kind: 'none' }
  /** A `.bit` version whose cell does not record an expiry (1-based version number). */
  | { kind: 'unrecorded'; version: number };

function secondsMs(value: string, what: string): number {
  if (!/^(0|[1-9]\d*)$/.test(value)) throw new Error(`${what}: invalid Unix seconds ${value}`);
  const ms = Number(value) * 1000;
  if (!Number.isSafeInteger(ms)) throw new Error(`${what}: Unix seconds out of range ${value}`);
  return ms;
}

function blockMs<T>(version: ProtocolVersion<T>, what: string): number {
  const ms = Number(version.cell.timestamp);
  if (!/^\d+$/.test(version.cell.timestamp) || !Number.isSafeInteger(ms))
    throw new Error(`${what}: invalid block timestamp ${version.cell.timestamp}`);
  return ms;
}

function termFrom<T>(
  itemId: string,
  versions: readonly { version: ProtocolVersion<T>; index: number; expires: number }[],
  start: number,
  firstLabel: string,
  graceEndsAt?: number
): IdentityTerm {
  const first = versions[0];
  const periods: TenurePeriod[] = [{ label: firstLabel, start, end: first.expires }];
  const renewals: { at: number; from: number; to: number }[] = [];
  for (let i = 1; i < versions.length; i++) {
    const prev = versions[i - 1].expires,
      { version, index, expires } = versions[i];
    if (expires < prev)
      throw new Error(
        `${itemId}: expiry moved back at version ${index + 1} (${version.cell.txHash}) within one registration`
      );
    if (expires === prev) continue;
    const at = blockMs(version, `${itemId} version ${index + 1}`);
    renewals.push({ at, from: prev, to: expires });
    periods.push({ label: `renewed ${formatUtc8Date(at)}`, start: prev, end: expires });
  }
  const expiresAt = periods[periods.length - 1].end;
  return {
    kind: 'term',
    periods,
    expiresAt,
    renewals,
    ...(graceEndsAt === undefined
      ? {}
      : { graceEnd: secondsMs(String(graceEndsAt), `${itemId} grace end`) }),
  };
}

/** The current term of an identity, from its protocol versions (oldest first, as served). */
export function identityTerm(
  protocol: ItemProtocol,
  options: { graceEndsAt?: number } = {}
): IdentityTerm {
  const id = protocol.itemId;
  if (!protocol.versions.length) throw new Error(`${id}: identity has no protocol versions`);
  switch (protocol.kind) {
    case 'did_ckb':
      return { kind: 'none' };
    case 'dotbit': {
      // The witness names the registration; a recycled and re-registered account starts a new
      // term, so only the trailing versions of the current registration are drawn.
      const registeredAt = protocol.versions.at(-1)!.decoded.registeredAt;
      let from = protocol.versions.length - 1;
      while (from > 0 && protocol.versions[from - 1].decoded.registeredAt === registeredAt) from--;
      const current = protocol.versions
        .slice(from)
        .map((version, i) => ({ version, index: from + i }));
      const unrecorded = current.find(({ version }) => version.decoded.expiredAt === null);
      if (unrecorded) return { kind: 'unrecorded', version: unrecorded.index + 1 };
      return termFrom(
        id,
        current.map(({ version, index }) => ({
          version,
          index,
          expires: secondsMs(version.decoded.expiredAt!, `${id} version ${index + 1} expiry`),
        })),
        secondsMs(registeredAt, `${id} registeredAt`),
        'paid at registration'
      );
    }
    case 'bit_cell':
    case 'dotcell': {
      const versions = (protocol.versions as ProtocolVersion<{ expiredAt: string }>[]).map(
        (version, index) => ({
          version,
          index,
          expires: secondsMs(version.decoded.expiredAt, `${id} version ${index + 1} expiry`),
        })
      );
      return termFrom(
        id,
        versions,
        blockMs(versions[0].version, `${id} version 1`),
        protocol.kind === 'bit_cell' ? 'from minting' : 'registered',
        protocol.kind === 'dotcell' ? options.graceEndsAt : undefined
      );
    }
    default:
      throw new Error(`${id}: ${protocol.kind} is not an identity protocol`);
  }
}

/** Each owner of an identity in order (as `ownerOf` names it), with the version it took over at. */
export function ownerRuns<V>(
  versions: readonly V[],
  ownerOf: (version: V) => string
): { owner: string; since: V }[] {
  const runs: { owner: string; since: V }[] = [];
  for (const version of versions) {
    const owner = ownerOf(version);
    if (!runs.length || runs[runs.length - 1].owner !== owner) runs.push({ owner, since: version });
  }
  return runs;
}

/** The current owner's run: the version it took over at, and how many times the owner changed. */
export function ownerRun<V>(
  versions: readonly V[],
  ownerOf: (version: V) => string
): { since: V; changes: number } {
  const runs = ownerRuns(versions, ownerOf);
  if (!runs.length) throw new Error('owner run of an empty version list');
  return { since: runs[runs.length - 1].since, changes: runs.length - 1 };
}

const DAY = 86_400_000;
/** Whole days between two instants (ms), as every term caption on the identity pages counts them. */
export function termDays(a: number, b: number): number {
  return Math.floor(Math.abs(a - b) / DAY);
}
