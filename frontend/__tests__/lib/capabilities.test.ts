import { buildAiCapabilities } from '@/lib/ai/capabilities';

describe('buildAiCapabilities', () => {
  it('returns expected format negotiation contract', () => {
    const capabilities = buildAiCapabilities('http://localhost:3000');

    expect(capabilities.origin).toBe('http://localhost:3000');
    expect(capabilities.site.name).toBe('ckbadger');
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
  });

  it('declares tx debugger profile in raw route matrix', () => {
    const capabilities = buildAiCapabilities();

    expect(capabilities.routes.markdown).toContain('/nfts/dotbit/{nftId}');
    expect(capabilities.routes.markdown).toContain('/nfts/did/{nftId}');
    expect(capabilities.routes.raw).toContain('/tx/{hash}');
    expect(capabilities.routes.raw).toContain('/nfts/did/{nftId}');
    expect(capabilities.rawProfiles.routes['/nfts/did/{nftId}']).toEqual(['default']);
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
