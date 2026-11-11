import { describe, expect, it } from 'vitest';
import { addressProducerCells } from '@/__tests__/msw/fixtures/address-producer';
import { kindOf } from '@/components/transaction/transaction-cell-tiles';
import { addressCellSealId } from '@/lib/address-cell-seal';
import type { AddressCell } from '@/lib/api';
import { assertSealId } from '@/lib/seal/stones';

// The seals by kind (C3 for an address's cells): an AddressCell's seal id is a plain mapping of
// the API's fields; its class is the transaction page's (`kindOf()` over `typeCellType`), never
// the address page's own `class` grouping. Fixtures: the real API capture (address-producer).

const TX = '0xdce40476d0a0fcaf57ba8fdd7091552416ab27dfa5aba73d90a9eb48543993e1';
const LOCK = '0x94bfe0b549decec8d74c54769b74bf53100d936d70b6557631ba2deea49a8942';
/** 1,000 CKB in shannons, every producer cell's capacity. */
const CAPACITY = BigInt('100000000000');
const cells = addressProducerCells.data as readonly AddressCell[];
const at = (index: number): AddressCell => {
  const cell = cells.find((c) => c.outputIndex === index);
  if (!cell) throw new Error(`no producer cell ${index}`);
  return cell;
};
const OUT = (index: number) => `address cell ${TX}:${index}`;

describe('addressCellSealId', () => {
  it('a plain CKB cell: a bare stone, no type', () => {
    expect(addressCellSealId(at(0))).toEqual({
      kind: 'cell',
      tx: TX,
      index: 0,
      lock: LOCK,
      type: null,
      capacity: CAPACITY,
    });
  });

  it('a Nervos DAO cell (no display: the class is read from typeCellType)', () => {
    expect(at(2).display).toBeNull();
    expect(addressCellSealId(at(2))).toEqual({
      kind: 'cell',
      tx: TX,
      index: 2,
      lock: LOCK,
      type: {
        class: 'dao',
        hash: '0xcc77c4deac05d68ab5b26828f0bf4565a8d73113d7bb7e92b8362b8a74e58e58',
      },
      capacity: CAPACITY,
    });
  });

  it('a token cell', () => {
    expect(addressCellSealId(at(4))).toEqual({
      kind: 'cell',
      tx: TX,
      index: 4,
      lock: LOCK,
      type: {
        class: 'tokens',
        hash: '0x3b02b7c0d2d8abe06500d473edc9956a11ef5601c58361d4c3048df3f15902ee',
      },
      capacity: CAPACITY,
    });
  });

  it('an object cell: a Spore and an mNFT', () => {
    expect(addressCellSealId(at(6)).type).toEqual({
      class: 'objects',
      hash: '0x88597e678182ee74b8aa453d3ee263a6bbdd65f32900425ba842c0ea29b6817f',
    });
    expect(addressCellSealId(at(7)).type).toEqual({
      class: 'objects',
      hash: '0xf7c18075c4d3188c3bb1974275aef501545992639ee615f48fcbc5e95dd4331b',
    });
  });

  it('an identity cell: did:ckb, a .bit Cell, a .bit account', () => {
    expect(addressCellSealId(at(8))).toEqual({
      kind: 'cell',
      tx: TX,
      index: 8,
      lock: LOCK,
      type: {
        class: 'identities',
        hash: '0x7df5b9cf320a34aaa9d26f7d966a138253234a1a66a9440ef5ec7a9da8b7abdc',
      },
      capacity: CAPACITY,
    });
    expect(addressCellSealId(at(9)).type?.class).toBe('identities');
    expect(addressCellSealId(at(10)).type?.class).toBe('identities');
  });

  it('a cell of an unregistered type script (`typed`): class other', () => {
    expect(at(1).typeCellType).toBe('typed');
    expect(addressCellSealId(at(1))).toEqual({
      kind: 'cell',
      tx: TX,
      index: 1,
      lock: LOCK,
      type: {
        class: 'other',
        hash: '0x9898bd8afd8e711056c0fb1bfddb2bc2b53de26f766ec407f5ace9c1bdd55505',
      },
      capacity: CAPACITY,
    });
  });

  it('classes every cell as the transaction page does, and every id is drawable', () => {
    for (const cell of cells) {
      const id = addressCellSealId(cell);
      expect(id.type?.class ?? 'ckb').toBe(
        kindOf({
          display: cell.display,
          cellType: cell.typeCellType,
          type: cell.typeCodeHash ? { codeHash: cell.typeCodeHash } : null,
        })
      );
      expect(() => assertSealId(id, 'row')).not.toThrow();
    }
  });

  it('never reads the address page’s own class grouping', () => {
    // the same cell, grouped otherwise by the address index: the seal does not move
    expect(addressCellSealId({ ...at(1), class: 'token' })).toEqual(addressCellSealId(at(1)));
    expect(addressCellSealId({ ...at(4), class: 'plain' })).toEqual(addressCellSealId(at(4)));
    // with no display, a did:ckb cell is still an identity, by its cell type
    expect(addressCellSealId({ ...at(8), display: null }).type?.class).toBe('identities');
  });

  it('keeps the capacity exact in shannons, past 2^53', () => {
    expect(addressCellSealId({ ...at(0), capacity: '9007199254740993' }).capacity).toBe(
      BigInt('9007199254740993')
    );
  });

  describe('refuses, naming the out point', () => {
    it('a response with no typeCellType (an API older than the page)', () => {
      const old = { ...at(2) } as Partial<AddressCell>;
      delete old.typeCellType;
      expect(() => addressCellSealId(old as AddressCell)).toThrow(
        `${OUT(2)}: the API served no typeCellType`
      );
    });

    it('class ckb with a type script hash', () => {
      const hash = '0x' + 'ab'.repeat(32);
      expect(() => addressCellSealId({ ...at(0), typeScriptHash: hash })).toThrow(
        `${OUT(0)}: class ckb (cell type "plain") with type script hash ${hash}`
      );
    });

    it('a class with no type script hash: by cell type, by display, an unregistered script', () => {
      expect(() => addressCellSealId({ ...at(2), typeScriptHash: null })).toThrow(
        `${OUT(2)}: class dao (cell type "nervos-dao") with no type script hash`
      );
      expect(() =>
        addressCellSealId({
          ...at(4),
          typeScriptHash: null,
          typeCodeHash: null,
          typeHashType: null,
        })
      ).toThrow(`${OUT(4)}: class tokens (cell type "sudt") with no type script hash`);
      expect(() => addressCellSealId({ ...at(1), typeScriptHash: null })).toThrow(
        `${OUT(1)}: class other (cell type "typed") with no type script hash`
      );
    });

    it('a capacity that is not a whole number of shannons', () => {
      for (const capacity of ['1e11', '-1', '1000.5', ''])
        expect(() => addressCellSealId({ ...at(0), capacity })).toThrow(
          `${OUT(0)}: capacity: expected a non-negative decimal integer`
        );
    });
  });
});
