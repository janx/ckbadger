// The script groups' rows (lane D3, PA-24): the transaction page and the cell page drew a
// transaction's script groups in two layouts and two wordings ("not executed" against "not run").
// One builder, scriptGroupRows, writes them for both. Its wording is the transaction page's, but
// where that was wrong: a group of the built-in Type ID runs with no cell dep, which the page wrote
// as "No matching dep", as if one were missing; the cell page's "Built-in Type ID" is the right one.
import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { scriptGroupRows } from '@/components/transaction/script-group-rows';
import { ScriptGroupMatrix } from '@/components/transaction/script-group-matrix';
import { TYPE_ID_CODE_HASH } from '@/lib/tx-continuity';
import { longDeposit } from '@/__tests__/utils/long-tx';
import { scriptHash } from '@/__tests__/utils/ckb-hash';
import type { TransactionDetail } from '@/lib/api';

/** The long deposit with a Type ID on its change output, a group that ran with no cell dep. */
function withTypeId(): TransactionDetail {
  const tx = longDeposit();
  const typeId = { codeHash: TYPE_ID_CODE_HASH, hashType: 'type', args: `0x${'77'.repeat(32)}` };
  return {
    ...tx,
    outputs: tx.outputs!.map((o, i) => (i === 1 ? { ...o, type: typeId } : o)),
    scriptGroups: [
      ...tx.scriptGroups!,
      {
        scriptHash: scriptHash(typeId),
        kind: 'type',
        ran: true,
        inputIndices: [],
        outputIndices: [1],
        witnessIndex: null,
        script: typeId,
        cycles: '0',
      },
    ],
  } as unknown as TransactionDetail;
}

describe('scriptGroupRows: one wording for a transaction’s script groups (lane D3)', () => {
  it('says a built-in Type ID group runs with no cell dep, not that its dep is missing', () => {
    const tx = withTypeId();
    const { groups } = scriptGroupRows(tx, {
      lookup: undefined,
      deps: [],
      depsError: null,
      provisional: false,
      depHref: (i) => `#tx-dep-${i}`,
    });
    const typeId = groups.find((g) => g.id.startsWith('type:run:') && g.cellIds.includes('out:1'))!;
    render(<p data-testid="code">{typeId.code('row')}</p>);
    expect(screen.getByTestId('code')).toHaveTextContent(/^Built-in Type ID, no cell dep$/);
    // a group that ran and names a code hash with no dep still says no dep matches
    const dao = groups.find((g) => g.id.startsWith('type:run:') && g.cellIds.includes('out:0'))!;
    render(<p data-testid="dao">{dao.code('row')}</p>);
    expect(screen.getByTestId('dao')).toHaveTextContent(/^No matching dep$/);
  });

  it('draws rows with nothing to select where the page has no witness inspector', () => {
    const tx = longDeposit();
    const { inputs, outputs, groups } = scriptGroupRows(tx, {
      lookup: undefined,
      deps: [],
      depsError: null,
      provisional: false,
      depHref: (i) => `/tx/${tx.hash}#tx-dep-${i}`,
    });
    for (const g of groups) {
      expect(g.onFocus).toBeUndefined();
      expect(g.witness?.onSelect).toBeUndefined();
    }
    const { container } = render(
      <ScriptGroupMatrix txHash={tx.hash} inputs={inputs} outputs={outputs} groups={groups} />
    );
    // no control that does nothing: the rows are hover targets, the names links, nothing a button
    expect(container.querySelectorAll('button')).toHaveLength(0);
    expect(container.querySelector('[data-group-row]')).not.toBeNull();
  });
});
