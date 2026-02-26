export type WitnessRole = 'input' | 'extra';

const WITNESS_PREVIEW_LIMIT_BYTES = 1024;

export interface WitnessSegment {
  label: string;
  start: number;
  end: number;
  meaning: string;
  humanValue: string;
}

export interface WitnessDeterministicDecode {
  kind: string;
  summary: string;
  segments: WitnessSegment[];
}

export interface WitnessHeuristicGuess {
  kind: string;
  confidence: 'high' | 'medium' | 'low';
  reason: string;
  humanValue?: string;
}

export interface WitnessAnalysis {
  index: number;
  rawHex: string;
  byteLength: number;
  previewHex: string;
  previewBytes: number;
  isPreviewTruncated: boolean;
  remainingBytes: number;
  role: WitnessRole;
  deterministic: WitnessDeterministicDecode | null;
  heuristicGuesses: WitnessHeuristicGuess[];
}

export type WitnessInferenceSeverity = 'info' | 'warning' | 'error';

export interface WitnessInference {
  kind: string;
  severity: WitnessInferenceSeverity;
  message: string;
  detail?: string;
  relatedWitnessIndices?: number[];
}

interface MoleculeTable {
  fieldOffsets: number[];
  fields: Array<{ start: number; end: number; data: Uint8Array }>;
  headerEnd: number;
}

export interface ScriptGroupLens {
  key: string;
  kind: 'lock' | 'type';
  codeHash: string;
  hashType: string;
  args: string;
  inputIndices: number[];
  witnessIndex: number;
}

interface TxScriptRef {
  codeHash: string;
  hashType?: string;
  args?: string;
}

interface TxInputRef {
  lock?: TxScriptRef;
  type?: TxScriptRef;
}

export interface WitnessLensTransactionLike {
  inputsCount?: number;
  isCellbase?: boolean;
  witnesses?: string[];
  inputs?: TxInputRef[];
}

function strip0x(hex: string): string {
  return hex.startsWith('0x') ? hex.slice(2) : hex;
}

function readLeU32(bytes: Uint8Array, offset: number): number | null {
  if (offset < 0 || offset + 4 > bytes.length) return null;
  return (
    (bytes[offset] |
      (bytes[offset + 1] << 8) |
      (bytes[offset + 2] << 16) |
      (bytes[offset + 3] << 24)) >>>
    0
  );
}

function parseHexBytes(value: string): Uint8Array | null {
  const raw = strip0x(value).trim();
  if (raw.length === 0) return new Uint8Array(0);
  if (raw.length % 2 !== 0) return null;
  if (!/^[0-9a-fA-F]+$/.test(raw)) return null;

  const out = new Uint8Array(raw.length / 2);
  for (let i = 0; i < raw.length; i += 2) {
    out[i / 2] = parseInt(raw.slice(i, i + 2), 16);
  }
  return out;
}

function bytesToHex(bytes: Uint8Array, maxBytes?: number): string {
  const limit = maxBytes === undefined ? bytes.length : Math.min(bytes.length, maxBytes);
  let out = '0x';
  for (let i = 0; i < limit; i += 1) {
    out += bytes[i].toString(16).padStart(2, '0');
  }
  if (limit < bytes.length) out += '...';
  return out;
}

function toAsciiPreview(bytes: Uint8Array, maxLen: number = 64): string {
  let out = '';
  const limit = Math.min(maxLen, bytes.length);
  for (let i = 0; i < limit; i += 1) {
    const b = bytes[i];
    out += b >= 32 && b <= 126 ? String.fromCharCode(b) : '.';
  }
  if (limit < bytes.length) out += '...';
  return out;
}

function parseMoleculeTable(data: Uint8Array, minFieldCount: number): MoleculeTable | null {
  if (data.length < 8) return null;
  const totalSize = readLeU32(data, 0);
  if (totalSize === null || totalSize !== data.length) return null;

  const firstOffset = readLeU32(data, 4);
  if (
    firstOffset === null ||
    firstOffset < 8 ||
    firstOffset > data.length ||
    firstOffset % 4 !== 0
  ) {
    return null;
  }

  const fieldCount = firstOffset / 4 - 1;
  if (fieldCount < minFieldCount) return null;

  const headerEnd = 4 + fieldCount * 4;
  if (headerEnd !== firstOffset) return null;

  const offsets: number[] = [];
  for (let i = 0; i < fieldCount; i += 1) {
    const offset = readLeU32(data, 4 + i * 4);
    if (offset === null) return null;
    offsets.push(offset);
  }
  offsets.push(data.length);

  for (let i = 0; i < offsets.length - 1; i += 1) {
    const start = offsets[i];
    const end = offsets[i + 1];
    if (start > end || end > data.length) return null;
  }

  const fields = offsets.slice(0, -1).map((start, idx) => {
    const end = offsets[idx + 1];
    return { start, end, data: data.slice(start, end) };
  });

  return { fieldOffsets: offsets.slice(0, -1), fields, headerEnd };
}

function parseMoleculeBytes(data: Uint8Array): Uint8Array | null {
  if (data.length < 4) return null;
  const totalSize = readLeU32(data, 0);
  if (totalSize === null || totalSize !== data.length) return null;
  return data.slice(4);
}

function decodeDasWitnessDeterministic(bytes: Uint8Array): WitnessDeterministicDecode | null {
  if (bytes.length < 7) return null;
  if (!(bytes[0] === 0x64 && bytes[1] === 0x61 && bytes[2] === 0x73)) return null;

  const actionTypeBytes = bytes.slice(3, 7);
  const payload = bytes.slice(7);
  const segments: WitnessSegment[] = [
    {
      label: 'dasPrefix',
      start: 0,
      end: 3,
      meaning: 'DAS witness prefix ("das")',
      humanValue: 'das',
    },
    {
      label: 'actionType',
      start: 3,
      end: 7,
      meaning: 'DAS action type marker',
      humanValue: bytesToHex(actionTypeBytes),
    },
    {
      label: 'actionPayload',
      start: 7,
      end: bytes.length,
      meaning: 'DAS action payload',
      humanValue: bytesToHex(payload, 64),
    },
  ];

  const dataTable = parseMoleculeTable(payload, 3);
  if (dataTable) {
    segments.push({
      label: 'actionPayload.tableHeader',
      start: 7,
      end: 7 + dataTable.headerEnd,
      meaning: 'Molecule table header for DAS Data payload',
      humanValue: bytesToHex(payload.slice(0, dataTable.headerEnd), 32),
    });

    const dataFieldLabels = ['depDataOpt', 'oldDataOpt', 'newDataOpt'];
    dataTable.fields.forEach((field, fieldIndex) => {
      const label = dataFieldLabels[fieldIndex] ?? `dataField${fieldIndex}`;
      segments.push({
        label: `actionPayload.${label}`,
        start: 7 + field.start,
        end: 7 + field.end,
        meaning: 'DAS DataEntityOpt field',
        humanValue: bytesToHex(field.data, 64),
      });
    });

    return {
      kind: 'DASWitness',
      summary: `Decoded DAS witness with ${dataTable.fields.length} data fields`,
      segments,
    };
  }

  return {
    kind: 'DASWitness',
    summary: 'Decoded DAS witness header; payload is not a valid Data molecule table',
    segments,
  };
}

function decodeWitnessDeterministic(bytes: Uint8Array): WitnessDeterministicDecode | null {
  const dasDeterministic = decodeDasWitnessDeterministic(bytes);
  if (dasDeterministic) return dasDeterministic;

  const table = parseMoleculeTable(bytes, 3);
  if (!table) return null;

  if (table.fields.length === 3) {
    const labels = ['lock', 'inputType', 'outputType'] as const;
    const segments: WitnessSegment[] = [
      {
        label: 'tableHeader',
        start: 0,
        end: table.headerEnd,
        meaning: 'Molecule table header and field offsets',
        humanValue: bytesToHex(bytes.slice(0, table.headerEnd)),
      },
    ];

    const values: string[] = [];
    table.fields.forEach((field, idx) => {
      const payload = parseMoleculeBytes(field.data);
      if (field.data.length === 0) {
        values.push(`${labels[idx]}=none`);
        segments.push({
          label: labels[idx],
          start: field.start,
          end: field.end,
          meaning: 'BytesOpt field is empty (None)',
          humanValue: 'none',
        });
        return;
      }
      if (!payload) {
        values.push(`${labels[idx]}=invalid`);
        segments.push({
          label: labels[idx],
          start: field.start,
          end: field.end,
          meaning: 'BytesOpt field is malformed',
          humanValue: bytesToHex(field.data, 32),
        });
        return;
      }
      values.push(`${labels[idx]}=${payload.length}B`);
      segments.push({
        label: labels[idx],
        start: field.start,
        end: field.end,
        meaning: 'WitnessArgs BytesOpt payload',
        humanValue: bytesToHex(payload, 64),
      });

      const xudtTable = parseMoleculeTable(payload, 4);
      if (xudtTable) {
        const payloadStart = field.start + 4;
        segments.push({
          label: `${labels[idx]}.xudtHeader`,
          start: payloadStart,
          end: payloadStart + xudtTable.headerEnd,
          meaning: 'xUDT witness table header',
          humanValue: bytesToHex(payload.slice(0, xudtTable.headerEnd), 32),
        });

        xudtTable.fields.forEach((nestedField, nestedIdx) => {
          segments.push({
            label: `${labels[idx]}.xudtField${nestedIdx}`,
            start: payloadStart + nestedField.start,
            end: payloadStart + nestedField.end,
            meaning: 'xUDT witness table field',
            humanValue: bytesToHex(nestedField.data, 64),
          });
        });

        values[values.length - 1] = `${labels[idx]}=xUDTWitness(${payload.length}B)`;
      }
    });

    return {
      kind: 'WitnessArgs',
      summary: `Decoded as WitnessArgs (${values.join(', ')})`,
      segments,
    };
  }

  const segments: WitnessSegment[] = [
    {
      label: 'tableHeader',
      start: 0,
      end: table.headerEnd,
      meaning: 'Molecule table header and field offsets',
      humanValue: bytesToHex(bytes.slice(0, table.headerEnd)),
    },
    ...table.fields.map((field, idx) => ({
      label: `field${idx}`,
      start: field.start,
      end: field.end,
      meaning: 'Molecule table field',
      humanValue: bytesToHex(field.data, 64),
    })),
  ];

  return {
    kind: 'MoleculeTable',
    summary: `Decoded as molecule table with ${table.fields.length} fields`,
    segments,
  };
}

function decodeWitnessHeuristics(
  bytes: Uint8Array,
  deterministic: WitnessDeterministicDecode | null
): WitnessHeuristicGuess[] {
  const guesses: WitnessHeuristicGuess[] = [];

  if (bytes.length === 0) {
    guesses.push({
      kind: 'empty_witness',
      confidence: 'high',
      reason: 'Witness payload is empty.',
      humanValue: '0x',
    });
    return guesses;
  }

  if (bytes.length >= 3 && bytes[0] === 0x64 && bytes[1] === 0x61 && bytes[2] === 0x73) {
    guesses.push({
      kind: 'dotbit_prefix',
      confidence: 'high',
      reason: 'Starts with "das" header used by .bit witness payloads.',
      humanValue: toAsciiPreview(bytes, 24),
    });
  }

  if (deterministic?.kind === 'WitnessArgs') {
    const lockSegment = deterministic.segments.find((segment) => segment.label === 'lock');
    if (lockSegment) {
      const lockBytes = Math.max(0, lockSegment.end - lockSegment.start - 4);
      if (lockBytes === 65) {
        guesses.push({
          kind: 'signature_shape',
          confidence: 'medium',
          reason: 'Lock field has 65-byte payload, often secp256k1 signature format.',
          humanValue: `${lockBytes} bytes`,
        });
      }
    }
  }

  if (deterministic?.kind === 'DASWitness') {
    guesses.push({
      kind: 'das_witness',
      confidence: 'high',
      reason: 'Deterministic decode matched DAS witness framing.',
      humanValue: deterministic.summary,
    });
  }

  if (
    deterministic?.kind === 'WitnessArgs' &&
    deterministic.segments.some((segment) => segment.label.includes('.xudtField'))
  ) {
    guesses.push({
      kind: 'xudt_extension_witness',
      confidence: 'high',
      reason: 'WitnessArgs field contains a valid xUDT witness table.',
      humanValue: 'xUDT witness structure detected',
    });
  }

  const printableCount = bytes.filter((b) => b >= 32 && b <= 126).length;
  const printableRatio = printableCount / bytes.length;
  if (bytes.length >= 8 && printableRatio >= 0.85) {
    guesses.push({
      kind: 'ascii_payload',
      confidence: 'medium',
      reason: 'Most bytes are printable ASCII characters.',
      humanValue: toAsciiPreview(bytes),
    });
  }

  const firstU32 = readLeU32(bytes, 0);
  if (firstU32 !== null && firstU32 === bytes.length && !deterministic) {
    guesses.push({
      kind: 'molecule_container',
      confidence: 'medium',
      reason: 'First little-endian u32 matches total byte length.',
      humanValue: `total=${bytes.length}`,
    });
  }

  return guesses;
}

export function analyzeWitness(
  rawWitness: string,
  index: number,
  inputsCount: number
): WitnessAnalysis {
  const parsed = parseHexBytes(rawWitness);
  if (!parsed) {
    return {
      index,
      rawHex: strip0x(rawWitness),
      byteLength: 0,
      previewHex: '',
      previewBytes: 0,
      isPreviewTruncated: false,
      remainingBytes: 0,
      role: index < inputsCount ? 'input' : 'extra',
      deterministic: null,
      heuristicGuesses: [
        {
          kind: 'invalid_hex',
          confidence: 'high',
          reason: 'Witness is not valid hex encoding.',
          humanValue: rawWitness,
        },
      ],
    };
  }

  const previewBytes = Math.min(parsed.length, WITNESS_PREVIEW_LIMIT_BYTES);
  const previewSlice = parsed.slice(0, previewBytes);
  const deterministic = decodeWitnessDeterministic(parsed);
  const heuristicGuesses = decodeWitnessHeuristics(parsed, deterministic);

  return {
    index,
    rawHex: bytesToHex(parsed).slice(2),
    byteLength: parsed.length,
    previewHex: bytesToHex(previewSlice).slice(2),
    previewBytes,
    isPreviewTruncated: parsed.length > previewBytes,
    remainingBytes: Math.max(0, parsed.length - previewBytes),
    role: index < inputsCount ? 'input' : 'extra',
    deterministic,
    heuristicGuesses,
  };
}

export function buildScriptGroupLens(tx: WitnessLensTransactionLike): ScriptGroupLens[] {
  if (!tx.inputs || tx.inputs.length === 0) return [];

  const groups = new Map<string, ScriptGroupLens>();
  const addGroup = (script: TxScriptRef | undefined, kind: 'lock' | 'type', inputIndex: number) => {
    if (!script?.codeHash) return;
    const hashType = script.hashType ?? 'unknown';
    const args = script.args ?? '0x';
    const key = `${kind}:${script.codeHash}:${hashType}:${args}`;
    const existing = groups.get(key);
    if (existing) {
      existing.inputIndices.push(inputIndex);
      return;
    }
    groups.set(key, {
      key,
      kind,
      codeHash: script.codeHash,
      hashType,
      args,
      inputIndices: [inputIndex],
      witnessIndex: inputIndex,
    });
  };

  tx.inputs.forEach((input, inputIndex) => {
    addGroup(input.lock, 'lock', inputIndex);
    addGroup(input.type, 'type', inputIndex);
  });

  return Array.from(groups.values())
    .map((group) => ({
      ...group,
      inputIndices: [...group.inputIndices].sort((a, b) => a - b),
      witnessIndex: Math.min(...group.inputIndices),
    }))
    .sort((a, b) => a.witnessIndex - b.witnessIndex);
}

export function inferWitnessInsights(
  tx: WitnessLensTransactionLike,
  witnessAnalyses: WitnessAnalysis[],
  scriptGroupLens: ScriptGroupLens[]
): WitnessInference[] {
  const insights: WitnessInference[] = [];

  const inputsCount = tx.inputsCount ?? tx.inputs?.length ?? 0;
  const witnessesCount = witnessAnalyses.length;
  const inputWitnessIndices = Array.from(
    { length: Math.min(inputsCount, witnessesCount) },
    (_, idx) => idx
  );
  const missingInputWitnessCount = Math.max(0, inputsCount - witnessesCount);
  const isCellbase = tx.isCellbase ?? false;

  if (isCellbase && inputsCount === 0 && witnessesCount === 0) {
    insights.push({
      kind: 'cellbase_without_witness',
      severity: 'info',
      message: 'Cellbase transaction has no witness entries, which is expected.',
    });
  }

  if (missingInputWitnessCount > 0) {
    const missingIndices = Array.from(
      { length: missingInputWitnessCount },
      (_, idx) => witnessesCount + idx
    );
    insights.push({
      kind: 'missing_input_witness',
      severity: 'warning',
      message: `Missing ${missingInputWitnessCount} input witness slot(s).`,
      detail: `Input indices without direct witness slot: [${missingIndices.join(', ')}]`,
      relatedWitnessIndices: missingIndices,
    });
  } else if (inputsCount > 0) {
    insights.push({
      kind: 'input_witness_coverage',
      severity: 'info',
      message: 'All inputs are covered by witness slots.',
      detail: `${inputsCount} input slot(s) mapped to witness[0..${Math.max(0, inputsCount - 1)}].`,
      relatedWitnessIndices: inputWitnessIndices,
    });
  }

  if (witnessesCount > inputsCount) {
    const extraWitnesses = witnessAnalyses.filter((witness) => witness.role === 'extra');
    const nonEmptyExtra = extraWitnesses.filter((witness) => witness.byteLength > 0).length;
    insights.push({
      kind: 'extra_witnesses',
      severity: 'info',
      message: `Transaction has ${witnessesCount - inputsCount} extra witness slot(s).`,
      detail:
        nonEmptyExtra > 0
          ? `${nonEmptyExtra} extra witness slot(s) contain non-empty payload for extension logic.`
          : 'Extra witness slots are empty.',
      relatedWitnessIndices: extraWitnesses.map((witness) => witness.index),
    });
  }

  const invalidHexWitnessIndices = witnessAnalyses
    .filter((witness) => witness.heuristicGuesses.some((guess) => guess.kind === 'invalid_hex'))
    .map((witness) => witness.index);
  if (invalidHexWitnessIndices.length > 0) {
    insights.push({
      kind: 'invalid_witness_hex',
      severity: 'error',
      message: `Detected ${invalidHexWitnessIndices.length} witness entry with invalid hex encoding.`,
      detail: `Witness indices: [${invalidHexWitnessIndices.join(', ')}]`,
      relatedWitnessIndices: invalidHexWitnessIndices,
    });
  }

  const groupsWithMissingWitness = scriptGroupLens.filter(
    (group) => group.witnessIndex >= witnessesCount
  );
  if (groupsWithMissingWitness.length > 0) {
    insights.push({
      kind: 'script_group_missing_witness',
      severity: 'warning',
      message: `${groupsWithMissingWitness.length} script group(s) do not have an available witness slot.`,
      detail: groupsWithMissingWitness
        .slice(0, 3)
        .map(
          (group) =>
            `${group.kind}:${group.codeHash.slice(0, 10)}... -> witness#${group.witnessIndex}`
        )
        .join(' | '),
      relatedWitnessIndices: groupsWithMissingWitness.map((group) => group.witnessIndex),
    });
  }

  const groupsWithEmptyWitness = scriptGroupLens.filter((group) => {
    const witness = witnessAnalyses[group.witnessIndex];
    return witness && witness.byteLength === 0;
  });
  if (groupsWithEmptyWitness.length > 0) {
    insights.push({
      kind: 'script_group_empty_witness',
      severity: 'warning',
      message: `${groupsWithEmptyWitness.length} script group(s) map to empty witness payload.`,
      detail: groupsWithEmptyWitness
        .slice(0, 3)
        .map((group) => `${group.kind}:${group.codeHash.slice(0, 10)}...`)
        .join(' | '),
      relatedWitnessIndices: groupsWithEmptyWitness.map((group) => group.witnessIndex),
    });
  }

  const signatureWitnessIndices = witnessAnalyses
    .filter(
      (witness) =>
        witness.role === 'input' &&
        witness.heuristicGuesses.some((guess) => guess.kind === 'signature_shape')
    )
    .map((witness) => witness.index);
  if (signatureWitnessIndices.length > 0) {
    insights.push({
      kind: 'signature_like_lock_payload',
      severity: 'info',
      message: `${signatureWitnessIndices.length} input witness entry has signature-like lock payload.`,
      detail: `Witness indices: [${signatureWitnessIndices.join(', ')}]`,
      relatedWitnessIndices: signatureWitnessIndices,
    });
  }

  return insights;
}
