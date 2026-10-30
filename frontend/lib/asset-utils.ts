/**
 * Shared utility functions for asset detail pages (Objects + Identities).
 */

/** Decode URI component and ensure 0x prefix for asset IDs from URL params. */
export function normalizeAssetId(raw: string): string {
  const decoded = decodeURIComponent(raw);
  return decoded.startsWith('0x') ? decoded : `0x${decoded}`;
}

/** Parse a cursor query param into a string or undefined. */
export function parseActivityCursor(raw: string | null): string | undefined {
  if (!raw) return undefined;
  const trimmed = raw.trim();
  return trimmed.length > 0 ? trimmed : undefined;
}

/** Normalize action labels: "burn" → "recycled" for identity-type assets. */
export function normalizeActivityAction(action: string): string {
  if (action.toLowerCase() === 'burn') {
    return 'recycled';
  }
  return action.toLowerCase();
}

/** The object composition tiers the API reports (`CompositionTier` in ckbadger-store). */
export type CompositionTier =
  | 'btc_ckb'
  | 'pure_ckb'
  | 'decentralized_mixture'
  | 'centralized_mixture'
  | 'unknown'
  | 'decode_pending';

/** The short label that always sits beside a composition mark (AssetClasses board wording). */
export const COMPOSITION_TIER_LABELS: Record<CompositionTier, string> = {
  pure_ckb: 'Pure CKB',
  btc_ckb: 'BTC+CKB',
  decentralized_mixture: 'Decentralized mix',
  centralized_mixture: 'Centralized mix',
  unknown: 'Unknown',
  decode_pending: 'Decode pending',
};

/** Map composition tier enum to human-readable label (one label table, board wording). */
export function formatCompositionTier(tier: CompositionTier | string): string {
  return COMPOSITION_TIER_LABELS[tier as CompositionTier] ?? 'Unknown';
}

/** Format an optional Unix-second expiry timestamp. */
export function formatExpiry(expiredAt: number | null | undefined): string {
  if (!expiredAt) return 'Not available';
  return new Date(expiredAt * 1000).toLocaleString();
}
