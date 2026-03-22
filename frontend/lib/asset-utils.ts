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

/**
 * Flexible timestamp parsing for activity records.
 * Handles numeric (seconds or milliseconds), ISO strings, and pass-through.
 */
export function formatActivityTimestamp(timestamp: string): string {
  const numeric = Number(timestamp);
  if (Number.isFinite(numeric) && numeric > 0) {
    const milliseconds = numeric >= 1_000_000_000_000 ? numeric : numeric * 1000;
    return new Date(milliseconds).toLocaleString();
  }
  const parsed = Date.parse(timestamp);
  if (Number.isFinite(parsed)) {
    return new Date(parsed).toLocaleString();
  }
  return timestamp;
}

/** Normalize action labels: "burn" → "recycled" for identity-type assets. */
export function normalizeActivityAction(action: string): string {
  if (action.toLowerCase() === 'burn') {
    return 'recycled';
  }
  return action.toLowerCase();
}

/** Map composition tier enum to human-readable label. */
export function formatCompositionTier(
  tier:
    | 'btc_ckb'
    | 'pure_ckb'
    | 'decentralized_mixture'
    | 'centralized_mixture'
    | 'unknown'
    | string
): string {
  if (tier === 'pure_ckb') return 'Pure CKB';
  if (tier === 'btc_ckb') return 'BTC+CKB';
  if (tier === 'decentralized_mixture') return 'Decentralized Mixture';
  if (tier === 'centralized_mixture') return 'Centralized Mixture';
  return 'Unknown';
}

/** Format an optional Unix-second expiry timestamp. */
export function formatExpiry(expiredAt: number | null | undefined): string {
  if (!expiredAt) return 'Not available';
  return new Date(expiredAt * 1000).toLocaleString();
}
