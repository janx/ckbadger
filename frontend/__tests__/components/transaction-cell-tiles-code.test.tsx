// Round 3 lane F (contracts R-B · R-9, SC-K5): an output that is a code cell says whose code it is.
import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { cellTiles } from '@/components/transaction/transaction-cell-tiles';
import type { TransactionDetail } from '@/lib/api';

const TYPE_ID = '0x00000000000000000000000000000000000000000000000000545950455f4944';
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
    isCellbase: false,
    inputs: [],
    outputs,
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
