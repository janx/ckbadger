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
}

export type IdentityRecordValue =
  | { kind: 'text'; text: string }
  | { kind: 'colour'; text: string }
  | { kind: 'bytes'; bytes: number }
  | { kind: 'empty' };

export interface IdentityAddressRecord {
  key: string;
  coinType: string;
  coin: string;
  value: IdentityRecordValue;
  ckbAddress: string | null;
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

const HEX_COLOUR = /^#(?:[0-9a-f]{3}|[0-9a-f]{6})$/i;

function valueOf(record: IdentityRecordInput, colour: boolean): IdentityRecordValue {
  if (!record.valueHex.startsWith('0x'))
    throw new Error(`record ${record.key}: value hex missing 0x prefix`);
  if (record.valueUtf8 === null) {
    const digits = record.valueHex.length - 2;
    if (digits % 2 !== 0) throw new Error(`record ${record.key}: odd-length value hex`);
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
  return {
    addresses,
    profile,
    other,
    ttls: [...new Set(records.map((r) => r.ttl))].sort((a, b) => a - b),
  };
}
