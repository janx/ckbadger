import { describe, expect, it } from 'vitest';
import { protocolActionMeta } from '@/lib/protocol-action-meta';
import type { ActivityProtocolAction } from '@/lib/api';

const pa = (metadata: ActivityProtocolAction['metadata']): ActivityProtocolAction => ({
  protocol: 'dao',
  protocolName: 'Nervos DAO',
  action: 'withdraw_complete',
  metadata,
});

describe('protocolActionMeta', () => {
  it('reads every integer from its exact decimal string, past 2^53 too', () => {
    const meta = protocolActionMeta(
      pa({
        capacity: '9007199254740993',
        compensation: '12345678901234567',
        depositBlock: '19695550',
      })
    );
    expect(meta.capacity).toBe(BigInt('9007199254740993'));
    expect(meta.compensation).toBe(BigInt('12345678901234567'));
    expect(meta.depositBlock).toBe(BigInt(19695550));
  });

  it('reads the string facts as they are and absent keys as null', () => {
    const meta = protocolActionMeta({
      protocol: 'fiber',
      protocolName: 'Fiber',
      action: 'channel_close',
      metadata: { channelOutpoint: '0xabc:0' },
    });
    expect(meta).toEqual({
      capacity: null,
      compensation: null,
      depositBlock: null,
      channelOutpoint: '0xabc:0',
      btcTxid: null,
      btcOutputIndex: null,
    });
  });

  it('fails fast on a JSON number: free-form JSON carries integers as exact strings', () => {
    expect(() =>
      protocolActionMeta(
        pa({ capacity: 2605000000000 } as unknown as ActivityProtocolAction['metadata'])
      )
    ).toThrow(
      'dao:withdraw_complete metadata.capacity is the JSON number 2605000000000, not an exact integer string'
    );
  });

  it('fails fast on a string that is not an integer', () => {
    expect(() => protocolActionMeta(pa({ capacity: '26.5' }))).toThrow(
      'dao:withdraw_complete metadata.capacity "26.5" is not an integer'
    );
  });
});

describe('protocolActionMeta — the facts each DAO step must carry', () => {
  it('requires the capacity of a deposit', () => {
    expect(() =>
      protocolActionMeta({
        protocol: 'dao',
        protocolName: 'Nervos DAO',
        action: 'deposit',
        metadata: {},
      })
    ).toThrow('dao:deposit metadata.capacity is missing');
  });

  it('requires the capacity and deposit block of a withdraw request', () => {
    expect(() =>
      protocolActionMeta({
        protocol: 'dao',
        protocolName: 'Nervos DAO',
        action: 'withdraw_request',
        metadata: { capacity: '1' },
      })
    ).toThrow('dao:withdraw_request metadata.depositBlock is missing');
  });

  it('requires the capacity and compensation of a completed withdraw', () => {
    expect(() =>
      protocolActionMeta({
        protocol: 'dao',
        protocolName: 'Nervos DAO',
        action: 'withdraw_complete',
        metadata: { capacity: '1' },
      })
    ).toThrow('dao:withdraw_complete metadata.compensation is missing');
  });

  it('asks nothing of a protocol whose metadata is optional', () => {
    expect(
      protocolActionMeta({
        protocol: 'utxoswap',
        protocolName: 'UTXOSwap',
        action: 'swap',
        metadata: {},
      }).capacity
    ).toBeNull();
  });
});

it('reads an exact Bitcoin output index without numeric coercion', () => {
  expect(
    protocolActionMeta({
      protocol: 'rgbpp',
      protocolName: 'RGB++',
      action: 'transfer',
      metadata: { btcTxid: '0xabc', outIndex: '4294967295' },
    }).btcOutputIndex
  ).toBe(BigInt('4294967295'));
});
