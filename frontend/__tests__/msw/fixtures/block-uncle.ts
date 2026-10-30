import type { BlockUncle } from '@/lib/api';

/** `/blocks/8775638/uncles/0`: the uncle at #8,775,637 that block #8,775,638 included. */
export const blockUncle: BlockUncle = {
  number: 8_775_637,
  hash: `0x${'ee'.repeat(32)}`,
  parentHash: `0x${'dd'.repeat(32)}`,
  timestamp: '2026-02-21T23:59:40.123Z',
  epoch: '1/1800',
  epochNumber: 5414,
  epochIndex: 1,
  epochLength: 1800,
  compactTarget: '0x19083c4a',
  difficulty: '1210829853049231829',
  nonce: '0x8e5d3f6c1b2a4d7e9f0a1b2c3d4e5f60',
  version: 0,
  transactionsRoot: `0x${'11'.repeat(32)}`,
  proposalsHash: `0x${'22'.repeat(32)}`,
  extraHash: `0x${'33'.repeat(32)}`,
  dao: {
    raw: '0x28341e52bbf1665b9cb7fbb823dd2a0077f0143eecc3870a00e95a770f4f3407',
    c: '6586217291914818600',
    ar: '12065094518945692',
    s: '758790481656475767',
    u: '519126784900000000',
  },
  proposalIds: ['0xaabbccddeeff00112233'],
  includedBy: { number: 8_775_638, hash: `0x${'aa'.repeat(32)}`, index: 0 },
  canonicalAtHeight: { number: 8_775_637, hash: `0x${'bb'.repeat(32)}` },
  // F2 blocks (R10): the canonical #8,775,637 shares the uncle's parent; the uncle came 92 ms later.
  sameParentAs: 8_775_637,
  msAfterSibling: 92,
};
