import {
  DEFAULT_API_BASE,
  DEFAULT_BUILD_VERSION,
  DEFAULT_CKB_NETWORK,
  DEFAULT_CKB_RPC_URL,
  DEFAULT_WS_URL,
  resolveApiBase,
  resolveApiBasePattern,
  resolveBuildVersion,
  resolveCkbNetwork,
  resolveCkbRpcUrl,
  resolveDefaultNetwork,
  resolveNetworks,
  resolveWsUrl,
  resolveWsUrlPattern,
} from '@/lib/runtime-config';

describe('runtime config helpers', () => {
  it('uses configured values when present', () => {
    const config = {
      apiBase: 'http://127.0.0.1:9101/api/v1/',
      wsUrl: 'ws://127.0.0.1:9101/ws',
      ckbNetwork: 'testnet',
      ckbRpcUrl: 'http://127.0.0.1:18114',
      buildVersion: '0.1.0+feature/foo@abcdef123456',
    };

    expect(resolveApiBase(config)).toBe('http://127.0.0.1:9101/api/v1');
    expect(resolveWsUrl(config)).toBe('ws://127.0.0.1:9101/ws');
    expect(resolveCkbNetwork(config)).toBe('testnet');
    expect(resolveCkbRpcUrl(config)).toBe('http://127.0.0.1:18114');
    expect(resolveBuildVersion(config)).toBe('0.1.0+feature/foo@abcdef123456');
  });

  it('falls back to defaults when config values are blank or missing', () => {
    const config = {
      apiBase: ' ',
      wsUrl: '',
      ckbNetwork: '   ',
      ckbRpcUrl: '',
      buildVersion: '   ',
    };

    expect(resolveApiBase(config)).toBe(DEFAULT_API_BASE);
    expect(resolveWsUrl(config)).toBe(DEFAULT_WS_URL);
    expect(resolveCkbNetwork(config)).toBe(DEFAULT_CKB_NETWORK);
    expect(resolveCkbRpcUrl(config)).toBe(DEFAULT_CKB_RPC_URL);
    expect(resolveBuildVersion(config)).toBe(DEFAULT_BUILD_VERSION);
  });
});

describe('runtime network resolvers', () => {
  afterEach(() => {
    // jsdom's window persists across tests in a file; clear seeded config so it
    // does not leak into other tests (including the ones above that pass config
    // explicitly and the default-fallback assertions below).
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  it('reads networks and patterns from a seeded window config', () => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
      apiBasePattern: '/api/{network}/v1',
      wsUrlPattern: '/ws/{network}',
    };

    expect(resolveNetworks()).toEqual(['mainnet', 'testnet']);
    expect(resolveDefaultNetwork()).toBe('mainnet');
    expect(resolveApiBasePattern()).toBe('/api/{network}/v1');
    expect(resolveWsUrlPattern()).toBe('/ws/{network}');
  });

  it('defaultNetwork falls back to the first configured network when unset', () => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'testnet' }, { name: 'mainnet' }],
    };

    expect(resolveNetworks()).toEqual(['testnet', 'mainnet']);
    expect(resolveDefaultNetwork()).toBe('testnet');
  });

  it('falls back to defaults when no window config is present', () => {
    delete window.__CKBADGER_RUNTIME_CONFIG__;

    expect(resolveNetworks()).toEqual([DEFAULT_CKB_NETWORK]);
    expect(resolveDefaultNetwork()).toBe(DEFAULT_CKB_NETWORK);
    expect(resolveApiBasePattern()).toBe('/api/{network}/v1');
    expect(resolveWsUrlPattern()).toBe('/ws/{network}');
  });
});
