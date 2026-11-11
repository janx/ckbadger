// Round 3 lane F (contracts R-B · R-9, SC-K5): an output that is a code cell says whose code it is.
import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { cellTiles } from '@/components/transaction/transaction-cell-tiles';
import type { TransactionDetail } from '@/lib/api';

const TYPE_ID = '0x00000000000000000000000000000000000000000000000000545950455f4944';
/** The real script hashes (blake2b, CKB) of the lock and of the Type ID type below. */
const LOCK_HASH = '0x82cbea9785c59f24117b45294cff296e8ab0ac2f29bbfa78af6b01e4d5325644';
const TYPE_ID_HASH = '0x8f20ed3602ef8465071bf7b91d6444fe04f3035daa0f93599c7f665eebb11761';
const output = (over: Record<string, unknown> = {}) => ({
  capacity: '10043800000000',
  commonKnowledgeSize: '100438',
  status: 'live' as const,
  display: null,
  address: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq4s93hhsj',
  lock: { codeHash: `0x${'9b'.repeat(32)}`, hashType: 'type', args: '0x' },
  type: { codeHash: TYPE_ID, hashType: 'type', args: `0x${'aa'.repeat(32)}` },
  isChange: false,
  ...over,
});
const tx = (outputs: ReturnType<typeof output>[]) =>
  ({
    hash: `0x${'ee'.repeat(32)}`,
    status: 'committed',
    isCellbase: false,
    inputs: [],
    outputs,
    // as the API lists them: the output's Type ID ran; its lock did not
    scriptGroups: [
      {
        scriptHash: TYPE_ID_HASH,
        kind: 'type',
        ran: true,
        inputIndices: [],
        outputIndices: outputs.map((_, i) => i),
        witnessIndex: 0,
        script: outputs[0].type,
        cycles: null,
      },
      {
        scriptHash: LOCK_HASH,
        kind: 'lock',
        ran: false,
        inputIndices: [],
        outputIndices: outputs.map((_, i) => i),
        witnessIndex: null,
        script: outputs[0].lock,
        cycles: null,
      },
    ],
  }) as unknown as TransactionDetail;

function tileContent(o: ReturnType<typeof output>) {
  const { outputs } = cellTiles(tx([o]), undefined);
  render(<div data-testid="tile">{outputs[0].content}</div>);
  return screen.getByTestId('tile');
}

describe('cellTiles code cells (R-9)', () => {
  it('says "code <script> v<version>" instead of "Type script"', () => {
    const tile = tileContent(
      output({ codeCell: { scriptName: 'JoyID', versionHash: `0x${'11'.repeat(32)}`, version: 5 } })
    );
    expect(tile).toHaveTextContent(/^code JoyID v5$/);
    expect(tile).not.toHaveTextContent('Type script');
  });

  it('names a binary with no indexed code cell yet (pending) as a new version, without a number', () => {
    const tile = tileContent(
      output({
        codeCell: { scriptName: 'JoyID', versionHash: `0x${'11'.repeat(32)}`, version: null },
      })
    );
    expect(tile).toHaveTextContent(/^code JoyID, new version, not indexed yet$/);
  });

  it('keeps the type script wording for an output that is no registry code', () => {
    expect(tileContent(output())).toHaveTextContent(/^Type script$/);
  });
});
