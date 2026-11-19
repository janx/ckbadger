/**
 * The long transaction fixture says its figures agree with themselves (`__tests__/utils/long-tx.ts`):
 * this holds it to that (review of phases C and D, tests NOTE 8; lane P1). The fee payer's outflow
 * is what its lock lost, the API's "> 0" (`lib/api.ts`, `feePaidBy`), and every decoded witness
 * field's bytes are the witness's own bytes over its span.
 */
import { describe, expect, it } from 'vitest';
import { longDeposit, LONG_TX_SENDER } from '@/__tests__/utils/long-tx';

describe('the long transaction fixture', () => {
  const tx = longDeposit();

  it('pays its fee from the sender, whose outflow is what its lock lost', () => {
    const consumed = tx.inputs!.reduce((sum, c) => sum + BigInt(c.capacity!), BigInt(0));
    const back = tx.outputs!.filter((c) => c.address === LONG_TX_SENDER);
    const kept = back.reduce((sum, c) => sum + BigInt(c.capacity), BigInt(0));
    expect(tx.inputs!.every((c) => c.address === LONG_TX_SENDER)).toBe(true);
    expect(tx.feePaidBy).toHaveLength(1);
    expect(tx.feePaidBy[0].address).toBe(LONG_TX_SENDER);
    expect(BigInt(tx.feePaidBy[0].outflow)).toBe(consumed - kept);
    expect(BigInt(tx.feePaidBy[0].outflow) > BigInt(0)).toBe(true);
  });

  it('decodes each witness field as the bytes of its own span', () => {
    for (const w of tx.witnessDetails!) {
      const bytes = tx.witnesses[w.index].slice(2);
      expect(bytes.length / 2, `witness ${w.index}`).toBe(w.size);
      for (const f of w.fields)
        expect(f.hex, `witness ${w.index} ${f.name}`).toBe(
          `0x${bytes.slice(2 * f.start, 2 * f.end)}`
        );
    }
  });
});
