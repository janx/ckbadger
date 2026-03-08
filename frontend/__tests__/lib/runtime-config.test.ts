import {
  DEFAULT_API_BASE,
  DEFAULT_BUILD_VERSION,
  DEFAULT_CKB_NETWORK,
  DEFAULT_CKB_RPC_URL,
  DEFAULT_WS_URL,
  resolveApiBase,
  resolveBuildVersion,
  resolveCkbNetwork,
  resolveCkbRpcUrl,
  resolveWsUrl,
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
