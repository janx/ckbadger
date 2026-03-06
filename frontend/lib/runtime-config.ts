export interface CkbadgerRuntimeConfig {
  apiBase?: string;
  wsUrl?: string;
  ckbNetwork?: string;
  ckbRpcUrl?: string;
}

declare global {
  interface Window {
    __CKBADGER_RUNTIME_CONFIG__?: CkbadgerRuntimeConfig;
  }
}

export const DEFAULT_API_BASE = 'http://localhost:8101/api/v1';
export const DEFAULT_WS_URL = 'ws://localhost:8101/ws';
export const DEFAULT_CKB_NETWORK = 'mainnet';
export const DEFAULT_CKB_RPC_URL = 'http://127.0.0.1:8114';

function runtimeConfigFromWindow(): CkbadgerRuntimeConfig | undefined {
  if (typeof window === 'undefined') {
    return undefined;
  }
  return window.__CKBADGER_RUNTIME_CONFIG__;
}

function trimTrailingSlash(value: string): string {
  return value.replace(/\/+$/, '');
}

export function resolveApiBase(config: CkbadgerRuntimeConfig = runtimeConfigFromWindow() ?? {}) {
  const configured = config.apiBase?.trim();
  return trimTrailingSlash(configured && configured.length > 0 ? configured : DEFAULT_API_BASE);
}

export function resolveWsUrl(config: CkbadgerRuntimeConfig = runtimeConfigFromWindow() ?? {}) {
  const configured = config.wsUrl?.trim();
  return configured && configured.length > 0 ? configured : DEFAULT_WS_URL;
}

export function resolveCkbNetwork(config: CkbadgerRuntimeConfig = runtimeConfigFromWindow() ?? {}) {
  const configured = config.ckbNetwork?.trim();
  return configured && configured.length > 0 ? configured : DEFAULT_CKB_NETWORK;
}

export function resolveCkbRpcUrl(config: CkbadgerRuntimeConfig = runtimeConfigFromWindow() ?? {}) {
  const configured = config.ckbRpcUrl?.trim();
  return configured && configured.length > 0 ? configured : DEFAULT_CKB_RPC_URL;
}
