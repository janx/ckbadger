import { toNftDetailSlug } from '@/lib/nft-collections';

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

export function getNftDetailHref(assetId: string, standard?: string | null): string {
  return `/nfts/${toNftDetailSlug(assetId, standard ?? undefined)}`;
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
