// The script groups' rows (lane D3, PA-24): the transaction page and the cell page drew a
// transaction's script groups in two layouts and two wordings ("not executed" against "not run").
// One builder, scriptGroupRows, writes them for both. Its wording is the transaction page's, but
// where that was wrong: a group of the built-in Type ID runs with no cell dep, which the page wrote
// as "No matching dep", as if one were missing; the cell page's "Built-in Type ID" is the right one.
import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { groupHref, groupLabel, scriptGroupRows } from '@/components/transaction/script-group-rows';
import { ScriptGroupMatrix } from '@/components/transaction/script-group-matrix';
import { TYPE_ID_CODE_HASH } from '@/lib/tx-continuity';
import { longDeposit } from '@/__tests__/utils/long-tx';
import { scriptHash } from '@/__tests__/utils/ckb-hash';
import type { ScriptLookupResponse, TransactionDetail, TransactionScriptGroup } from '@/lib/api';
import { DEFAULT_LOCK_CODE_HASH } from '@/lib/lock-script';
import { shortenHash } from '@/lib/shorten';
import { scriptLookupKey } from '@/lib/script-pair';

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

// Lane P1 follow-up (the lock-words gate's known writer 12, script-group-rows): a lock group's name
// is the one decision's. A lock group on the default lock's code hash by data is another lock,
// written by its code hash and "by data", and its link opens that lock, not the default's.
// Re-keyed (lane rd/api-lock-names): the lookup answers each pair; the registry names the default
// lock's pair (its code hash by type) and not the code hash by data.
describe('a lock group’s name is the one decision’s (lane P1 follow-up)', () => {
  const script = {
    codeHash: DEFAULT_LOCK_CODE_HASH,
    hashType: 'data',
    args: `0x${'ab'.repeat(20)}`,
  };
  const group = (hashType: string, kind: 'lock' | 'type' = 'lock') =>
    ({
      scriptHash: scriptHash({ ...script, hashType }),
      kind,
      ran: true,
      inputIndices: [0],
      outputIndices: [],
      witnessIndex: 0,
      script: { ...script, hashType },
      cycles: '0',
    }) as unknown as TransactionScriptGroup;
  const lookup = {
    [scriptLookupKey(DEFAULT_LOCK_CODE_HASH, 'type')]: {
      codeHash: DEFAULT_LOCK_CODE_HASH,
      name: 'SECP256K1/blake160',
    },
  } as unknown as ScriptLookupResponse;

  it('names a lock group on the default lock’s code hash by data by its code hash and "by data"', () => {
    expect(groupLabel(group('data'), lookup)).toBe(
      `${shortenHash(DEFAULT_LOCK_CODE_HASH, 'row')} by data`
    );
    expect(groupHref(group('data'), lookup)).toBe(
      `/script/${DEFAULT_LOCK_CODE_HASH}?hashType=data&kind=lock`
    );
  });

  it('names the default lock’s group and a type group by the registry’s name', () => {
    expect(groupLabel(group('type'), lookup)).toBe('SECP256K1/blake160');
    expect(groupHref(group('type'), lookup)).toBe('/scripts/SECP256K1%2Fblake160');
    // re-pinned (lane rd/api-lock-names): a type group on the default lock's code hash by data is
    // its own pair, which the registry does not name; it is its code hash (it took the default
    // lock's name when the registry named a code hash)
    expect(groupLabel(group('data', 'type'), lookup)).toBe(
      shortenHash(DEFAULT_LOCK_CODE_HASH, 'row')
    );
  });
});
