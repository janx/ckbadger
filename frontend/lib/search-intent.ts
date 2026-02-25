const HASH32_PATTERN = /^(?:0x)?([a-fA-F0-9]{64})$/;

export type SearchPrefix =
  | 'block'
  | 'tx'
  | 'address'
  | 'cell'
  | 'script'
  | 'token'
  | 'spore'
  | 'cluster';

export interface ParsedSearchIntent {
  rawInput: string;
  normalizedInput: string;
  prefix: SearchPrefix | null;
  body: string;
}

export interface ParsedOutpoint {
  txHash: string;
  outputIndex: number;
  normalized: string;
}

function parsePrefix(value: string): SearchPrefix | null {
  switch (value) {
    case 'block':
      return 'block';
    case 'tx':
      return 'tx';
    case 'addr':
    case 'address':
      return 'address';
    case 'cell':
      return 'cell';
    case 'script':
      return 'script';
    case 'token':
      return 'token';
    case 'spore':
      return 'spore';
    case 'cluster':
      return 'cluster';
    default:
      return null;
  }
}

export function parseSearchIntent(input: string): ParsedSearchIntent {
  const normalizedInput = input.trim();
  const split = normalizedInput.split(':');
  if (split.length < 2) {
    return {
      rawInput: input,
      normalizedInput,
      prefix: null,
      body: normalizedInput,
    };
  }

  const maybePrefix = parsePrefix(split[0].trim().toLowerCase());
  if (!maybePrefix) {
    return {
      rawInput: input,
      normalizedInput,
      prefix: null,
      body: normalizedInput,
    };
  }

  return {
    rawInput: input,
    normalizedInput,
    prefix: maybePrefix,
    body: split.slice(1).join(':').trim(),
  };
}

export function normalizeHash32(value: string): string | null {
  const match = value.trim().match(HASH32_PATTERN);
  if (!match) return null;
  return `0x${match[1].toLowerCase()}`;
}

export function isCkbAddress(value: string): boolean {
  return value.startsWith('ckb1') || value.startsWith('ckt1');
}

function parseOutputIndex(rawValue: string): number | null {
  const trimmed = rawValue.trim();
  if (!trimmed) return null;

  if (/^0x[0-9a-fA-F]+$/.test(trimmed)) {
    const value = Number.parseInt(trimmed.slice(2), 16);
    if (!Number.isInteger(value) || value < 0) return null;
    return value;
  }

  if (/^[0-9]+$/.test(trimmed)) {
    const value = Number.parseInt(trimmed, 10);
    if (!Number.isInteger(value) || value < 0) return null;
    return value;
  }

  return null;
}

export function parseOutpoint(value: string): ParsedOutpoint | null {
  const trimmed = value.trim();
  if (!trimmed) return null;

  const delimiterIndex = Math.max(
    trimmed.lastIndexOf('-'),
    trimmed.lastIndexOf(':'),
    trimmed.lastIndexOf('#')
  );
  if (delimiterIndex < 1 || delimiterIndex >= trimmed.length - 1) {
    return null;
  }

  const txHashRaw = trimmed.slice(0, delimiterIndex);
  const txHash = normalizeHash32(txHashRaw);
  if (!txHash) return null;

  const outputIndex = parseOutputIndex(trimmed.slice(delimiterIndex + 1));
  if (outputIndex === null) return null;

  return {
    txHash,
    outputIndex,
    normalized: `${txHash}-${outputIndex}`,
  };
}
