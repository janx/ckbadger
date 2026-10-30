import type { TransactionDetail } from '@/lib/api';
import type { TxContinuity } from '@/components/transaction/tx-structure';
import type { CellTileData } from '@/components/ui/cell-tile';

/** The built-in Type ID script: `"TYPE_ID"` right-aligned, referenced by type. */
export const TYPE_ID_CODE_HASH =
  '0x00000000000000000000000000000000000000000000000000545950455f4944';

/**
 * The dashed "same … continues" links of a transaction structure (txviz): a type script group
 * with exactly one input and one output carries one thing across — a Type ID (the same code cell
 * id, replaced), a file cell, an object or an identity. Read from the API's script groups and
 * cell displays only.
 *
 * group scripts (F2-5): the transaction page builds the same links inline; it should call this
 * so the two pages keep one path.
 */
export function txContinuity(
  tx: TransactionDetail,
  inputs: readonly CellTileData[]
): TxContinuity[] {
  return (tx.scriptGroups ?? []).flatMap((group): TxContinuity[] => {
    if (
      group.kind !== 'type' ||
      group.inputIndices.length !== 1 ||
      group.outputIndices.length !== 1
    )
      return [];
    const inputIndex = group.inputIndices[0];
    const outputIndex = group.outputIndices[0];
    const from = `in:${inputIndex}`;
    const to = `out:${outputIndex}`;
    if (
      group.script.hashType === 'type' &&
      group.script.codeHash.toLowerCase() === TYPE_ID_CODE_HASH
    )
      return [{ from, to, label: 'same type id continues', kind: 'type-id' as const }];
    const tile = inputs[inputIndex];
    if (!tile) throw new Error(`${tx.hash}: script group input ${inputIndex} has no cell tile`);
    const inputDisplay = tx.inputs?.[inputIndex]?.display;
    const outputDisplay = tx.outputs?.[outputIndex]?.display;
    if (
      inputDisplay &&
      outputDisplay &&
      (inputDisplay.kind !== outputDisplay.kind ||
        inputDisplay.standard !== outputDisplay.standard ||
        inputDisplay.id !== outputDisplay.id)
    )
      return [];
    const file = inputDisplay?.kind === 'file' && outputDisplay?.kind === 'file';
    if (!file && tile.kind !== 'objects' && tile.kind !== 'identities') return [];
    return [
      {
        from,
        to,
        label: `same ${file ? 'file cell' : tile.kind === 'objects' ? 'object' : 'identity'} continues`,
        kind: file ? ('file' as const) : tile.kind,
      },
    ];
  });
}
