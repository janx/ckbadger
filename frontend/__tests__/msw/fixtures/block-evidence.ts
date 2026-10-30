/** Explicit read-time evidence for the block redesign fixtures. */
export const blockEvidence = {
  maxBlockBytes: 597_000,
  maxBlockCycles: '3500000000',
  minerMessageText: 'ckb 0.210.0',
  size: 1200,
  userTransactionsSize: 226,
  overheadSize: 974,
  totalFees: '12345',
  cycles: '5000000',
  cyclesStatus: 'complete' as const,
  transactionSizes: [635, 226],
  rewardBreakdown: {
    primary: '9000000000',
    secondary: '900000000',
    committed: '60000000',
    proposal: '40000000',
    // F2 blocks (R2b-a 2)
    secondaryIssuance: '9000000000',
  },
  // F2 blocks (R2b-a 1): this cellbase pays block #8,775,627's reward.
  cellbasePays: { number: 8775627, hash: `0x${'77'.repeat(32)}` },
  rewardPaidAtBlock: 8775649,
  rewardFinalizesAt: 8775649,
  confirmations: 12,
  parentTimestamp: '2026-02-21T23:59:50Z',
  rawHeader: {
    epoch: '0x7080000001526',
    dao: `0x${'00'.repeat(32)}`,
    proposalsHash: `0x${'44'.repeat(32)}`,
    extraHash: `0x${'55'.repeat(32)}`,
  },
  uncles: [],
};
