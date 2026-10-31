/**
 * Identity records in plain words (ID-DC1 / ID-DC5). `.cell` and `.bit` records are owner-written
 * `key → value` pairs; the keys follow two public namespaces: `address.<coin type>` (SLIP-44 coin
 * types) and `profile.<field>`. This names those keys for people; values are shown exactly as the
 * API decoded them (UTF-8 text, a CKB address the API resolved, or a byte count for binary).
 */
export interface IdentityRecordInput {
  key: string;
  valueUtf8: string | null;
  valueHex: string;
  ttl: number;
  /** A CKB address the API decoded from the value (`.cell` `address.309`), else null. */
  address: string | null;
  /** That address's lock hash, as the API decoded it. */
  addressLockHash?: string | null;
}

export type IdentityRecordValue =
  | { kind: 'text'; text: string }
  | { kind: 'colour'; text: string }
  | { kind: 'bytes'; bytes: number }
  /** J13: bytes that carry a raster image signature, shown as that image (`src` is a data URL). */
  | { kind: 'image'; format: ImageFormat; bytes: number; src: string }
  | { kind: 'empty' };

type ImageFormat = 'WebP' | 'PNG' | 'JPEG' | 'GIF';

/**
 * J13 (ID-DC-N1, user-approved 2026-10-31): the raster image formats a record's bytes are
 * recognised as, by their fixed file signatures (no guessing from the key). SVG is text, so it
 * never reaches here; anything else stays a byte count.
 */
const IMAGE_SIGNATURES: { format: ImageFormat; mime: string; test: (hex: string) => boolean }[] = [
  {
    format: 'WebP',
    mime: 'image/webp',
    // "RIFF" <size, 4 bytes> "WEBP"
    test: (hex) => hex.startsWith('52494646') && hex.slice(16, 24) === '57454250',
  },
  { format: 'PNG', mime: 'image/png', test: (hex) => hex.startsWith('89504e470d0a1a0a') },
  { format: 'JPEG', mime: 'image/jpeg', test: (hex) => hex.startsWith('ffd8ff') },
  {
    format: 'GIF',
    mime: 'image/gif',
    test: (hex) => hex.startsWith('474946383761') || hex.startsWith('474946383961'),
  },
];

function hexToBase64(hex: string): string {
  let binary = '';
  for (let i = 0; i < hex.length; i += 2)
    binary += String.fromCharCode(parseInt(hex.slice(i, i + 2), 16));
  return btoa(binary);
}

export interface IdentityAddressRecord {
  key: string;
  coinType: string;
  coin: string;
  value: IdentityRecordValue;
  ckbAddress: string | null;
  ckbLockHash: string | null;
}
export interface IdentityProfileRecord {
  key: string;
  label: string;
  value: IdentityRecordValue;
}
export interface IdentityRecordsView {
  addresses: IdentityAddressRecord[];
  profile: IdentityProfileRecord[];
  other: { key: string; value: IdentityRecordValue }[];
  /** The distinct cache TTLs, seconds, ascending. */
  ttls: number[];
}

/** SLIP-44 coin types the identity records on CKB actually use. */
const COINS: Record<string, string> = {
  '0': 'Bitcoin',
  '2': 'Litecoin',
  '3': 'Dogecoin',
  '60': 'Ethereum',
  '195': 'Tron',
  '309': 'CKB',
  '501': 'Solana',
};

const PROFILE_ORDER = [
  'name',
  'location',
  'bio',
  'description',
  'website',
  'email',
  'twitter',
  'github',
  'telegram',
  'discord',
  'instagram',
];

const HEX_COLOUR = /^#(?:[0-9a-f]{3}|[0-9a-f]{6})$/i;

function valueOf(record: IdentityRecordInput, colour: boolean): IdentityRecordValue {
  if (!record.valueHex.startsWith('0x'))
    throw new Error(`record ${record.key}: value hex missing 0x prefix`);
  if (record.valueUtf8 === null) {
    const digits = record.valueHex.length - 2;
    if (digits % 2 !== 0) throw new Error(`record ${record.key}: odd-length value hex`);
    const hex = record.valueHex.slice(2).toLowerCase();
    if (!/^[0-9a-f]*$/.test(hex)) throw new Error(`record ${record.key}: value is not hex`);
    const image = IMAGE_SIGNATURES.find((signature) => signature.test(hex));
    if (image)
      return {
        kind: 'image',
        format: image.format,
        bytes: digits / 2,
        src: `data:${image.mime};base64,${hexToBase64(hex)}`,
      };
    return { kind: 'bytes', bytes: digits / 2 };
  }
  const text = record.valueUtf8.trim();
  if (!text) return { kind: 'empty' };
  if (colour && HEX_COLOUR.test(text)) return { kind: 'colour', text };
  return { kind: 'text', text };
}

function fieldLabel(field: string): string {
  const words = field.replace(/[_-]+/g, ' ').trim();
  return words ? words.charAt(0).toUpperCase() + words.slice(1) : '(empty key)';
}

export function describeIdentityRecords(
  records: readonly IdentityRecordInput[]
): IdentityRecordsView {
  const addresses: IdentityAddressRecord[] = [];
  const profile: IdentityProfileRecord[] = [];
  const other: IdentityRecordsView['other'] = [];
  for (const record of records) {
    const address = /^address\.(\d+)$/.exec(record.key);
    if (address) {
      const coinType = address[1];
      addresses.push({
        key: record.key,
        coinType,
        coin: COINS[coinType] ?? `Coin type ${coinType}`,
        value: valueOf(record, false),
        ckbAddress: record.address,
        ckbLockHash: record.address === null ? null : (record.addressLockHash ?? null),
      });
      continue;
    }
    if (record.key.startsWith('profile.')) {
      const field = record.key.slice('profile.'.length);
      profile.push({
        key: record.key,
        label: fieldLabel(field),
        value: valueOf(record, field === 'accent'),
      });
      continue;
    }
    other.push({ key: record.key, value: valueOf(record, false) });
  }
  addresses.sort((a, b) => Number(a.coinType) - Number(b.coinType));
  // Read order: who (name, place, bio), then links, then any other text, the colour, and the
  // binary values last; equal ranks keep their stored order.
  const rank = (p: IdentityProfileRecord) => {
    const field = PROFILE_ORDER.indexOf(p.key.slice('profile.'.length));
    if (field >= 0) return field;
    return p.value.kind === 'text' ? 100 : p.value.kind === 'colour' ? 200 : 300;
  };
  profile.sort((a, b) => rank(a) - rank(b));
  return {
    addresses,
    profile,
    other,
    ttls: [...new Set(records.map((r) => r.ttl))].sort((a, b) => a - b),
  };
}
