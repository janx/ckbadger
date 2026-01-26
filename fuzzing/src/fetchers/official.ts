import { config } from '../config';
import type {
  OfficialBlockResponse,
  OfficialTransactionResponse,
  OfficialAddressResponse,
  OfficialDaoResponse,
} from '../types';

const BASE_URL = config.official.baseUrl;

async function fetchJson<T>(endpoint: string): Promise<T> {
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), config.request.timeout);

  try {
    const res = await fetch(`${BASE_URL}${endpoint}`, {
      signal: controller.signal,
      headers: {
        Accept: 'application/vnd.api+json',
        'Content-Type': 'application/vnd.api+json',
      },
    });

    if (!res.ok) {
      throw new Error(`HTTP ${res.status}: ${res.statusText}`);
    }

    return res.json() as Promise<T>;
  } finally {
    clearTimeout(timeoutId);
  }
}

export const officialApi = {
  getBlock: (idOrNumber: string | number): Promise<OfficialBlockResponse> =>
    fetchJson(`/blocks/${idOrNumber}`),

  getTransaction: (hash: string): Promise<OfficialTransactionResponse> =>
    fetchJson(`/transactions/${hash}`),

  getAddress: (address: string): Promise<OfficialAddressResponse> =>
    fetchJson(`/addresses/${address}`),

  getDaoStatistics: (): Promise<OfficialDaoResponse> => fetchJson('/statistics'),
};
