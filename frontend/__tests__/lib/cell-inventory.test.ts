import { describe, expect, it } from 'vitest';
import type { Cell } from '@/lib/api';
import { CELL_INVENTORY_KINDS, cellInventoryItem } from '@/lib/cell-inventory';
import spore from '@/__tests__/fixtures/cell-inventory/spore.json';
import xudt from '@/__tests__/fixtures/cell-inventory/xudt.json';
import dao from '@/__tests__/fixtures/cell-inventory/dao.json';
import daoWithdraw from '@/__tests__/fixtures/cell-inventory/dao-withdraw.json';
import dotbit from '@/__tests__/fixtures/cell-inventory/dotbit.json';
import bitcell from '@/__tests__/fixtures/cell-inventory/bitcell.json';
import did from '@/__tests__/fixtures/cell-inventory/did.json';
import dotcell from '@/__tests__/fixtures/cell-inventory/dotcell.json';
import mnft from '@/__tests__/fixtures/cell-inventory/mnft.json';

// Real cells from the local mainnet API (2026-11-03): the user's Nervape Mask object, an RGB++
// token cell, a DAO deposit and a withdraw request, one cell of each identity standard and an
// m-NFT token. Only the fields the line reads are kept.
const cell = (fixture: unknown, patch: Partial<Cell> = {}) =>
  ({ ...(fixture as Cell), ...patch }) as Cell;

describe('cellInventoryItem (lane U, U6): what inventory item a cell is, from protocolScript.type', () => {
  it('lists the protocol kinds it names', () => {
    expect([...CELL_INVENTORY_KINDS]).toEqual([
      'spore',
      'spore-cluster',
      'm-nft',
      'm-nft-class',
      'xudt',
      'simple-udt',
      'bit-account',
      'bit-cell',
      'did-ckb',
      'dotcell-account',
      'nervos-dao',
    ]);
  });

  it('a Spore cell is the object its type args name, in the cluster its data names', () => {
    expect(cellInventoryItem(cell(spore))).toEqual({
      kind: 'spore',
      id: '0x7e3bc1bce9aa1f5d085911dee7673fe54ece2d205f6a796a773d9e6f15216539',
      clusterId: '0x95a56c18700cbc3f4e98895fe7063094f592fd89964d49ba5a5c1da1f66b7169',
      href: '/objects/0x7e3bc1bce9aa1f5d085911dee7673fe54ece2d205f6a796a773d9e6f15216539',
    });
  });

  it('an xUDT cell holds the amount its data decodes, of the token its type hash names', () => {
    expect(cellInventoryItem(cell(xudt))).toEqual({
      kind: 'token',
      typeHash: '0xb5fdb4e8ad52f5e3ed4a294c01ca99f7811b06baef2e7fce5b7d4ebbf006717d',
      amount: '1,000',
      unit: 'RGB++',
      href: '/tokens/0xb5fdb4e8ad52f5e3ed4a294c01ca99f7811b06baef2e7fce5b7d4ebbf006717d',
    });
  });

  it('a token of unknown decimals shows its raw amount, marked', () => {
    const raw = cell(xudt, {
      dataAnalysis: {
        ...xudt.dataAnalysis,
        token: { ...xudt.dataAnalysis.token, decimals: null, symbol: null, name: null },
      } as Cell['dataAnalysis'],
    });
    expect(cellInventoryItem(raw)).toMatchObject({
      amount: '100,000,000,000 (raw)',
      unit: 'units of token 0xb5fdb4e8…06717d',
    });
  });

  it('a DAO cell is a deposit or a withdrawal request of its whole capacity', () => {
    expect(cellInventoryItem(cell(dao))).toEqual({
      kind: 'dao',
      stage: 'deposit',
      capacity: BigInt('1250000000000'),
      href: '/dao',
    });
    expect(cellInventoryItem(cell(daoWithdraw))).toMatchObject({
      kind: 'dao',
      stage: 'withdraw-request',
      capacity: BigInt('920000000000'),
    });
  });

  it('identity cells name their identity and its page', () => {
    expect(cellInventoryItem(cell(dotbit))).toEqual({
      kind: 'identity',
      standard: 'dotbit',
      id: '0x000063c0e5d40b236bb9b6b7b5831d87e4429d5c',
      name: 'conaservices.bit',
      href: '/identities/dotbit/0x000063c0e5d40b236bb9b6b7b5831d87e4429d5c',
    });
    expect(cellInventoryItem(cell(dotcell))).toEqual({
      kind: 'identity',
      standard: 'dotcell',
      id: '0x0b340d49874bf09f050bf079b862d61a3e485ce4',
      name: 'verification.cell',
      href: '/identities/dotcell/verification.cell',
    });
    // .bit Cell and did:ckb carry no decoded name: the line reads it as their pages do.
    expect(cellInventoryItem(cell(bitcell))).toEqual({
      kind: 'identity',
      standard: 'bit-cell',
      id: '0x006d5680998ef5d153b1f07f1c13f5f0d910ceaa28f8954906476551f61ff026',
      name: null,
      href: '/identities/bit-cell/0x006d5680998ef5d153b1f07f1c13f5f0d910ceaa28f8954906476551f61ff026',
    });
    expect(cellInventoryItem(cell(did))).toEqual({
      kind: 'identity',
      standard: 'did',
      id: '0x02d945cd1a3ea7fe8448d53d27b5b25d415dfa5fe8b873117133001ec9759a93',
      name: null,
      href: '/identities/did/0x02d945cd1a3ea7fe8448d53d27b5b25d415dfa5fe8b873117133001ec9759a93',
    });
  });

  it('an m-NFT token cell is the object its type args name', () => {
    expect(cellInventoryItem(cell(mnft))).toEqual({
      kind: 'm-nft',
      id: '0x8f67efedd50c61c9dd332defd4051f08a02d79770000001400000000',
      href: '/objects/mnft/0x8f67efedd50c61c9dd332defd4051f08a02d79770000001400000000',
    });
  });

  it('a cluster or an m-NFT class cell is its collection', () => {
    const clusterId = `0x${'95'.repeat(32)}`;
    expect(
      cellInventoryItem(
        cell(spore, {
          protocolScript: { lock: null, type: 'spore-cluster' },
          type: { ...spore.type, args: clusterId },
          dataAnalysis: undefined,
        })
      )
    ).toEqual({ kind: 'spore-cluster', id: clusterId, href: `/clusters/${clusterId}` });
    const classId = '0x8f67efedd50c61c9dd332defd4051f08a02d797700000014';
    expect(
      cellInventoryItem(
        cell(mnft, {
          protocolScript: { lock: null, type: 'm-nft-class' },
          type: { ...mnft.type, args: classId },
        })
      )
    ).toEqual({ kind: 'm-nft-class', id: classId, href: `/classes/${classId}` });
  });

  it('names no item for a plain cell, an unhandled protocol, or the .cell ring root', () => {
    expect(cellInventoryItem(cell(spore, { protocolScript: { lock: null, type: null } }))).toBe(
      null
    );
    for (const type of ['ckbfs', 'stable-asset', 'stable-pool', 'dotcell-price', 'm-nft-issuer'])
      expect(cellInventoryItem(cell(spore, { protocolScript: { lock: null, type } }))).toBe(null);
    const ring = cell(dotcell, {
      dataAnalysis: {
        heuristicGuesses: [],
        deterministic: { kind: 'dotcell_ring_root', summary: '', segments: [] },
      } as unknown as Cell['dataAnalysis'],
    });
    expect(cellInventoryItem(ring)).toBe(null);
  });

  it('fails fast, with the out point, when a handled kind lacks what names its item', () => {
    const at = '0x0361eb40512ede959eecf4a0fc2e14cea834b647f5f91e48eacf08539db6518a:0';
    expect(() => cellInventoryItem(cell(spore, { protocolScript: undefined }))).toThrow(
      `Cell ${at}: the cell detail carries no protocolScript`
    );
    expect(() =>
      cellInventoryItem(cell(spore, { type: { ...spore.type, args: '0x7e3b' } }))
    ).toThrow(`Cell ${at}: spore type args are not a 32-byte id: 0x7e3b`);
    expect(() => cellInventoryItem(cell(spore, { dataAnalysis: undefined }))).toThrow(
      `Cell ${at}: spore cell without its decoded data (got none)`
    );
    expect(() => cellInventoryItem(cell(xudt, { dataAnalysis: undefined }))).toThrow(
      /xudt cell without its decoded token amount/
    );
    expect(() => cellInventoryItem(cell(dao, { dataAnalysis: undefined }))).toThrow(
      /nervos-dao cell is neither a deposit nor a withdraw request \(got none\)/
    );
    expect(() => cellInventoryItem(cell(spore, { type: undefined }))).toThrow(
      /protocolScript\.type is spore but the cell has no type script/
    );
  });
});
