import { config } from '../config';
import type {
  CkbadgerBlock,
  CkbadgerTransaction,
  CkbadgerTransactionDetail,
  CkbadgerAddress,
  CkbadgerNetworkStats,
  CkbadgerDaoStatistics,
  CkbadgerToken,
  CursorPaginatedResponse,
} from '../types';

const BASE_URL = config.ckbadger.baseUrl;

async function fetchJson<T>(endpoint: string): Promise<T> {
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), config.request.timeout);

  try {
    const res = await fetch(`${BASE_URL}${endpoint}`, {
      signal: controller.signal,
      headers: { Accept: 'application/json' },
    });

    if (!res.ok) {
      throw new Error(`HTTP ${res.status}: ${res.statusText}`);
    }

    return res.json() as Promise<T>;
  } finally {
    clearTimeout(timeoutId);
  }
}

export const ckbadgerApi = {
  getNetworkStats: (): Promise<CkbadgerNetworkStats> => fetchJson('/statistics/network'),

  getBlock: (id: string | number): Promise<CkbadgerBlock> => fetchJson(`/blocks/${id}`),

  getBlocks: (
    params: { limit?: number; cursor?: string } = {}
  ): Promise<CursorPaginatedResponse<CkbadgerBlock>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchJson(`/blocks?${query}`);
  },

  getTransaction: (hash: string): Promise<CkbadgerTransaction> =>
    fetchJson(`/transactions/${hash}`),

  getTransactionDetail: (hash: string): Promise<CkbadgerTransactionDetail> =>
    fetchJson(`/transactions/${hash}/detail`),

  getTransactions: (
    params: { blockNumber?: number; limit?: number; cursor?: string } = {}
  ): Promise<CursorPaginatedResponse<CkbadgerTransaction>> => {
    const query = new URLSearchParams();
    if (params.blockNumber !== undefined) query.set('block_number', String(params.blockNumber));
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchJson(`/transactions?${query}`);
  },

  getAddress: (addr: string): Promise<CkbadgerAddress> => fetchJson(`/addresses/${addr}`),

  getAddressTransactions: (
    addr: string,
    params: { limit?: number; cursor?: string } = {}
  ): Promise<CursorPaginatedResponse<{ txHash: string }>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchJson(`/addresses/${addr}/transactions?${query}`);
  },

  getTopAddresses: (limit = 100): Promise<CkbadgerAddress[]> =>
    fetchJson(`/addresses/top?limit=${limit}`),

  getActiveAddresses: (
    params: { limit?: number; days?: number } = {}
  ): Promise<CkbadgerAddress[]> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.days) query.set('days', String(params.days));
    return fetchJson(`/addresses/active?${query}`);
  },

  getLiveCells: (
    params: { lockScriptHash?: string; limit?: number; cursor?: string } = {}
  ): Promise<CursorPaginatedResponse<{ txHash: string; outputIndex: number }>> => {
    const query = new URLSearchParams();
    if (params.lockScriptHash) query.set('lock_script_hash', params.lockScriptHash);
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchJson(`/cells/live?${query}`);
  },

  getDaoStatistics: (): Promise<CkbadgerDaoStatistics> => fetchJson('/dao/statistics'),

  getDaoDeposits: (
    params: { limit?: number; cursor?: string } = {}
  ): Promise<CursorPaginatedResponse<{ txHash: string }>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchJson(`/dao/deposits?${query}`);
  },

  getToken: (typeHash: string): Promise<CkbadgerToken> => fetchJson(`/tokens/${typeHash}`),

  getTokens: (
    params: { limit?: number; cursor?: string } = {}
  ): Promise<CursorPaginatedResponse<CkbadgerToken>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchJson(`/tokens?${query}`);
  },

  getTokenHolders: (
    typeHash: string,
    params: { limit?: number; cursor?: string } = {}
  ): Promise<CursorPaginatedResponse<{ lockScriptHash: string }>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchJson(`/tokens/${typeHash}/holders?${query}`);
  },

  getTokenTransfers: (
    typeHash: string,
    params: { limit?: number; cursor?: string } = {}
  ): Promise<CursorPaginatedResponse<{ txHash: string }>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchJson(`/tokens/${typeHash}/transfers?${query}`);
  },

  getBlockProposals: (id: string | number): Promise<{ proposalId: string }[]> =>
    fetchJson(`/blocks/${id}/proposals`),
};
