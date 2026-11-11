import type { CellTileData } from '@/components/ui/cell-tile';
import type { CellSeal } from '@/lib/cell-seal';
import type { SealState } from '@/lib/seal/stones';
import { sealHash } from '@/__tests__/utils/seals';

/**
 * The seal of a hand-made tile (seals by kind, C3): a cell of the tile's class and capacity, at
 * its out point when it has one (else at a transaction named by `seed`, output 0), its lock and
 * type hashes real 32-byte hashes from `seed`. Consistent with the tile, as `CellTile` requires.
 */
export function tileSeal(
  tile: Pick<CellTileData, 'kind' | 'capacity' | 'outPoint'>,
  seed: string,
  state: SealState = 'pressed'
): CellSeal {
  return {
    id: {
      kind: 'cell',
      tx: tile.outPoint ? tile.outPoint.txHash : sealHash(`tile-tx ${seed}`),
      index: tile.outPoint ? tile.outPoint.index : 0,
      lock: sealHash(`tile-lock ${seed}`),
      type: tile.kind === 'ckb' ? null : { class: tile.kind, hash: sealHash(`tile-type ${seed}`) },
      capacity: BigInt(tile.capacity),
    },
    state,
  };
}

/** A hand-made tile with its seal: `tileSeal` of the tile's own fields, seeded by its id. */
export function sealedTile(
  tile: Omit<CellTileData, 'seal'>,
  state: SealState = 'pressed'
): CellTileData {
  return { ...tile, seal: tileSeal(tile, tile.id, state) };
}
