import { SCRIPT_ACTIVITY_FAMILIES, scriptActivityLink } from '@/lib/script-activity-link';

// Lane S (S1, ruling 4): a script the registry maps to an activity class links to that class's
// feed on /activities; the label says what the filter opens. Generic locks and every unmapped
// family get no link — never a guessed one.
describe('scriptActivityLink', () => {
  it.each([
    ['nervos-dao', 'dao', 'Activity: Nervos DAO →'],
    ['spore', 'object', 'Activity: Objects →'],
    ['spore-cluster', 'object', 'Activity: Objects →'],
    ['m-nft', 'object', 'Activity: Objects →'],
    ['m-nft-class', 'object', 'Activity: Objects →'],
    ['xudt', 'token', 'Activity: Tokens →'],
    ['simple-udt', 'token', 'Activity: Tokens →'],
    ['bit-account', 'identity', 'Activity: Identities →'],
    ['bit-cell', 'identity', 'Activity: Identities →'],
    ['did-ckb', 'identity', 'Activity: Identities →'],
    ['dotcell-account', 'identity', 'Activity: Identities →'],
    // The API's `protocol` bucket holds RGB++, Fiber and UTXOSwap together: the label says so.
    ['rgb', 'protocol', 'Protocol activity →'],
    ['fiber-funding-lock', 'protocol', 'Protocol activity →'],
    ['fiber-commitment-lock', 'protocol', 'Protocol activity →'],
    ['utxoswap-intent-lock', 'protocol', 'Protocol activity →'],
    ['utxoswap-proxy-lock', 'protocol', 'Protocol activity →'],
  ])('maps %s to the %s feed', (familyId, filter, label) => {
    expect(scriptActivityLink(familyId)).toEqual({
      filter,
      label,
      href: `/activities?filter=${filter}`,
    });
  });

  it.each([
    'default-lock',
    'joyid',
    'omni-lock',
    'default-multisig',
    'anyone-can-pay-lock',
    'ckbfs',
    'cota',
    'm-nft-issuer',
    'dotcell-price',
    'stable-asset',
    'no-such-family',
  ])('gives %s no link', (familyId) => {
    expect(scriptActivityLink(familyId)).toBeNull();
  });

  it('lists every mapped family once, each to a filter the activities page offers', () => {
    const ids = SCRIPT_ACTIVITY_FAMILIES.map(([id]) => id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const [, filter] of SCRIPT_ACTIVITY_FAMILIES)
      expect(['dao', 'object', 'token', 'identity', 'protocol']).toContain(filter);
  });
});
