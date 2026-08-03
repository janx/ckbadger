import {
  apiBaseFor,
  isKnownNetwork,
  networkFromPath,
  resolveActiveNetwork,
  wsUrlFor,
} from '@/lib/active-network';

describe('active-network', () => {
  beforeEach(() => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
  });

  afterEach(() => {
    // jsdom's window persists across tests in a file; clear seeded config so it
    // does not leak into other test files sharing the same jsdom instance.
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  it('networkFromPath returns the first segment when it is a known network', () => {
    expect(networkFromPath('/testnet/tx/0x1')).toBe('testnet');
  });

  it('networkFromPath returns null when the first segment is not a network', () => {
    expect(networkFromPath('/tx/0x1')).toBeNull();
  });

  it('networkFromPath returns null for the root path', () => {
    expect(networkFromPath('/')).toBeNull();
  });

  it('isKnownNetwork reflects the configured networks', () => {
    expect(isKnownNetwork('testnet')).toBe(true);
    expect(isKnownNetwork('devnet')).toBe(false);
  });

  it('resolveActiveNetwork derives the network from the path', () => {
    expect(resolveActiveNetwork('/testnet/blocks')).toBe('testnet');
  });

  it('resolveActiveNetwork falls back to the default network', () => {
    expect(resolveActiveNetwork('/blocks')).toBe('mainnet');
  });

  it('apiBaseFor substitutes the network into the (relative) pattern', () => {
    expect(apiBaseFor('testnet')).toBe('/api/testnet/v1');
  });

  it('wsUrlFor builds an absolute ws url ending in the pattern', () => {
    const url = wsUrlFor('testnet');
    expect(url.startsWith('ws')).toBe(true);
    expect(url.endsWith('/ws/testnet')).toBe(true);
  });
});
