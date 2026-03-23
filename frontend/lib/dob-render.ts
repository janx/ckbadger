const MAX_TEXT_BYTES = 256 * 1024;

export interface SporeDataSegmentLike {
  label: string;
  start: number;
  end: number;
}

export interface SporeCellLike {
  data?: string;
  dataAnalysis?: {
    deterministic?: {
      kind?: string;
      segments?: SporeDataSegmentLike[];
    };
  };
}

export interface SporePayload {
  contentType: string;
  contentBytes: Uint8Array;
  contentHex: string;
  textContent: string | null;
}

export interface DobTrait {
  name: string;
  value: string;
}

export interface DobDecodedContent {
  dnaHex: string | null;
  traits: DobTrait[];
  issues: string[];
}

type JsonRecord = Record<string, unknown>;

interface Dob0PatternElement {
  traitName: string;
  dnaOffset: number;
  dnaLength: number;
  patternType: string;
  traitArgs?: unknown;
  dobType?: string;
}

interface DobMetadata {
  description?: unknown;
  dob?: {
    ver?: unknown;
    pattern?: unknown;
    decoders?: unknown;
  };
}

function asRecord(value: unknown): JsonRecord | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  return value as JsonRecord;
}

function cleanHex(raw: string): string | null {
  const normalized = raw.trim().toLowerCase().replace(/^0x/, '').replace(/\s+/g, '');
  if (!normalized || !/^[0-9a-f]+$/.test(normalized)) {
    return null;
  }
  if (normalized.length % 2 === 1) {
    return `0${normalized}`;
  }
  return normalized;
}

function hexToBytes(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    bytes[i / 2] = Number.parseInt(hex.slice(i, i + 2), 16);
  }
  return bytes;
}

function bytesToHex(bytes: Uint8Array): string {
  let out = '';
  for (let i = 0; i < bytes.length; i += 1) {
    out += bytes[i].toString(16).padStart(2, '0');
  }
  return out;
}

function decodeUtf8(bytes: Uint8Array): string {
  return new TextDecoder('utf-8', { fatal: false }).decode(bytes);
}

function isTextLikeContentType(contentType: string): boolean {
  const normalized = contentType.trim().toLowerCase();
  return (
    normalized.startsWith('text/') ||
    normalized.includes('json') ||
    normalized.includes('xml') ||
    normalized.includes('javascript') ||
    normalized.startsWith('dob/')
  );
}

function getSegmentBounds(
  segments: SporeDataSegmentLike[],
  label: string
): { start: number; end: number } | null {
  const segment = segments.find((item) => item.label === label);
  if (!segment) {
    return null;
  }
  if (!Number.isInteger(segment.start) || !Number.isInteger(segment.end)) {
    return null;
  }
  if (segment.start < 0 || segment.end <= segment.start) {
    return null;
  }
  return { start: segment.start, end: segment.end };
}

function parseDobMetadata(description: string | null | undefined): DobMetadata | null {
  if (!description) {
    return null;
  }
  try {
    const parsed = JSON.parse(description);
    return asRecord(parsed) as DobMetadata | null;
  } catch {
    return null;
  }
}

function parseDnaFromDobContent(contentText: string): string | null {
  const trimmed = contentText.trim();
  if (!trimmed) {
    return null;
  }

  const extractDna = (value: unknown): string | null => {
    if (typeof value === 'string') {
      return cleanHex(value);
    }
    if (Array.isArray(value) && typeof value[0] === 'string') {
      return cleanHex(value[0]);
    }
    const record = asRecord(value);
    if (!record || typeof record.dna !== 'string') {
      return null;
    }
    return cleanHex(record.dna);
  };

  try {
    const parsed = JSON.parse(trimmed);
    return extractDna(parsed);
  } catch {
    return extractDna(trimmed);
  }
}

function parseLeModulo(bytes: Uint8Array, modulo: number): number {
  if (modulo <= 0) {
    return 0;
  }
  let acc = 0;
  let factor = 1 % modulo;
  for (let i = 0; i < bytes.length; i += 1) {
    acc = (acc + (((bytes[i] % modulo) * factor) % modulo)) % modulo;
    factor = (factor * 256) % modulo;
  }
  return acc;
}

function parseLeNumber(bytes: Uint8Array): number {
  let value = 0;
  let factor = 1;
  for (let i = 0; i < bytes.length; i += 1) {
    value += bytes[i] * factor;
    factor *= 256;
  }
  return value;
}

function normalizeDob0PatternElement(value: unknown): Dob0PatternElement | null {
  if (Array.isArray(value)) {
    const [traitName, dobType, dnaOffset, dnaLength, patternType, traitArgs] = value;
    if (
      typeof traitName !== 'string' ||
      !Number.isInteger(dnaOffset) ||
      !Number.isInteger(dnaLength)
    ) {
      return null;
    }
    return {
      traitName,
      dobType: typeof dobType === 'string' ? dobType : undefined,
      dnaOffset,
      dnaLength,
      patternType: typeof patternType === 'string' ? patternType : 'raw',
      traitArgs,
    };
  }

  const record = asRecord(value);
  if (!record) {
    return null;
  }
  if (
    typeof record.traitName !== 'string' ||
    !Number.isInteger(Number(record.dnaOffset)) ||
    !Number.isInteger(Number(record.dnaLength))
  ) {
    return null;
  }
  const dnaOffset = Number(record.dnaOffset);
  const dnaLength = Number(record.dnaLength);
  return {
    traitName: record.traitName,
    dnaOffset,
    dnaLength,
    patternType: typeof record.patternType === 'string' ? record.patternType : 'raw',
    traitArgs: record.traitArgs,
    dobType: typeof record.dobType === 'string' ? record.dobType : undefined,
  };
}

function extractDob0Pattern(metadata: DobMetadata): Dob0PatternElement[] {
  const dob = metadata.dob;
  if (!dob) {
    return [];
  }

  if (dob.ver === 0 || typeof dob.ver !== 'number') {
    if (!Array.isArray(dob.pattern)) {
      return [];
    }
    return dob.pattern
      .map(normalizeDob0PatternElement)
      .filter((item): item is Dob0PatternElement => !!item);
  }

  if (!Array.isArray(dob.decoders)) {
    return [];
  }
  for (const decoderEntry of dob.decoders) {
    const record = asRecord(decoderEntry);
    if (!record || !Array.isArray(record.pattern)) {
      continue;
    }
    const normalized = record.pattern
      .map(normalizeDob0PatternElement)
      .filter((item): item is Dob0PatternElement => !!item);
    if (normalized.length > 0) {
      return normalized;
    }
  }

  return [];
}

function formatUnknownValue(value: unknown): string {
  if (typeof value === 'string') {
    return value;
  }
  if (typeof value === 'number' || typeof value === 'boolean' || typeof value === 'bigint') {
    return String(value);
  }
  if (value === null || value === undefined) {
    return '-';
  }
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function decodeDob0TraitValue(pattern: Dob0PatternElement, dnaSlice: Uint8Array): unknown {
  const kind = pattern.patternType.toLowerCase();
  const rawNumber = parseLeNumber(dnaSlice);

  if (kind === 'options') {
    if (!Array.isArray(pattern.traitArgs) || pattern.traitArgs.length === 0) {
      return null;
    }
    const index = parseLeModulo(dnaSlice, pattern.traitArgs.length);
    return pattern.traitArgs[index];
  }

  if (kind === 'range') {
    if (
      !Array.isArray(pattern.traitArgs) ||
      pattern.traitArgs.length < 2 ||
      typeof pattern.traitArgs[0] !== 'number' ||
      typeof pattern.traitArgs[1] !== 'number'
    ) {
      return null;
    }
    const min = Math.min(pattern.traitArgs[0], pattern.traitArgs[1]);
    const max = Math.max(pattern.traitArgs[0], pattern.traitArgs[1]);
    const width = max - min + 1;
    return min + parseLeModulo(dnaSlice, width);
  }

  if (kind === 'utf8') {
    return decodeUtf8(dnaSlice).replace(/\u0000+$/g, '');
  }

  if (kind === 'rawnumber') {
    if (dnaSlice.length > 6) {
      return `0x${bytesToHex(dnaSlice)}`;
    }
    return rawNumber.toString();
  }

  if (kind === 'rawstring') {
    return `0x${bytesToHex(dnaSlice)}`;
  }

  if (kind === 'raw') {
    if (pattern.dobType?.toLowerCase() === 'number') {
      if (dnaSlice.length > 6) {
        return `0x${bytesToHex(dnaSlice)}`;
      }
      return rawNumber.toString();
    }
    return `0x${bytesToHex(dnaSlice)}`;
  }

  return `0x${bytesToHex(dnaSlice)}`;
}

export function extractSporePayload(cell: SporeCellLike | null | undefined): SporePayload | null {
  if (!cell?.data) {
    return null;
  }

  const deterministic = cell.dataAnalysis?.deterministic;
  const segments = deterministic?.segments;
  if (deterministic?.kind !== 'spore_cell' || !segments || !Array.isArray(segments)) {
    return null;
  }

  const contentTypeSegment = getSegmentBounds(segments, 'content_type');
  const contentSegment = getSegmentBounds(segments, 'content');
  if (!contentTypeSegment || !contentSegment) {
    return null;
  }

  const rawHex = cleanHex(cell.data);
  if (!rawHex) {
    return null;
  }
  const allBytes = hexToBytes(rawHex);

  if (contentTypeSegment.end > allBytes.length || contentSegment.end > allBytes.length) {
    return null;
  }

  const contentTypeBytes = allBytes.slice(contentTypeSegment.start, contentTypeSegment.end);
  const contentBytes = allBytes.slice(contentSegment.start, contentSegment.end);
  const contentType = decodeUtf8(contentTypeBytes)
    .replace(/\u0000/g, '')
    .trim();
  const textContent =
    contentBytes.length <= MAX_TEXT_BYTES && isTextLikeContentType(contentType)
      ? decodeUtf8(contentBytes)
      : null;

  return {
    contentType,
    contentBytes,
    contentHex: bytesToHex(contentBytes),
    textContent,
  };
}

export function decodeDobContent(input: {
  sporeContentType: string;
  contentText: string | null | undefined;
  clusterDescription: string | null | undefined;
}): DobDecodedContent | null {
  if (!input.sporeContentType.toLowerCase().startsWith('dob/')) {
    return null;
  }

  const issues: string[] = [];
  const metadata = parseDobMetadata(input.clusterDescription);
  if (!metadata) {
    issues.push('Missing or invalid DOB metadata in cluster description');
  }

  const dnaHex = input.contentText ? parseDnaFromDobContent(input.contentText) : null;
  if (!dnaHex) {
    issues.push('Missing or invalid DNA in DOB content');
  }

  const traits: DobTrait[] = [];
  if (metadata && dnaHex) {
    const dnaBytes = hexToBytes(dnaHex);
    const patterns = extractDob0Pattern(metadata);
    for (const pattern of patterns) {
      const start = Math.max(0, pattern.dnaOffset);
      const end = Math.min(dnaBytes.length, start + Math.max(0, pattern.dnaLength));
      const slice = end > start ? dnaBytes.slice(start, end) : new Uint8Array();
      const rawValue = decodeDob0TraitValue(pattern, slice);
      traits.push({
        name: pattern.traitName,
        value: formatUnknownValue(rawValue),
      });
    }
    if (patterns.length === 0) {
      issues.push('No DOB/0 pattern found in cluster metadata');
    }
  }

  return { dnaHex, traits, issues };
}
