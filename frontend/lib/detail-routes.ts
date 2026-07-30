function hasKnownScriptName(name: string | null | undefined): boolean {
  return Boolean(name && name.trim() && name.trim().toLowerCase() !== 'unknown');
}

function normalizeScriptKind(
  value: string | null | undefined
): 'lock' | 'type' | 'both' | undefined {
  if (value === 'lock' || value === 'type' || value === 'both') return value;
  if (value === 'lock+type') return 'both';
  return undefined;
}

function normalizeHashType(value: string | null | undefined): string | null {
  if (!value) return null;
  const normalized = value.trim().toLowerCase();
  if (!normalized) return null;
  return normalized;
}

export function getTokenDetailHref(typeHash: string): string {
  return `/tokens/${encodeURIComponent(typeHash)}`;
}

export function getClusterDetailHref(clusterId: string): string {
  return `/clusters/${encodeURIComponent(clusterId)}`;
}

export function getObjectDetailHref(assetId: string): string {
  return `/objects/${encodeURIComponent(assetId)}`;
}

export function getMnftClassDetailHref(classId: string): string {
  return `/classes/${encodeURIComponent(classId)}`;
}

export function getIdentityCollectionHref(standard: string, collectionId: string): string {
  if (standard === 'dotbit') return '/identities/dotbit';
  if (standard === 'did_ckb' || standard === 'did:ckb') return '/identities/did:ckb';
  return `/identities/${encodeURIComponent(collectionId)}`;
}

/** Sentinel collection IDs the indexer assigns to the identity standards. */
export const DOTBIT_COLLECTION_ID =
  '0x646f746269745f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f';
export const DID_CKB_COLLECTION_ID =
  '0x6469645f636b625f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f';

export function isDotbitCollectionAlias(value: string): boolean {
  const normalized = value.trim().toLowerCase();
  return normalized === 'dotbit' || normalized === '.bit' || normalized === DOTBIT_COLLECTION_ID;
}

export function isDidCkbCollectionAlias(value: string): boolean {
  const normalized = value.trim().toLowerCase();
  return (
    normalized === 'did:ckb' || normalized === 'did_ckb' || normalized === DID_CKB_COLLECTION_ID
  );
}

/**
 * Byte widths of the identifier classes that can arrive at `/objects/{id}`.
 * They do not overlap, so which resource an identifier denotes is a property of
 * the identifier itself and must never be inferred from an endpoint's error
 * status.
 */
const SPORE_OBJECT_ID_BYTES = 32;
/** mNFT class ID: 20-byte issuer ID + 4-byte class index (parser/mnft.rs). */
const MNFT_CLASS_ID_BYTES = 24;

export type ObjectRouteTarget =
  /** The identifier belongs to another page; send the user there. */
  | { kind: 'redirect'; href: string }
  /** A 32-byte ID: a Spore object, or a 32-byte object/identity collection. */
  | { kind: 'spore-object'; assetId: string }
  /** No object route owns this identifier. */
  | { kind: 'unroutable' };

/** Byte length of a hex identifier, or null when it is not whole-byte hex. */
function hexByteLength(value: string): number | null {
  const body = value.startsWith('0x') || value.startsWith('0X') ? value.slice(2) : value;
  if (body.length === 0 || body.length % 2 !== 0 || !/^[0-9a-f]+$/i.test(body)) {
    return null;
  }
  return body.length / 2;
}

/**
 * Resolve what an `/objects/{id}` identifier actually denotes, before any
 * request is made. mNFT class IDs (24 bytes) and Spore object IDs (32 bytes)
 * are distinguishable by width alone, so the page dispatches on the identifier
 * instead of probing one endpoint and reading its failure as a routing signal.
 */
export function resolveObjectRouteTarget(rawAssetId: string): ObjectRouteTarget {
  const assetId = rawAssetId.trim();
  if (isDotbitCollectionAlias(assetId)) {
    return { kind: 'redirect', href: getIdentityCollectionHref('dotbit', assetId) };
  }
  if (isDidCkbCollectionAlias(assetId)) {
    return { kind: 'redirect', href: getIdentityCollectionHref('did:ckb', assetId) };
  }
  switch (hexByteLength(assetId)) {
    case MNFT_CLASS_ID_BYTES: {
      const canonical = `0x${assetId.replace(/^0x/i, '').toLowerCase()}`;
      return { kind: 'redirect', href: getMnftClassDetailHref(canonical) };
    }
    case SPORE_OBJECT_ID_BYTES:
      return { kind: 'spore-object', assetId };
    default:
      return { kind: 'unroutable' };
  }
}

export function getIdentityItemDetailHref(standard: string, identityId: string): string {
  if (standard === 'dotbit') return `/identities/dotbit/${encodeURIComponent(identityId)}`;
  if (standard === 'did_ckb' || standard === 'did:ckb')
    return `/identities/did/${encodeURIComponent(identityId)}`;
  return `/objects/mnft/${encodeURIComponent(identityId)}`;
}

export function getScriptDetailHref(input: {
  name?: string | null;
  codeHash: string;
  hashType?: string | null;
  scriptKind?: string | null;
}): string {
  if (hasKnownScriptName(input.name)) {
    return `/scripts/${encodeURIComponent(input.name!.trim())}`;
  }

  const query = new URLSearchParams();
  const hashType = normalizeHashType(input.hashType);
  const scriptKind = normalizeScriptKind(input.scriptKind);

  if (hashType) query.set('hashType', hashType);
  if (scriptKind) query.set('kind', scriptKind);

  const suffix = query.toString();
  return `/script/${encodeURIComponent(input.codeHash)}${suffix ? `?${suffix}` : ''}`;
}
