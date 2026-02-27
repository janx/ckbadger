export const DOTBIT_COLLECTION_ID =
  '0x646f746269745f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f';
export const DID_CKB_COLLECTION_ID =
  '0x6469645f636b625f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f';

export function isDotbitAlias(assetId: string): boolean {
  const normalized = assetId.toLowerCase();
  return normalized === 'dotbit' || normalized === '.bit' || normalized === DOTBIT_COLLECTION_ID;
}

export function isDidCkbAlias(assetId: string): boolean {
  const normalized = assetId.toLowerCase();
  return (
    normalized === 'did:ckb' || normalized === 'did_ckb' || normalized === DID_CKB_COLLECTION_ID
  );
}

export function normalizeNftAssetId(assetId: string): string {
  if (isDotbitAlias(assetId)) {
    return DOTBIT_COLLECTION_ID;
  }
  if (isDidCkbAlias(assetId)) {
    return DID_CKB_COLLECTION_ID;
  }
  return assetId;
}

export function toNftDetailSlug(assetId: string, standard?: string | null): string {
  if (standard === 'dotbit') return 'dotbit';
  if (standard === 'did:ckb' || standard === 'did_ckb') return 'did:ckb';
  if (assetId.toLowerCase() === DOTBIT_COLLECTION_ID) return 'dotbit';
  if (assetId.toLowerCase() === DID_CKB_COLLECTION_ID) return 'did:ckb';
  return assetId;
}
