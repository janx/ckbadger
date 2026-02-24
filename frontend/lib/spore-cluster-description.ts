export interface ClusterMetadataEntry {
  key: string;
  label: string;
  value: string;
}

export interface ParsedClusterDescription {
  summary: string;
  metadataEntries: ClusterMetadataEntry[];
  rawJson: string | null;
  isJson: boolean;
}

const SUMMARY_KEYS = ['description', 'desc', 'summary', 'about', 'bio', 'title', 'name'] as const;
const MAX_METADATA_ENTRIES = 8;
const MAX_VALUE_LENGTH = 120;

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
}

function truncate(value: string, maxLength: number = MAX_VALUE_LENGTH): string {
  if (value.length <= maxLength) {
    return value;
  }
  return `${value.slice(0, maxLength)}...`;
}

function normalizeKeyLabel(key: string): string {
  const withSpaces = key
    .replace(/[_-]+/g, ' ')
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .trim();
  if (!withSpaces) {
    return key;
  }
  return withSpaces.charAt(0).toUpperCase() + withSpaces.slice(1);
}

function formatMetadataValue(value: unknown): string | null {
  if (typeof value === 'string') {
    const trimmed = value.trim();
    if (!trimmed) {
      return null;
    }
    return truncate(trimmed);
  }

  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }

  if (value === null) {
    return 'null';
  }

  if (Array.isArray(value)) {
    if (value.length === 0) {
      return '[]';
    }

    const primitiveItems = value.filter(
      (item) =>
        typeof item === 'string' ||
        typeof item === 'number' ||
        typeof item === 'boolean' ||
        item === null
    );
    if (primitiveItems.length === value.length) {
      return truncate(
        primitiveItems
          .map((item) => {
            if (item === null) {
              return 'null';
            }
            return String(item);
          })
          .join(', ')
      );
    }

    return `${value.length} items`;
  }

  if (typeof value === 'object') {
    const keys = Object.keys(value as Record<string, unknown>);
    return `${keys.length} fields`;
  }

  return null;
}

function extractDobMetadataEntries(value: unknown): ClusterMetadataEntry[] {
  const dob = asRecord(value);
  if (!dob) {
    return [];
  }

  const entries: ClusterMetadataEntry[] = [];
  const version = dob.ver;
  if (typeof version === 'number' || typeof version === 'string') {
    entries.push({
      key: 'dob.ver',
      label: 'DOB Version',
      value: String(version),
    });
  }

  const pattern = dob.pattern;
  if (Array.isArray(pattern)) {
    entries.push({
      key: 'dob.pattern',
      label: 'DOB Pattern Items',
      value: String(pattern.length),
    });
  }

  const decoders = dob.decoders;
  if (Array.isArray(decoders)) {
    entries.push({
      key: 'dob.decoders',
      label: 'DOB Decoders',
      value: String(decoders.length),
    });
  }

  const description = dob.description;
  if (typeof description === 'string' && description.trim()) {
    entries.push({
      key: 'dob.description',
      label: 'DOB Description',
      value: truncate(description.trim()),
    });
  }

  return entries;
}

export function parseSporeClusterDescription(
  description: string | null | undefined
): ParsedClusterDescription | null {
  if (!description) {
    return null;
  }

  const trimmed = description.trim();
  if (!trimmed) {
    return null;
  }

  try {
    const parsed: unknown = JSON.parse(trimmed);
    if (typeof parsed === 'string') {
      const summary = parsed.trim();
      if (!summary) {
        return null;
      }
      return {
        summary,
        metadataEntries: [],
        rawJson: null,
        isJson: false,
      };
    }

    if (typeof parsed !== 'object' || parsed === null) {
      return {
        summary: String(parsed),
        metadataEntries: [],
        rawJson: null,
        isJson: false,
      };
    }

    if (Array.isArray(parsed)) {
      return {
        summary: `JSON metadata array (${parsed.length} items)`,
        metadataEntries: [],
        rawJson: JSON.stringify(parsed, null, 2),
        isJson: true,
      };
    }

    const record = parsed as Record<string, unknown>;
    const metadataEntries: ClusterMetadataEntry[] = [];
    let summary: string | null = null;
    let summaryKey: string | null = null;

    for (const key of SUMMARY_KEYS) {
      const value = record[key];
      if (typeof value === 'string' && value.trim()) {
        summary = value.trim();
        summaryKey = key;
        break;
      }
    }

    for (const [key, value] of Object.entries(record)) {
      if (summaryKey && key === summaryKey) {
        continue;
      }

      if (key === 'dob') {
        for (const entry of extractDobMetadataEntries(value)) {
          metadataEntries.push(entry);
          if (metadataEntries.length >= MAX_METADATA_ENTRIES) {
            break;
          }
        }
        if (metadataEntries.length >= MAX_METADATA_ENTRIES) {
          break;
        }
        continue;
      }

      const formatted = formatMetadataValue(value);
      if (!formatted) {
        continue;
      }

      metadataEntries.push({
        key,
        label: normalizeKeyLabel(key),
        value: formatted,
      });

      if (metadataEntries.length >= MAX_METADATA_ENTRIES) {
        break;
      }
    }

    if (!summary) {
      summary = `JSON metadata (${Object.keys(record).length} keys)`;
    }

    return {
      summary,
      metadataEntries,
      rawJson: JSON.stringify(record, null, 2),
      isJson: true,
    };
  } catch {
    return {
      summary: trimmed,
      metadataEntries: [],
      rawJson: null,
      isJson: false,
    };
  }
}
