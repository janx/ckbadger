// @vitest-environment node
import { createHash } from 'node:crypto';
import { describe, expect, it } from 'vitest';
import { assertSealId, drawSeal, sealLabel, type SealId, type SealState } from '@/lib/seal/stones';
import { SEAL_INKS, sealHash } from '@/__tests__/utils/seals';

// Lane D2b (the owner's ruling of 2026-11-19, U-03, PA-06): an uncle is sealed with the block stone
// WITHOUT rods, in the dead ink. The chain keeps no transactions for an uncle, so there is no count
// to write; the id says so explicitly (`txs: 'not kept'`), never by a missing count, and the stone
// then draws no rod and nothing in the rods' place. Every other mark is what the hash draws with a
// count, and a block whose count is known draws exactly what it drew before.

const HASHES = Array.from({ length: 24 }, (_, i) => sealHash(`uncle-${i}`));
const COUNTS = [1, 2, 9, 10, 248, 1_820, 9_999];
const SIZES = [
  { form: 'page', height: 104 },
  { form: 'page', height: 80 },
  { form: 'row', height: 16 },
] as const;
const STATES: SealState[] = ['pressed', 'ink', 'unpressed'];

type Txs = number | 'not kept';
const block = (hash: string, txs: Txs) => ({ kind: 'block', hash, txs }) as SealId;
const draw = (
  hash: string,
  txs: Txs,
  form: 'page' | 'row',
  height: number,
  state: SealState
): string => drawSeal(block(hash, txs), { form, height, state, ...SEAL_INKS }).svg;
/** The markup with its accessible name set aside: the name says the count, the marks are compared. */
const marks = (svg: string) => svg.replace(/aria-label="[^"]*"/, 'aria-label=""');

/**
 * What `counted` holds that `rodless` does not, when `rodless` is `counted` with one run of whole
 * elements taken out and nothing else changed: the longest common prefix and suffix make up all of
 * `rodless`, and the run between them begins and ends an element. Fails otherwise.
 */
function takenOut(counted: string, rodless: string): string {
  let pre = 0;
  while (pre < rodless.length && counted[pre] === rodless[pre]) pre++;
  // a run starts at an element's start: back up to the last '<' of the common prefix
  while (pre > 0 && counted[pre] !== '<') pre--;
  let suf = 0;
  while (
    suf < rodless.length - pre &&
    counted[counted.length - 1 - suf] === rodless[rodless.length - 1 - suf]
  )
    suf++;
  expect(rodless).toBe(counted.slice(0, pre) + counted.slice(counted.length - suf));
  const run = counted.slice(pre, counted.length - suf);
  expect(run).toMatch(/^<[\s\S]*>$/);
  return run;
}

describe('the block stone of a block whose transaction count is not kept (lane D2b)', () => {
  it.each(SIZES)(
    'draws no rods and nothing in their place at $form $height px, every other mark as with a count',
    ({ form, height }) => {
      for (const state of STATES)
        for (const hash of HASHES) {
          const rodless = marks(draw(hash, 'not kept', form, height, state));
          for (const txs of COUNTS) {
            const counted = marks(draw(hash, txs, form, height, state));
            // the counted stone is the rodless one with one run of elements more: its rods
            const rods = takenOut(counted, rodless);
            expect(rods.length).toBeGreaterThan(0);
          }
        }
    }
  );

  it('states the count not kept only explicitly: a missing or malformed count is refused, naming the block', () => {
    const hash = HASHES[0];
    for (const txs of [undefined, null, 'none', 'Not kept', '', -1, 1.5, Number.NaN])
      for (const form of ['page', 'row'] as const)
        expect(() => assertSealId({ kind: 'block', hash, txs } as unknown as SealId, form)).toThrow(
          `seal: block ${hash}: the transaction count must be a whole number ≥ 0`
        );
    expect(() => assertSealId(block(hash, 'not kept'), 'page')).not.toThrow();
    expect(() => assertSealId(block(hash, 'not kept'), 'row')).not.toThrow();
    // a count is held to the page's four digits as before
    expect(() => assertSealId(block(hash, 10_000), 'page')).toThrow(
      `seal: block ${hash}: a page seal writes the transaction count in up to 4 digits, got 10000`
    );
  });

  it('says so to a screen reader: the chain keeps no transaction count for an uncle', () => {
    const hash = HASHES[3];
    const label = `Block seal ${hash.slice(0, 10)}, the chain keeps no transaction count for an uncle`;
    expect(sealLabel(block(hash, 'not kept'))).toBe(label);
    for (const { form, height } of SIZES)
      expect(draw(hash, 'not kept', form, height, 'ink')).toContain(`aria-label="${label}"`);
    // a counted block's name is as it was
    expect(sealLabel(block(hash, 1))).toBe(`Block seal ${hash.slice(0, 10)}, 1 transaction`);
    expect(sealLabel(block(hash, 248))).toBe(`Block seal ${hash.slice(0, 10)}, 248 transactions`);
  });

  // The proof that a counted block's stone did not move: the digest of every counted stone above,
  // at each size and state, taken from the drawing before this lane changed lib/seal (stones.ts at
  // fix/readability 00f23415), pinned here; the fidelity test holds the page seals to the prototype
  // besides.
  it('leaves a counted block’s stone as it was, mark for mark', () => {
    const digest = createHash('sha256');
    for (const { form, height } of SIZES)
      for (const state of STATES)
        for (const hash of HASHES)
          for (const txs of COUNTS) digest.update(draw(hash, txs, form, height, state));
    expect(digest.digest('hex')).toBe(COUNTED_DIGEST);
  });
});

const COUNTED_DIGEST = '8ed0e9153d31f1c1544c00b2755fa623b3a383a5510fa943c85c12c5a69746b8';
