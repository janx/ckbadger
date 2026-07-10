import type {
  Cell,
  CellDep,
  CursorPaginatedResponse,
  MnftItemActivity,
  MnftItemDetail,
  CollectionItem,
  TransactionDetail,
  TransactionLifecycle,
} from '@/lib/api';
import { api } from '@/lib/api';
import type { ParsedRawPage } from '@/lib/ai/raw-route';
import { resolveBuildVersion, resolveCkbRpcUrl } from '@/lib/runtime-config';
import { resolveActiveNetwork } from '@/lib/active-network';
import {
  analyzeWitness,
  buildScriptGroupLens,
  inferWitnessInsights,
  type ScriptGroupLens,
  type WitnessAnalysis,
  type WitnessInference,
} from '@/lib/witness-analysis';

const RAW_SCHEMA_VERSION = '1.1.0';
const RAW_FORMAT = 'raw';
const DEFAULT_PROFILE = 'default';
const DEFAULT_LIMIT = 20;
const MAX_LIMIT = 200;

const RAW_PROFILES = ['default', 'debugger'] as const;
type RawProfile = (typeof RAW_PROFILES)[number];

type RouteKind = Exclude<ParsedRawPage['kind'], 'unknown'>;

const ROUTE_PROFILE_MATRIX: Record<RouteKind, RawProfile[]> = {
  block_detail: ['default'],
  cell_detail: ['default'],
  dotbit_item_detail: ['default'],
  did_ckb_item_detail: ['default'],
  mnft_item_detail: ['default'],
  tx_detail: ['default', 'debugger'],
};

interface RenderRawInput {
  page: ParsedRawPage;
  searchParams: URLSearchParams;
  origin: string;
}

interface RawMeta {
  format: typeof RAW_FORMAT;
  profile: RawProfile;
  schemaVersion: string;
  buildVersion: string;
  network: string;
  path: string;
  canonical: string;
  pageType: ParsedRawPage['kind'];
  generatedAt: string;
}

interface RawErrorPayload {
  code: string;
  message: string;
  details?: Record<string, unknown>;
}

interface RpcError {
  code: number;
  message: string;
}

interface RpcResponse<T> {
  result?: T | null;
  error?: RpcError;
}

interface RpcOutPoint {
  tx_hash: string;
  index: string;
}

interface RpcCellDep {
  out_point: RpcOutPoint;
  dep_type: string;
}

interface RpcScript {
  code_hash: string;
  hash_type: string;
  args: string;
}

interface RpcInput {
  previous_output: RpcOutPoint;
  since: string;
}

interface RpcOutput {
  capacity: string;
  lock: RpcScript;
  type?: RpcScript | null;
}

interface RpcTransaction {
  version: string;
  cell_deps: RpcCellDep[];
  header_deps: string[];
  inputs: RpcInput[];
  outputs: RpcOutput[];
  outputs_data: string[];
  witnesses: string[];
}

interface RpcTransactionWithStatus {
  transaction?: RpcTransaction | null;
}

interface RpcLiveCell {
  output: RpcOutput;
  data: {
    content: string;
  };
}

interface RpcLiveCellResult {
  cell?: RpcLiveCell | null;
}

interface RpcHeader {
  compact_target: string;
  hash: string;
  number: string;
  parent_hash: string;
  nonce: string;
  timestamp: string;
  transactions_root: string;
  proposals_hash: string;
  extra_hash?: string | null;
  uncles_hash?: string | null;
  version: string;
  epoch: string;
  dao: string;
}

interface DebuggerScript {
  code_hash: string;
  hash_type: string;
  args: string;
}

interface DebuggerOutPoint {
  tx_hash: string;
  index: string;
}

interface DebuggerCellDep {
  out_point: DebuggerOutPoint;
  dep_type: string;
}

interface DebuggerInput {
  previous_output: DebuggerOutPoint;
  since: string;
}

interface DebuggerOutput {
  capacity: string;
  lock: DebuggerScript;
  type?: DebuggerScript;
}

interface DebuggerMockInput {
  input: DebuggerInput;
  output: DebuggerOutput;
  data: string;
}

interface DebuggerMockCellDep {
  cell_dep: DebuggerCellDep;
  output: DebuggerOutput;
  data: string;
}

interface DebuggerMockInfo {
  inputs: DebuggerMockInput[];
  cell_deps: DebuggerMockCellDep[];
  header_deps: RpcHeader[];
}

interface DebuggerTransaction {
  version: string;
  cell_deps: DebuggerCellDep[];
  header_deps: string[];
  inputs: DebuggerInput[];
  outputs: DebuggerOutput[];
  outputs_data: string[];
  witnesses: string[];
}

interface DebuggerMockTransaction {
  mock_info: DebuggerMockInfo;
  tx: DebuggerTransaction;
}

interface TxDebuggerData {
  transaction: TransactionDetail;
  cellDeps: CellDep[];
  lifecycle: TransactionLifecycle | null;
  mockTransaction: DebuggerMockTransaction;
  debugger: {
    directRunnable: boolean;
    commandTemplate: string;
    rpcUrl: string;
  };
}

interface TxWitnessData {
  available: boolean;
  witnessesCount: number;
  inputCount: number;
  analyses: WitnessAnalysis[];
  scriptGroupLens: ScriptGroupLens[];
  inference: WitnessInference[];
}

type RawPayload = {
  block?: unknown;
  cell?: Cell;
  dotbitItem?: CollectionItem;
  dotbitActivities?: CursorPaginatedResponse<MnftItemActivity>;
  didCkbItem?: CollectionItem;
  didCkbActivities?: CursorPaginatedResponse<MnftItemActivity>;
  mnftItem?: MnftItemDetail;
  mnftActivities?: CursorPaginatedResponse<MnftItemActivity>;
  transaction?: TransactionDetail;
  txDebugger?: TxDebuggerData;
  txWitness?: TxWitnessData;
};

interface RenderRawOutput {
  status: number;
  body: {
    meta: RawMeta;
    data?: RawPayload;
    error?: RawErrorPayload;
  };
}

export class RawRenderError extends Error {
  status: number;
  code: string;
  details?: Record<string, unknown>;

  constructor(status: number, code: string, message: string, details?: Record<string, unknown>) {
    super(message);
    this.name = 'RawRenderError';
    this.status = status;
    this.code = code;
    this.details = details;
  }
}

function parseProfile(searchParams: URLSearchParams): RawProfile {
  const raw = searchParams.get('profile');
  if (raw === null || raw === '') {
    return DEFAULT_PROFILE;
  }
  if ((RAW_PROFILES as readonly string[]).includes(raw)) {
    return raw as RawProfile;
  }
  throw new RawRenderError(400, 'invalid_profile', `Invalid query param "profile": ${raw}`, {
    allowedProfiles: RAW_PROFILES,
  });
}

function parseLimit(searchParams: URLSearchParams): number {
  const raw = searchParams.get('limit');
  if (raw === null) return DEFAULT_LIMIT;
  if (!/^\d+$/.test(raw)) {
    throw new RawRenderError(400, 'invalid_limit', `Invalid query param "limit": ${raw}`);
  }
  const limit = Number(raw);
  if (!Number.isInteger(limit) || limit < 1 || limit > MAX_LIMIT) {
    throw new RawRenderError(
      400,
      'invalid_limit',
      `Invalid query param "limit": ${raw}. Expected integer in [1, ${MAX_LIMIT}]`
    );
  }
  return limit;
}

function parseMnftActivityAction(raw: string | null): 'mint' | 'transfer' | 'burn' | undefined {
  if (raw === null) return undefined;
  if (raw === 'mint' || raw === 'transfer' || raw === 'burn') {
    return raw;
  }
  throw new RawRenderError(
    400,
    'invalid_action',
    `Invalid query param "action": ${raw}. Expected one of mint,transfer,burn`
  );
}

function parseOutpoint(outpoint: string): { txHash: string; outputIndex: number } {
  const delimiter = outpoint.lastIndexOf('-');
  if (delimiter < 1 || delimiter === outpoint.length - 1) {
    throw new RawRenderError(
      400,
      'invalid_outpoint',
      `Invalid outpoint "${outpoint}". Expected "{txHash}-{outputIndex}"`
    );
  }
  const txHash = outpoint.slice(0, delimiter);
  const rawIndex = outpoint.slice(delimiter + 1);
  if (!/^\d+$/.test(rawIndex)) {
    throw new RawRenderError(400, 'invalid_outpoint', `Invalid outputIndex "${rawIndex}"`);
  }
  const outputIndex = Number(rawIndex);
  if (!Number.isInteger(outputIndex) || outputIndex < 0) {
    throw new RawRenderError(400, 'invalid_outpoint', `Invalid outputIndex "${rawIndex}"`);
  }
  return { txHash, outputIndex };
}

function buildMeta(
  pathname: string,
  profile: RawProfile,
  pageType: ParsedRawPage['kind'],
  origin: string
): RawMeta {
  return {
    format: RAW_FORMAT,
    profile,
    schemaVersion: RAW_SCHEMA_VERSION,
    buildVersion: resolveBuildVersion(),
    network: resolveActiveNetwork(),
    path: pathname,
    canonical: `${origin}${pathname}`,
    pageType,
    generatedAt: new Date().toISOString(),
  };
}

function mapApiErrorToRawError(error: unknown): RawRenderError {
  if (error instanceof RawRenderError) {
    return error;
  }
  const message = error instanceof Error ? error.message : String(error);
  const statusMatch = message.match(/API error:\s*(\d{3})/);
  const status = statusMatch ? Number(statusMatch[1]) : 502;
  const code =
    status === 404
      ? 'upstream_not_found'
      : status === 400
        ? 'upstream_bad_request'
        : 'upstream_error';
  return new RawRenderError(status, code, message);
}

function profileAllowedForRoute(page: ParsedRawPage, profile: RawProfile): boolean {
  if (page.kind === 'unknown') {
    return true;
  }
  return ROUTE_PROFILE_MATRIX[page.kind].includes(profile);
}

function buildErrorBody(meta: RawMeta, error: RawRenderError): RenderRawOutput['body'] {
  return {
    meta,
    error: {
      code: error.code,
      message: error.message,
      details: error.details,
    },
  };
}

function getCkbRpcUrl(): string {
  return resolveCkbRpcUrl();
}

function normalizeHex(hex: string): string {
  return hex.startsWith('0x') ? hex : `0x${hex}`;
}

function parseHexIndex(value: string): number {
  const normalized = value.startsWith('0x') ? value.slice(2) : value;
  if (normalized.length === 0 || !/^[0-9a-fA-F]+$/.test(normalized)) {
    throw new RawRenderError(502, 'invalid_rpc_index', `Invalid hex index from RPC: ${value}`);
  }
  const index = Number.parseInt(normalized, 16);
  if (!Number.isInteger(index) || index < 0) {
    throw new RawRenderError(502, 'invalid_rpc_index', `Invalid hex index from RPC: ${value}`);
  }
  return index;
}

function outPointKey(outPoint: RpcOutPoint | DebuggerOutPoint): string {
  return `${normalizeHex(outPoint.tx_hash).toLowerCase()}:${outPoint.index.toLowerCase()}`;
}

function decodeDepGroupOutPoints(data: string): RpcOutPoint[] {
  const raw = data.startsWith('0x') ? data.slice(2) : data;
  let bytes: Uint8Array;
  try {
    bytes = Uint8Array.from(Buffer.from(raw, 'hex'));
  } catch (error) {
    throw new RawRenderError(
      502,
      'dep_group_decode_error',
      `Failed to decode dep_group hex data: ${error instanceof Error ? error.message : String(error)}`
    );
  }
  if (bytes.length < 4) {
    throw new RawRenderError(502, 'dep_group_decode_error', 'dep_group data too short');
  }

  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const count = view.getUint32(0, true);
  const outPoints: RpcOutPoint[] = [];
  let offset = 4;
  for (let i = 0; i < count; i += 1) {
    if (offset + 36 > bytes.length) {
      throw new RawRenderError(502, 'dep_group_decode_error', 'dep_group data truncated');
    }
    const txHashBytes = bytes.slice(offset, offset + 32);
    const index = view.getUint32(offset + 32, true);
    const txHash = `0x${Buffer.from(txHashBytes).toString('hex')}`;
    outPoints.push({ tx_hash: txHash, index: `0x${index.toString(16)}` });
    offset += 36;
  }

  return outPoints;
}

function toDebuggerScript(script: RpcScript): DebuggerScript {
  return {
    code_hash: script.code_hash,
    hash_type: script.hash_type,
    args: script.args,
  };
}

function toDebuggerOutput(output: RpcOutput): DebuggerOutput {
  const typeScript = output.type ? { type: toDebuggerScript(output.type) } : {};
  return {
    capacity: output.capacity,
    lock: toDebuggerScript(output.lock),
    ...typeScript,
  };
}

function toDebuggerInput(input: RpcInput): DebuggerInput {
  return {
    previous_output: {
      tx_hash: normalizeHex(input.previous_output.tx_hash),
      index: input.previous_output.index,
    },
    since: input.since,
  };
}

function toDebuggerCellDep(dep: RpcCellDep): DebuggerCellDep {
  return {
    out_point: {
      tx_hash: normalizeHex(dep.out_point.tx_hash),
      index: dep.out_point.index,
    },
    dep_type: dep.dep_type,
  };
}

async function rpcCall<T>(rpcUrl: string, method: string, params: unknown[]): Promise<T> {
  const response = await fetch(rpcUrl, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      id: 1,
      jsonrpc: '2.0',
      method,
      params,
    }),
  });

  if (!response.ok) {
    throw new RawRenderError(
      502,
      'rpc_http_error',
      `CKB RPC request failed with status ${response.status} for method "${method}"`,
      { method, rpcUrl, status: response.status }
    );
  }

  let payload: RpcResponse<T>;
  try {
    payload = (await response.json()) as RpcResponse<T>;
  } catch (error) {
    throw new RawRenderError(
      502,
      'rpc_parse_error',
      `Failed to parse CKB RPC response for "${method}": ${
        error instanceof Error ? error.message : String(error)
      }`,
      { method, rpcUrl }
    );
  }

  if (payload.error) {
    throw new RawRenderError(
      502,
      'rpc_error',
      `CKB RPC error for "${method}": ${payload.error.message}`,
      { method, rpcUrl, code: payload.error.code }
    );
  }

  return payload.result as T;
}

async function fetchRpcTransaction(rpcUrl: string, txHash: string): Promise<RpcTransaction> {
  const result = await rpcCall<RpcTransactionWithStatus | null>(rpcUrl, 'get_transaction', [
    normalizeHex(txHash),
  ]);

  const tx = result?.transaction;
  if (!tx) {
    throw new RawRenderError(
      404,
      'tx_not_found',
      `Transaction not found in CKB RPC: ${normalizeHex(txHash)}`
    );
  }
  return tx;
}

async function fetchCellFromTxByOutPoint(
  rpcUrl: string,
  outPoint: RpcOutPoint
): Promise<{ output: RpcOutput; data: string }> {
  const tx = await fetchRpcTransaction(rpcUrl, outPoint.tx_hash);
  const outputIndex = parseHexIndex(outPoint.index);
  const output = tx.outputs[outputIndex];
  if (!output) {
    throw new RawRenderError(
      502,
      'rpc_output_not_found',
      `Output ${outPoint.index} not found in transaction ${normalizeHex(outPoint.tx_hash)}`
    );
  }
  return {
    output,
    data: tx.outputs_data[outputIndex] ?? '0x',
  };
}

async function fetchCellWithData(
  rpcUrl: string,
  outPoint: RpcOutPoint
): Promise<{ output: RpcOutput; data: string }> {
  const normalizedOutPoint = {
    tx_hash: normalizeHex(outPoint.tx_hash),
    index: outPoint.index,
  };
  const result = await rpcCall<RpcLiveCellResult>(rpcUrl, 'get_live_cell', [
    normalizedOutPoint,
    true,
  ]);

  if (result.cell?.output && result.cell.data?.content) {
    return {
      output: result.cell.output,
      data: result.cell.data.content,
    };
  }

  return fetchCellFromTxByOutPoint(rpcUrl, normalizedOutPoint);
}

async function fetchRpcHeader(rpcUrl: string, blockHash: string): Promise<RpcHeader> {
  const header = await rpcCall<RpcHeader | null>(rpcUrl, 'get_header', [normalizeHex(blockHash)]);
  if (!header) {
    throw new RawRenderError(
      502,
      'rpc_header_not_found',
      `Header not found in CKB RPC: ${normalizeHex(blockHash)}`
    );
  }
  return header;
}

async function buildDebuggerMockTransaction(
  rpcUrl: string,
  txHash: string
): Promise<DebuggerMockTransaction> {
  const tx = await fetchRpcTransaction(rpcUrl, txHash);

  const mockInputs = await Promise.all(
    tx.inputs.map(async (input) => {
      const { output, data } = await fetchCellWithData(rpcUrl, input.previous_output);
      return {
        input: toDebuggerInput(input),
        output: toDebuggerOutput(output),
        data,
      };
    })
  );

  const mockCellDeps: DebuggerMockCellDep[] = [];
  const seenOutPoints = new Set<string>();
  for (const cellDep of tx.cell_deps) {
    const { output, data } = await fetchCellWithData(rpcUrl, cellDep.out_point);
    const dep = toDebuggerCellDep(cellDep);
    mockCellDeps.push({
      cell_dep: dep,
      output: toDebuggerOutput(output),
      data,
    });
    seenOutPoints.add(outPointKey(dep.out_point));

    if (cellDep.dep_type !== 'dep_group') {
      continue;
    }

    const referencedOutPoints = decodeDepGroupOutPoints(data);
    for (const referencedOutPoint of referencedOutPoints) {
      if (seenOutPoints.has(outPointKey(referencedOutPoint))) {
        continue;
      }
      const { output: refOutput, data: refData } = await fetchCellWithData(
        rpcUrl,
        referencedOutPoint
      );
      const depOutPoint: DebuggerOutPoint = {
        tx_hash: normalizeHex(referencedOutPoint.tx_hash),
        index: referencedOutPoint.index,
      };
      mockCellDeps.push({
        cell_dep: {
          out_point: depOutPoint,
          dep_type: 'code',
        },
        output: toDebuggerOutput(refOutput),
        data: refData,
      });
      seenOutPoints.add(outPointKey(depOutPoint));
    }
  }

  const headerDeps = await Promise.all(tx.header_deps.map((hash) => fetchRpcHeader(rpcUrl, hash)));

  return {
    mock_info: {
      inputs: mockInputs,
      cell_deps: mockCellDeps,
      header_deps: headerDeps,
    },
    tx: {
      version: tx.version,
      cell_deps: tx.cell_deps.map((dep) => toDebuggerCellDep(dep)),
      header_deps: tx.header_deps,
      inputs: tx.inputs.map((input) => toDebuggerInput(input)),
      outputs: tx.outputs.map((output) => toDebuggerOutput(output)),
      outputs_data: tx.outputs_data,
      witnesses: tx.witnesses,
    },
  };
}

async function renderTxDebuggerPayload(hash: string): Promise<TxDebuggerData> {
  const rpcUrl = getCkbRpcUrl();
  const tx = await api.getTransactionDetail(hash);
  const [lifecycle, cellDeps, mockTransaction] = await Promise.all([
    tx.isCellbase ? Promise.resolve(null) : api.getTransactionLifecycle(hash),
    api.getTransactionCellDeps(hash),
    buildDebuggerMockTransaction(rpcUrl, hash),
  ]);

  return {
    transaction: tx,
    cellDeps,
    lifecycle,
    mockTransaction,
    debugger: {
      directRunnable: true,
      commandTemplate:
        'curl "<url>.raw?profile=debugger" | jq \'.data.txDebugger.mockTransaction\' > mock_tx.json && ckb-debugger --tx-file mock_tx.json --cell-index 0 --cell-type input --script-group-type lock',
      rpcUrl,
    },
  };
}

function buildTxWitnessData(tx: TransactionDetail): TxWitnessData {
  const witnesses = tx.witnesses ?? [];
  const analyses = witnesses.map((witness, index) =>
    analyzeWitness(witness, index, tx.inputsCount)
  );
  const scriptGroupLens = buildScriptGroupLens(tx);
  const inference = inferWitnessInsights(tx, analyses, scriptGroupLens);

  return {
    available: tx.witnessesAvailable ?? witnesses.length > 0,
    witnessesCount: witnesses.length,
    inputCount: tx.inputsCount,
    analyses,
    scriptGroupLens,
    inference,
  };
}

export async function renderRawPage(input: RenderRawInput): Promise<RenderRawOutput> {
  const { page, searchParams, origin } = input;
  const profile = parseProfile(searchParams);
  const meta = buildMeta(page.pathname, profile, page.kind, origin);

  if (page.kind === 'unknown') {
    return {
      status: 404,
      body: {
        meta,
        error: {
          code: 'unknown_page',
          message: `No raw renderer is registered for "${page.pathname}"`,
        },
      },
    };
  }

  if (!profileAllowedForRoute(page, profile)) {
    const allowedProfiles = ROUTE_PROFILE_MATRIX[page.kind];
    throw new RawRenderError(
      400,
      'profile_not_supported',
      `Profile "${profile}" is not supported for "${page.pathname}"`,
      { allowedProfiles }
    );
  }

  try {
    switch (page.kind) {
      case 'block_detail': {
        const block = await api.getBlock(page.id);
        return {
          status: 200,
          body: { meta, data: { block } },
        };
      }
      case 'cell_detail': {
        const { txHash, outputIndex } = parseOutpoint(page.outpoint);
        const cell = await api.getCell(txHash, outputIndex);
        return {
          status: 200,
          body: { meta, data: { cell } },
        };
      }
      case 'tx_detail': {
        if (profile === 'debugger') {
          const txDebugger = await renderTxDebuggerPayload(page.hash);
          const txWitness = buildTxWitnessData(txDebugger.transaction);
          return {
            status: 200,
            body: { meta, data: { txDebugger, txWitness } },
          };
        }
        const transaction = await api.getTransactionDetail(page.hash);
        const txWitness = buildTxWitnessData(transaction);
        return {
          status: 200,
          body: { meta, data: { transaction, txWitness } },
        };
      }
      case 'dotbit_item_detail': {
        const limit = parseLimit(searchParams);
        const cursor = searchParams.get('cursor') ?? undefined;
        const action = parseMnftActivityAction(searchParams.get('action'));
        const [dotbitItem, dotbitActivities] = await Promise.all([
          api.getDotbitItemDetail(page.identityId),
          api.getDotbitItemActivities(page.identityId, {
            limit,
            cursor,
            action,
          }),
        ]);
        return {
          status: 200,
          body: { meta, data: { dotbitItem, dotbitActivities } },
        };
      }
      case 'did_ckb_item_detail': {
        const limit = parseLimit(searchParams);
        const cursor = searchParams.get('cursor') ?? undefined;
        const action = parseMnftActivityAction(searchParams.get('action'));
        const [didCkbItem, didCkbActivities] = await Promise.all([
          api.getDidCkbItemDetail(page.identityId),
          api.getDidCkbItemActivities(page.identityId, {
            limit,
            cursor,
            action,
          }),
        ]);
        return {
          status: 200,
          body: { meta, data: { didCkbItem, didCkbActivities } },
        };
      }
      case 'mnft_item_detail': {
        const limit = parseLimit(searchParams);
        const cursor = searchParams.get('cursor') ?? undefined;
        const action = parseMnftActivityAction(searchParams.get('action'));
        const [mnftItem, mnftActivities] = await Promise.all([
          api.getMnftItemDetail(page.objectId),
          api.getMnftItemActivities(page.objectId, {
            limit,
            cursor,
            action,
          }),
        ]);
        return {
          status: 200,
          body: { meta, data: { mnftItem, mnftActivities } },
        };
      }
    }
  } catch (error) {
    const mapped = mapApiErrorToRawError(error);
    throw mapped;
  }
}

export function buildRawErrorResponse(
  pathname: string,
  profile: string | null,
  origin: string,
  pageType: ParsedRawPage['kind'],
  error: RawRenderError
): RenderRawOutput {
  const fallbackProfile =
    profile && (RAW_PROFILES as readonly string[]).includes(profile)
      ? (profile as RawProfile)
      : DEFAULT_PROFILE;
  const meta = buildMeta(pathname, fallbackProfile, pageType, origin);
  return {
    status: error.status,
    body: buildErrorBody(meta, error),
  };
}
