import type {
  ScriptCustodyApi,
  ScriptFamilyDetail,
  ScriptHistoryMarkerApi,
  ScriptVersionDetail,
} from '@/lib/api';

/**
 * Reading of a script family's code lineage from the API's own records (versions and history
 * markers). Nothing here classifies a lock or guesses an event: it orders, numbers and pairs
 * what `/scripts/{name}` returned, and throws when two of its records disagree.
 */

/** `0xd1db…b95c`: the board's short form for lock args, code hashes and outpoints. */
export function shortHex(value: string, head = 4, tail = 4): string {
  if (!/^0x[0-9a-fA-F]+$/.test(value)) throw new Error(`Not a hex value: ${value}`);
  return value.length <= 2 + head + tail + 1
    ? value
    : `${value.slice(0, 2 + head)}…${value.slice(-tail)}`;
}

/** Versions in deployment order (v1 = first deployed), the order the Code versions table uses. */
export function orderedVersions(
  family: Pick<ScriptFamilyDetail, 'versions'>
): ScriptVersionDetail[] {
  const key = (version: ScriptVersionDetail) => {
    const times = version.deployments.map((deployment) => deployment.deployedAt);
    return times.length ? Math.min(...times) : (version.deployedAt ?? Number.POSITIVE_INFINITY);
  };
  return [...family.versions].sort(
    (a, b) => key(a) - key(b) || a.versionHash.localeCompare(b.versionHash)
  );
}

/** `versionHash → n` for `vn` labels. */
export function versionNumbers(family: Pick<ScriptFamilyDetail, 'versions'>): Map<string, number> {
  return new Map(orderedVersions(family).map((version, index) => [version.versionHash, index + 1]));
}

const markerOrder = (a: ScriptHistoryMarkerApi, b: ScriptHistoryMarkerApi) =>
  a.blockNumber - b.blockNumber ||
  (a.txIndex ?? 0) - (b.txIndex ?? 0) ||
  (a.event === b.event ? 0 : a.event === 'consumed' ? -1 : 1);

/**
 * The deployed markers of the family's one Type ID reference, oldest first. `null` when the
 * family has no Type ID reference with history, or more than one (then no single lineage exists
 * and the page must not pretend there is one).
 */
export function typeLineage(
  markers: readonly ScriptHistoryMarkerApi[]
): ScriptHistoryMarkerApi[] | null {
  const deployed = markers.filter(
    (marker) => marker.hashType === 'type' && marker.event === 'deployed'
  );
  const references = new Set(deployed.map((marker) => marker.referenceHash));
  if (references.size !== 1) return null;
  return [...deployed].sort(markerOrder);
}

/** Who signed the transaction that deployed `marker`: the custody of the code cell it consumed. */
export function signerOf(
  marker: ScriptHistoryMarkerApi,
  markers: readonly ScriptHistoryMarkerApi[]
): ScriptCustodyApi | null {
  const consumed = markers.filter(
    (other) =>
      other.event === 'consumed' &&
      other.eventTxHash === marker.eventTxHash &&
      other.referenceHash === marker.referenceHash &&
      other.hashType === marker.hashType
  );
  if (consumed.length > 1)
    throw new Error(
      `Upgrade ${marker.eventTxHash} consumed ${consumed.length} code cells of reference ${marker.referenceHash}`
    );
  return consumed[0]?.custody ?? null;
}

export interface CodeHolder {
  /** Lock args of the code cell's lock: the holder's identity. */
  lockArgs: string;
  custody: ScriptCustodyApi;
  /** First deployment into this holder, and the first into the next one (null = still holds). */
  from: ScriptHistoryMarkerApi;
  until: ScriptHistoryMarkerApi | null;
  /** Its signing configuration as revealed by a spend; null while it has never signed. */
  signing: ScriptCustodyApi | null;
  /** Version numbers of the deployments it signed (null: the API left that version unresolved). */
  signed: (number | null)[];
  neverSigned: boolean;
}

function holderKey(custody: ScriptCustodyApi, context: string): string {
  if (custody.lockArgs === null || custody.lockCodeHash === null)
    throw new Error(`${context}: code cell custody has no lock`);
  return `${custody.lockCodeHash}:${custody.lockHashType}:${custody.lockArgs}`;
}

/** The successive holders of the lineage's code cell, oldest first. */
export function codeHolders(
  lineage: readonly ScriptHistoryMarkerApi[],
  markers: readonly ScriptHistoryMarkerApi[],
  numbers: ReadonlyMap<string, number>
): CodeHolder[] {
  const holders: CodeHolder[] = [];
  for (const marker of lineage) {
    const key = holderKey(marker.custody, `deployment ${marker.codeCellTxHash}`);
    const last = holders.at(-1);
    if (last && holderKey(last.custody, 'holder') === key) continue;
    if (last) last.until = marker;
    holders.push({
      lockArgs: marker.custody.lockArgs!,
      custody: marker.custody,
      from: marker,
      until: null,
      signing: null,
      signed: [],
      neverSigned: false,
    });
  }
  for (const holder of holders) {
    const key = holderKey(holder.custody, 'holder');
    const own = markers.filter(
      (marker) => marker.custody.lockArgs !== null && holderKey(marker.custody, 'marker') === key
    );
    holder.signing =
      own.find((marker) => marker.custody.witnessState === 'verified')?.custody ?? null;
    holder.neverSigned = own.some((marker) => marker.custody.witnessState === 'never_signed');
    for (const deployed of lineage) {
      const signer = signerOf(deployed, markers);
      if (signer && signer.lockArgs !== null && holderKey(signer, 'signer') === key) {
        if (deployed.versionHash === null) {
          holder.signed.push(null);
          continue;
        }
        const number = numbers.get(deployed.versionHash);
        if (number === undefined)
          throw new Error(
            `Deployment ${deployed.codeCellTxHash} names version ${deployed.versionHash}, which the family does not list`
          );
        holder.signed.push(number);
      }
    }
  }
  return holders;
}

/** The lineage deployment in force at the end of chart day `day` (`YYYY-MM-DD`), or null before v1. */
export function deploymentOnDay(
  lineage: readonly ScriptHistoryMarkerApi[],
  day: string
): ScriptHistoryMarkerApi | null {
  let current: ScriptHistoryMarkerApi | null = null;
  for (const marker of lineage) if (marker.indexedDay <= day) current = marker;
  return current;
}

/** How the board names a code cell holder: `multisig 0xd1db…b95c`. */
export function holderName(custody: ScriptCustodyApi): string {
  switch (custody.class) {
    case 'multisig':
      return `multisig ${shortHex(custody.lockArgs ?? '0x')}`;
    case 'one_key':
      return `one key ${shortHex(custody.lockArgs ?? '0x')}`;
    case 'unspendable':
      return 'an unspendable lock';
    case 'fixed':
      return 'no one (pinned by data hash)';
    default:
      return custody.lockCodeHash
        ? `lock ${shortHex(custody.lockCodeHash)}`
        : 'an unrecorded holder';
  }
}

/** `3 of 5 signatures`, or why the threshold is not known. */
export function signingText(
  holder: Pick<CodeHolder, 'signing' | 'neverSigned' | 'custody'>
): string {
  if (holder.signing && holder.signing.threshold !== null && holder.signing.keyCount !== null)
    return `${holder.signing.threshold} of ${holder.signing.keyCount} signatures`;
  if (holder.neverSigned) return 'signers not on chain yet';
  if (holder.custody.class === 'multisig') return 'signing configuration unavailable';
  return '';
}
