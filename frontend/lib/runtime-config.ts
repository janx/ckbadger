export interface CkbadgerRuntimeConfig {
  apiBase?: string;
  wsUrl?: string;
  ckbNetwork?: string;
  ckbRpcUrl?: string;
  buildVersion?: string;
  networks?: { name: string }[];
  defaultNetwork?: string;
  apiBasePattern?: string;
  wsUrlPattern?: string;
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
export const DEFAULT_BUILD_VERSION = 'dev';

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

export function resolveBuildVersion(
  config: CkbadgerRuntimeConfig = runtimeConfigFromWindow() ?? {}
) {
  const configured = config.buildVersion?.trim();
  return configured && configured.length > 0 ? configured : DEFAULT_BUILD_VERSION;
}

export function resolveNetworks(
  config: CkbadgerRuntimeConfig = runtimeConfigFromWindow() ?? {}
): string[] {
  const list = config.networks?.map((n) => n.name).filter(Boolean);
  return list && list.length > 0 ? list : [DEFAULT_CKB_NETWORK];
}

export function resolveDefaultNetwork(
  config: CkbadgerRuntimeConfig = runtimeConfigFromWindow() ?? {}
): string {
  return config.defaultNetwork?.trim() || resolveNetworks(config)[0] || DEFAULT_CKB_NETWORK;
}

export function resolveApiBasePattern(
  config: CkbadgerRuntimeConfig = runtimeConfigFromWindow() ?? {}
): string {
  return config.apiBasePattern?.trim() || `/api/{network}/v1`;
}

export function resolveWsUrlPattern(
  config: CkbadgerRuntimeConfig = runtimeConfigFromWindow() ?? {}
): string {
  return config.wsUrlPattern?.trim() || `/ws/{network}`;
}
