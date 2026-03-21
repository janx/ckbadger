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

/** Map storage tier enum to human-readable label. */
export function formatStorageTier(
  tier:
    | 'fully_onchain'
    | 'fully_on_ckb'
    | 'fully_on_btc'
    | 'decentralized_external'
    | 'centralized_dependent'
    | 'unknown'
    | string
): string {
  if (tier === 'fully_on_ckb') return 'Fully on CKB';
  if (tier === 'fully_on_btc') return 'Fully on Bitcoin';
  if (tier === 'fully_onchain') return 'Fully On-chain';
  if (tier === 'decentralized_external') return 'Decentralized External';
  if (tier === 'centralized_dependent') return 'Centralized Dependency';
  return 'Unknown';
}

/** Format an optional Unix-second expiry timestamp. */
export function formatExpiry(expiredAt: number | null | undefined): string {
  if (!expiredAt) return 'Not available';
  return new Date(expiredAt * 1000).toLocaleString();
}
