import { buildAiCapabilities } from '@/lib/ai/capabilities';

describe('buildAiCapabilities', () => {
  beforeEach(() => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
  });

  afterEach(() => {
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  it('returns expected format negotiation contract', () => {
    const capabilities = buildAiCapabilities('http://localhost:3000');

    expect(capabilities.origin).toBe('http://localhost:3000');
    expect(capabilities.site.name).toBe('ckbadger');
    expect(capabilities.site.pageBasePattern).toBe('/{network}');
    expect(capabilities.site.apiBasePattern).toBe('/api/{network}/v1');
    expect(capabilities.site.wsUrlPattern).toBe('/ws/{network}');
    // Every API/WS path is per-network: the un-prefixed `/api/v1` and `/ws` this
    // document used to advertise hit the SPA fallback, not the API.
    expect(capabilities.site).not.toHaveProperty('directApiBase');
    expect(capabilities.site).not.toHaveProperty('directWsUrl');
    expect(capabilities.site.networks).toEqual(['mainnet', 'testnet']);
    expect(capabilities.site.defaultNetwork).toBe('mainnet');
    expect(capabilities.formatNegotiation.supportedFormats).toEqual(['html', 'md', 'raw']);
    expect(capabilities.formatNegotiation.priority).toEqual([
      'query.format',
      'path.suffix',
      'accept.header',
    ]);
    expect(capabilities.formatNegotiation.raw.defaultProfile).toBe('default');
    expect(capabilities.responseHeaders.raw.formatHeader).toBe('x-ckbadger-format');
    expect(capabilities.responseHeaders.raw.profileHeader).toBe('x-ckbadger-profile');
    expect(capabilities.responseHeaders.raw.schemaHeader).toBe('x-ckbadger-schema');
    expect(capabilities.responseMetadata.markdown.frontmatterFields).toContain('buildVersion');
    expect(capabilities.responseMetadata.raw.metaFields).toContain('buildVersion');
  });

  it('derives the per-network patterns and network list from the runtime config', () => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'devnet' }],
      defaultNetwork: 'devnet',
      apiBasePattern: '/proxy/api/{network}/v1',
      wsUrlPattern: '/proxy/ws/{network}',
    };

    const capabilities = buildAiCapabilities();

    expect(capabilities.site.apiBasePattern).toBe('/proxy/api/{network}/v1');
    expect(capabilities.site.wsUrlPattern).toBe('/proxy/ws/{network}');
    expect(capabilities.site.networks).toEqual(['devnet']);
    expect(capabilities.site.defaultNetwork).toBe('devnet');
  });

  it('declares tx debugger profile in raw route matrix', () => {
    const capabilities = buildAiCapabilities();

    expect(capabilities.site).not.toHaveProperty('apiBase');
    expect(capabilities.routes.markdown).toContain('/activities');
    expect(capabilities.routes.markdown).toContain('/network');
    expect(capabilities.routes.markdown).toContain('/identities/dotbit/{identityId}');
    expect(capabilities.routes.markdown).toContain('/identities/did/{identityId}');
    expect(capabilities.routes.markdown).toContain('/objects/mnft/{objectId}');
    expect(capabilities.routes.raw).toContain('/tx/{hash}');
    expect(capabilities.routes.raw).toContain('/identities/dotbit/{identityId}');
    expect(capabilities.routes.raw).toContain('/identities/did/{identityId}');
    expect(capabilities.routes.raw).toContain('/objects/mnft/{objectId}');
    expect(capabilities.rawProfiles.routes['/identities/dotbit/{identityId}']).toEqual(['default']);
    expect(capabilities.rawProfiles.routes['/identities/did/{identityId}']).toEqual(['default']);
    expect(capabilities.rawProfiles.routes['/objects/mnft/{objectId}']).toEqual(['default']);
    expect(capabilities.rawProfiles.routes['/tx/{hash}']).toEqual(['default', 'debugger']);
    expect(capabilities.rawProfiles.txDebuggerProfile.payloadPath).toBe(
      'data.txDebugger.mockTransaction'
    );
    expect(capabilities.rawProfiles.txDebuggerProfile.profile).toBe('debugger');
    expect(capabilities.rawProfiles.txWitnessPayload.payloadPath).toBe('data.txWitness');
    expect(capabilities.rawProfiles.txWitnessPayload.fields).toEqual([
      'available',
      'witnessesCount',
      'inputCount',
      'analyses',
      'inference',
    ]);
  });
});
