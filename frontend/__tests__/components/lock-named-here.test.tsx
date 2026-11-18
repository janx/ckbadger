// Readability lane B4, fix round (review-rules-logic MUST 1, MUST 3, SHOULD 2; review-rules-tests
// MUST-1, SHOULD-4, NOTE-2, NOTE-7; the coordinator's items 1, 2, 6, 9, 10): one decision says
// whether a lock is named where it stands, and by what words; every place reads it, and a place
// that is a part of a list leaves the part out when the answer is no.
//
// - The default lock is the pair (code hash 0x9bd7e06f…, hash type `type`). Where a row has an
//   address, the lock is read off it (code hash and hash type), not off a bare code hash.
// - A lock on the default code hash by `data` / `data1` / `data2` is another lock: the registry's
//   name (which names the code hash's `type` lock) is not its name, so it is written by its code
//   hash and hash type, "lock 0x9bd7e06f…a3cce8 by data".
// - In a column headed "Lock" no cell is empty: the default lock by its name, a lock that is not
//   indexed as "not indexed", both in quiet words without the column's tag.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { atViewportWidth } from '@/__tests__/utils/viewport';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { JOYID_CODE_HASH, seededAddress } from '@/__tests__/utils/ckb-address';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import { ActivityRow, type ActivityRowData } from '@/components/activity/activity-row';
import { TokenHolders } from '@/components/asset/token-holders';
import { CollectionHolders } from '@/components/asset/collection-holders';
import { WhatMoved } from '@/components/transaction/what-moved';
import { BalanceChanges, lockCodeHashesByAddress } from '@/components/transaction/balance-changes';
import { cellTiles } from '@/components/transaction/transaction-cell-tiles';
import { CELL_GAP_PX } from '@/components/ui/list-table';
import { api, type CollectionHolder, type GlobalActivity, type TransactionDetail } from '@/lib/api';
import { DEFAULT_LOCK_CODE_HASH } from '@/lib/lock-script';
import { columnPx, fontPx, textPx } from '@/lib/type-metrics';

/** RFC 0021's example args on the default lock by type, and the same code hash by data. */
const DEFAULT_ADDRESS =
  'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqdnnw7qkdnnclfkg59uzn8umtfd2kwxceqxwquc4';
const BY_DATA_ADDRESS =
  'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq9nnw7qkdnnclfkg59uzn8umtfd2kwxceqvguktl';
const BY_DATA_WORDS = 'lock 0x9bd7e06f…a3cce8 by data';

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

function party(address: string | null, lockKind: string | null, n: string, ckbDelta = '0') {
  return {
    address,
    lockHash: `0x${n.repeat(32)}`,
    lockHashPrefix: null,
    lockKind,
    roles: [],
    tags: 0,
    ckbDelta,
    usedDelta: '0',
    itemDeltas: [],
  };
}

describe('an activity row names a lock on the default code hash by data as another lock', () => {
  it('writes its code hash and hash type, never the default lock’s name, never silence', () => {
    const row: ActivityRowData = {
      txHash: `0x${'ab'.repeat(32)}`,
      blockNumber: 21000000,
      timestamp: String(Date.now() - 60_000),
      isCellbase: false,
      protocolActions: [],
      typeCalls: [],
      lockCalls: [],
      participants: [party(BY_DATA_ADDRESS, 'Default Lock', '01', '-100000000')],
    };
    const { container } = render(<ActivityRow activity={row} />);
    const kind = container.querySelector('[data-role="participant"] [data-role="lock-kind"]');
    expect(kind).toHaveTextContent(new RegExp(`^${BY_DATA_WORDS}$`));
  });
});

describe('a "Lock" column reads the lock off the holder’s address, and is never empty', () => {
  /** Each body row's cell under the column headed "Lock", and that column's `col`. */
  function lockColumn(table: HTMLElement) {
    const heads = within(table).getAllByRole('columnheader');
    const at = heads.findIndex((h) => h.textContent === 'Lock');
    expect(at).toBeGreaterThanOrEqual(0);
    const cells = within(table)
      .getAllByRole('row')
      .slice(1)
      .map((r) => within(r).getAllByRole('cell')[at]);
    const col = table.querySelectorAll('col')[at] as HTMLElement;
    return { cells, minWidth: Number.parseFloat(col.style.minWidth) };
  }

  const tokenHolder = (n: string, address: string | null, lockCodeHash: string | null) => ({
    lockScriptHash: `0x${n.repeat(32)}`,
    address,
    balance: '100',
    lockCodeHash,
    lockKind: lockCodeHash === null ? null : 'Default Lock',
  });

  it('in the token holders: default by its name, by data as another lock, not indexed in words', async () => {
    atContainerWidth(1248);
    vi.spyOn(api, 'getTokenHolders').mockResolvedValue({
      data: [
        tokenHolder('11', DEFAULT_ADDRESS, DEFAULT_LOCK_CODE_HASH),
        tokenHolder('22', BY_DATA_ADDRESS, DEFAULT_LOCK_CODE_HASH),
        tokenHolder('33', null, null),
      ],
      total: 3,
      limit: 10,
      hasMore: false,
      nextCursor: null,
      totalSupply: '300',
      anchor: null,
    });
    vi.spyOn(api, 'getTokenDistribution').mockRejectedValue(new Error('not needed'));
    render(<TokenHolders typeHash="0xabc" symbol="T" decimals={0} />);
    const table = await screen.findByRole('table', { name: 'Top 10 holders' });
    const { cells, minWidth } = lockColumn(table);
    const [byType, byData, notIndexed] = cells.map(
      (c) => c.querySelector('[data-lock-kind]') as HTMLElement
    );
    expect(byType).toHaveTextContent(/^Default Lock$/);
    expect(byType).not.toHaveClass('border');
    expect(byData).toHaveTextContent(new RegExp(`^${BY_DATA_WORDS}$`));
    expect(byData).toHaveClass('border');
    expect(notIndexed).toHaveTextContent(/^not indexed$/);
    expect(notIndexed).toHaveClass('text-text-dim');
    expect(notIndexed).not.toHaveClass('border');
    // NOTE-2: the column's budget, read from the table, holds the widest of its plain words whole
    // ("Default Lock", 12 glyphs at 12 px, 86.4; "not indexed", 79.2) and a 12-glyph tag (98.4)
    const tag = textPx(12, fontPx('meta')) + 2 * 5 + 2 * 1;
    expect(minWidth).toBe(columnPx(tag + CELL_GAP_PX));
    for (const words of ['Default Lock', 'not indexed'])
      expect(textPx(words.length, fontPx('meta'))).toBeLessThanOrEqual(minWidth - CELL_GAP_PX);
  });

  it('in the collection holders: not indexed in words, in the quiet ink', async () => {
    atContainerWidth(1248);
    const holder = (n: string, over: Partial<CollectionHolder>): CollectionHolder => ({
      lockScriptHash: `0x${n.repeat(32)}`,
      address: null,
      itemCount: 2,
      lockCodeHash: null,
      lockKind: null,
      ...over,
    });
    const load = vi.fn().mockResolvedValue({
      data: [
        holder('11', {
          address: DEFAULT_ADDRESS,
          lockCodeHash: DEFAULT_LOCK_CODE_HASH,
          lockKind: 'Default Lock',
        }),
        holder('22', {}),
      ],
      limit: 10,
      total: 2,
      hasMore: false,
      nextCursor: null,
    });
    render(<CollectionHolders assetClass="objects" load={load} queryKey="not-indexed" />);
    const table = await screen.findByRole('table', { name: 'Top 10 collection holders' });
    const { cells, minWidth } = lockColumn(table);
    for (const cell of cells) expect(cell.textContent?.trim()).not.toBe('');
    expect(cells[1].querySelector('[data-lock-kind]')).toHaveTextContent(/^not indexed$/);
    const tag = textPx(12, fontPx('meta')) + 2 * 5 + 2 * 1;
    expect(minWidth).toBe(columnPx(tag + CELL_GAP_PX));
  });
});

describe('a place that may name no lock leaves the part out', () => {
  it('the token holders’ phone row: no space and no empty slot after a default-lock address', async () => {
    atContainerWidth(358);
    atViewportWidth(390);
    vi.spyOn(api, 'getTokenHolders').mockResolvedValue({
      data: [
        {
          lockScriptHash: `0x${'11'.repeat(32)}`,
          address: DEFAULT_ADDRESS,
          balance: '100',
          lockCodeHash: DEFAULT_LOCK_CODE_HASH,
          lockKind: 'Default Lock',
        },
      ],
      total: 1,
      limit: 10,
      hasMore: false,
      nextCursor: null,
      totalSupply: '100',
      anchor: null,
    });
    vi.spyOn(api, 'getTokenDistribution').mockRejectedValue(new Error('not needed'));
    render(<TokenHolders typeHash="0xabc" symbol="T" decimals={0} />);
    const [row] = await screen.findAllByTestId('holder-row-mobile');
    expect(phoneLines(row)[0].left).toMatch(/xwquc4$/);
  });

  it('What moved: no empty lock span (and no gap) beside a default-lock party', () => {
    stubChartWidth(876);
    const a: GlobalActivity = {
      txHash: `0x${'54'.repeat(32)}`,
      blockNumber: 20_835_685,
      txIndex: 1,
      timestamp: '1761448067000',
      isCellbase: false,
      protocolActions: [],
      typeCalls: [],
      lockCalls: [],
      participants: [
        party(DEFAULT_ADDRESS, 'Default Lock', '01', '-100008888'),
        party(seededAddress('recvr1', JOYID_CODE_HASH), 'JoyID', '02', '100000000'),
      ],
    };
    const { container } = render(
      <WhatMoved
        hash={a.txHash}
        activity={a}
        fee="8888"
        compensation={null}
        lockCodeHashes={lockCodeHashesByAddress([
          {
            address: DEFAULT_ADDRESS,
            lock: { codeHash: DEFAULT_LOCK_CODE_HASH, hashType: 'type', args: '0x' },
          },
          {
            address: a.participants[1].address,
            lock: { codeHash: JOYID_CODE_HASH, hashType: 'type', args: '0x' },
          },
        ])}
      />
    );
    // a span with no content that is not a mark (the ribbon's 4 px swatch is aria-hidden)
    const empty = [...container.querySelectorAll('[data-flow-end="party"] span')].filter(
      (s) => s.childNodes.length === 0 && s.getAttribute('aria-hidden') !== 'true'
    );
    expect(empty).toEqual([]);
  });

  it('Balance changes: a party a protocol named, with no cell here, is silent on the default lock', () => {
    stubChartWidth(912);
    const base = feedSamples.objectTransfer;
    const named = party(DEFAULT_ADDRESS, 'Default Lock', '0c');
    const a: GlobalActivity = { ...base, participants: [...base.participants, named] };
    render(
      <BalanceChanges
        activity={a}
        fee="64119"
        compensation={null}
        lockCodeHashes={lockCodeHashesByAddress(
          base.participants.map((p) => ({
            address: p.address!,
            lock: { codeHash: DEFAULT_LOCK_CODE_HASH, hashType: 'type', args: '0x' },
          }))
        )}
      />
    );
    expect(document.body).not.toHaveTextContent('Default Lock');
  });
});

describe('the transaction’s cell tiles read the same decision', () => {
  const output = (codeHash: string, isChange: boolean, seed: string) => ({
    capacity: '10000000000',
    commonKnowledgeSize: '61',
    status: 'live' as const,
    display: null,
    address: seededAddress(seed, codeHash),
    lock: { codeHash, hashType: 'type', args: `0x${'00'.repeat(14)}${'61'.repeat(6)}` },
    type: null,
    isChange,
  });

  it('writes no lock beside a default-lock owner, "change" alone on its change, another lock’s name', () => {
    const outputs = [
      output(DEFAULT_LOCK_CODE_HASH, false, 'aaaaaa'),
      output(DEFAULT_LOCK_CODE_HASH, true, 'bbbbbb'),
      output(JOYID_CODE_HASH, false, 'cccccc'),
    ];
    const tx = {
      hash: `0x${'ee'.repeat(32)}`,
      status: 'committed',
      isCellbase: false,
      inputs: [],
      outputs,
      scriptGroups: outputs.map((o, i) => ({
        scriptHash: `0x${String(i + 1).repeat(64)}`,
        kind: 'lock',
        ran: false,
        inputIndices: [],
        outputIndices: [i],
        witnessIndex: null,
        script: o.lock,
        cycles: null,
      })),
    } as unknown as TransactionDetail;
    const lookup = {
      [DEFAULT_LOCK_CODE_HASH]: { name: 'Default Lock' },
      [JOYID_CODE_HASH]: { name: 'JoyID' },
    } as never;
    const tiles = cellTiles(tx, lookup).outputs;
    const words = tiles.map((t) => {
      const { container } = render(<span>{t.lockKind}</span>);
      return container.textContent;
    });
    expect(words).toEqual(['', 'change', 'JoyID']);
  });
});
