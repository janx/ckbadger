import { ckbAddress } from '@/__tests__/utils/ckb-address';
import { scriptHash } from '@/__tests__/utils/ckb-hash';
import type { TransactionDetail } from '@/lib/api';

/**
 * The long transaction of the readability review (lane D3): a Nervos DAO deposit that consumes
 * 158 cells of one lock and creates two, the deposit and the change, the shape of mainnet's
 * 0x1422d49b…859e4c02 (30,316 px of page at 1280 before the fold). Its figures agree with
 * themselves: the inputs' capacities sum to the outputs' and the fee, every cell's lock and type
 * are the scripts of the groups that list them, each group's hash is its script's, and the
 * witnesses are the first lock's WitnessArgs (85 bytes) and 157 empty ones.
 */
export const LONG_TX_HASH = `0x1422d49b${'04b8'.repeat(13)}4c02`;
/** The transaction whose outputs it spends: input i spends its output i. */
export const LONG_TX_PREV = `0x66c3a1b2${'00'.repeat(25)}0bb516`;
export const LONG_TX_INPUTS = 158;

const DEFAULT_LOCK = '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8';
const DAO_CODE_HASH = '0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e';
const lock = (args: string) => ({ codeHash: DEFAULT_LOCK, hashType: 'type', args });
/** The sender's lock: every input's, and the change's. */
export const LONG_TX_SENDER_LOCK = lock(`0x${'33'.repeat(20)}`);
export const LONG_TX_SENDER = ckbAddress({ codeHash: DEFAULT_LOCK, args: `0x${'33'.repeat(20)}` });
const DEPOSITOR_LOCK = lock(`0x${'44'.repeat(20)}`);
const DEPOSITOR = ckbAddress({ codeHash: DEFAULT_LOCK, args: `0x${'44'.repeat(20)}` });
const DAO = { codeHash: DAO_CODE_HASH, hashType: 'type', args: '0x' };

/** One input's capacity: 77,215.57595932 CKB, 61 bytes occupied (158 of them, 12,200,061 CKB). */
const INPUT_CAPACITY = BigInt(7721557595932);
const FEE = BigInt(1572400);
const DEPOSIT = BigInt(12200000000000);

export function longDeposit(): TransactionDetail {
  const consumed = INPUT_CAPACITY * BigInt(LONG_TX_INPUTS);
  const change = consumed - FEE - DEPOSIT;
  const sender = scriptHash(LONG_TX_SENDER_LOCK);
  const witnessArgs = `0x5500000010000000550000005500000041000000${'ab'.repeat(65)}`;
  if (witnessArgs.length !== 2 + 2 * 85) throw new Error('the WitnessArgs fixture is not 85 bytes');
  return {
    hash: LONG_TX_HASH,
    status: 'committed',
    blockNumber: 21008952,
    blockHash: `0x${'aa'.repeat(32)}`,
    index: 3,
    inputsCount: LONG_TX_INPUTS,
    outputsCount: 2,
    fee: FEE.toString(),
    feeRate: '1000',
    txSize: 15724,
    isCellbase: false,
    cycles: 5000000,
    cyclesStatus: 'complete',
    timestamp: '2026-11-14T04:54:57Z',
    confirmations: 36,
    inputsCapacity: consumed.toString(),
    outputsCapacity: (consumed - FEE).toString(),
    inputsCommonKnowledgeSize: String(61 * LONG_TX_INPUTS),
    outputsCommonKnowledgeSize: String(102 + 61),
    inputs: Array.from({ length: LONG_TX_INPUTS }, (_, i) => ({
      previousOutput: { txHash: LONG_TX_PREV, index: i },
      capacity: INPUT_CAPACITY.toString(),
      commonKnowledgeSize: '61',
      display: null,
      lock: LONG_TX_SENDER_LOCK,
      address: LONG_TX_SENDER,
    })),
    outputs: [
      {
        capacity: DEPOSIT.toString(),
        commonKnowledgeSize: '102',
        status: 'live',
        isChange: false,
        display: null,
        cellType: 'nervos-dao',
        lock: DEPOSITOR_LOCK,
        type: DAO,
        address: DEPOSITOR,
      },
      {
        capacity: change.toString(),
        commonKnowledgeSize: '61',
        status: 'live',
        isChange: true,
        display: null,
        lock: LONG_TX_SENDER_LOCK,
        address: LONG_TX_SENDER,
      },
    ],
    scriptGroups: [
      {
        scriptHash: sender,
        kind: 'lock',
        ran: true,
        inputIndices: Array.from({ length: LONG_TX_INPUTS }, (_, i) => i),
        outputIndices: [],
        witnessIndex: 0,
        script: LONG_TX_SENDER_LOCK,
        cycles: '1694860',
      },
      {
        scriptHash: scriptHash(DAO),
        kind: 'type',
        ran: true,
        inputIndices: [],
        outputIndices: [0],
        witnessIndex: null,
        script: DAO,
        cycles: '20000',
      },
      {
        scriptHash: scriptHash(DEPOSITOR_LOCK),
        kind: 'lock',
        ran: false,
        inputIndices: [],
        outputIndices: [0],
        witnessIndex: null,
        script: DEPOSITOR_LOCK,
        cycles: null,
      },
      {
        scriptHash: sender,
        kind: 'lock',
        ran: false,
        inputIndices: [],
        outputIndices: [1],
        witnessIndex: null,
        script: LONG_TX_SENDER_LOCK,
        cycles: null,
      },
    ],
    witnessDetails: Array.from({ length: LONG_TX_INPUTS }, (_, i) =>
      i === 0
        ? {
            index: 0,
            size: 85,
            kind: 'witness_args' as const,
            fields: [
              // the WitnessArgs bytes over each span: its total size and three offsets, the
              // lock's length (65), then the lock
              { name: 'header', start: 0, end: 16, hex: '0x55000000100000005500000055000000' },
              { name: 'lock length', start: 16, end: 20, hex: '0x41000000' },
              { name: 'lock', start: 20, end: 85, hex: `0x${'ab'.repeat(65)}` },
            ],
          }
        : { index: i, size: 0, kind: 'empty' as const, fields: [] }
    ),
    witnesses: Array.from({ length: LONG_TX_INPUTS }, (_, i) => (i === 0 ? witnessArgs : '0x')),
    witnessesAvailable: true,
    headerDeps: [],
    cellbaseWitness: null,
    // what the sender's lock lost: the deposit and the fee (its change came back to it)
    feePaidBy: [{ lockHash: sender, address: LONG_TX_SENDER, outflow: (DEPOSIT + FEE).toString() }],
  } as unknown as TransactionDetail;
}
