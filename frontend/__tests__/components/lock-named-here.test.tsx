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
import { lineRuns } from '@/__tests__/utils/line-runs';
import { ckbAddress } from '@/__tests__/utils/ckb-address';
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
import { BalanceChanges } from '@/components/transaction/balance-changes';
import { cellTiles } from '@/components/transaction/transaction-cell-tiles';
import { CELL_GAP_PX } from '@/components/ui/list-table';
import { api, type CollectionHolder, type GlobalActivity, type TransactionDetail } from '@/lib/api';
import { DEFAULT_LOCK_CODE_HASH } from '@/lib/lock-script';
import { scriptLookupKey } from '@/lib/script-pair';
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
      // re-keyed (lane rd/api-lock-names, fix round ruling 2): the API names the code a pair runs,
      // and a lock on the default lock's code hash by data runs none, so it sends no name for it
      participants: [party(BY_DATA_ADDRESS, null, '01', '-100000000')],
    };
    const { container } = render(<ActivityRow activity={row} />);
    const kind = container.querySelector('[data-role="participant"] [data-role="lock-kind"]');
    expect(kind).toHaveTextContent(new RegExp(`^${BY_DATA_WORDS}$`));
    // B4 second fix round (review-rules-fixes-logic SHOULD 2, the coordinator's item 4): its link
    // carries its hash type and kind; the script page keeps a code hash its family does not
    // reference by that hash type on the code hash, read by it (third round; it sent every named
    // code hash to its family, the default lock's: script-code-hash.test.tsx)
    expect(kind!.querySelector('a')).toHaveAttribute(
      'href',
      `/mainnet/script/${DEFAULT_LOCK_CODE_HASH}?hashType=data&kind=lock`
    );
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

  // re-keyed (lane rd/api-lock-names, fix round ruling 2): the API names the code a pair runs, and
  // a lock on the default lock's code hash by data runs none, so it sends no name for it
  const tokenHolder = (n: string, address: string | null, lockCodeHash: string | null) => ({
    lockScriptHash: `0x${n.repeat(32)}`,
    address,
    balance: '100',
    lockCodeHash,
    lockKind: lockCodeHash === null || address === BY_DATA_ADDRESS ? null : 'Default Lock',
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
    // re-pinned on purpose (lane p-assets): a lock is plain words, no chip; another lock reads in
    // the reading ink (it kept its tag's border)
    expect(byData).not.toHaveClass('border');
    expect(byData).toHaveClass('text-text');
    expect(notIndexed).toHaveTextContent(/^not indexed$/);
    expect(notIndexed).toHaveClass('text-text-dim');
    expect(notIndexed).not.toHaveClass('border');
    // B4 second fix round (review-rules-fixes-logic SHOULD 3, tests SHOULD-3; the coordinator's
    // item 5): each row's title says what its Lock cell says, from the one decision
    const titles = within(table)
      .getAllByRole('row')
      .slice(1)
      .map((r) => r.getAttribute('title')!);
    expect(titles[0]).toContain(' · Default Lock · ');
    expect(titles[1]).toContain(` · ${BY_DATA_WORDS} · `);
    expect(titles[1]).not.toContain('Default Lock');
    expect(titles[2]).toContain(' · not indexed · ');
    // NOTE-2: the column's budget, read from the table, holds the widest of its plain words whole
    // ("Default Lock", 12 glyphs at 12 px, 86.4; "not indexed", 79.2) and a 12-glyph tag (98.4)
    // re-pinned on purpose (lane p-assets): 12 characters of a lock's name at the cells' 13 px, no
    // box (it was a tag of 12 characters at 12 px with its 5 px sides and 1 px border)
    expect(minWidth).toBe(columnPx(textPx(12, fontPx('small')) + CELL_GAP_PX));
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
        // re-keyed (lane rd/api-lock-names, fix round ruling 2): the API names the code a pair
        // runs, and a lock on the default lock's code hash by data runs none, so it sends no name
        // for it
        holder('33', {
          address: BY_DATA_ADDRESS,
          lockCodeHash: DEFAULT_LOCK_CODE_HASH,
          lockKind: null,
        }),
      ],
      limit: 10,
      total: 3,
      hasMore: false,
      nextCursor: null,
    });
    render(<CollectionHolders assetClass="objects" load={load} queryKey="not-indexed" />);
    const table = await screen.findByRole('table', { name: 'Top 10 collection holders' });
    const { cells, minWidth } = lockColumn(table);
    for (const cell of cells) expect(cell.textContent?.trim()).not.toBe('');
    expect(cells[1].querySelector('[data-lock-kind]')).toHaveTextContent(/^not indexed$/);
    // B4 second fix round (the coordinator's item 5): the row titles read the one decision
    const titles = within(table)
      .getAllByRole('row')
      .slice(1)
      .map((r) => r.getAttribute('title')!);
    expect(titles[0]).toContain(' · Default Lock · ');
    expect(titles[1]).toContain(' · not indexed · ');
    expect(titles[2]).toContain(` · ${BY_DATA_WORDS} · `);
    expect(titles[2]).not.toContain('Default Lock');
    // re-pinned on purpose (lane p-assets): 12 characters of a lock's name at the cells' 13 px, no
    // box (it was a tag of 12 characters at 12 px with its 5 px sides and 1 px border)
    expect(minWidth).toBe(columnPx(textPx(12, fontPx('small')) + CELL_GAP_PX));
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
      <WhatMoved hash={a.txHash} activity={a} fee="8888" compensation={null} />
    );
    // a span with no content that is not a mark (the ribbon's 4 px swatch is aria-hidden)
    const empty = [...container.querySelectorAll('[data-flow-end="party"] span')].filter(
      (s) => s.childNodes.length === 0 && s.getAttribute('aria-hidden') !== 'true'
    );
    expect(empty).toEqual([]);
    // B4 second fix round (tests NOTE-8): and the ends are drawn, the other lock named beside its
    // party (an absence alone passes on a section that draws nothing)
    const ends = [...container.querySelectorAll('[data-flow-end="party"]')];
    expect(ends.length).toBeGreaterThanOrEqual(2);
    expect(ends.some((e) => /JoyID/.test(e.textContent ?? ''))).toBe(true);
    expect(ends.some((e) => e.textContent?.includes(DEFAULT_ADDRESS.slice(-8)))).toBe(true);
  });

  it('Balance changes: a party a protocol named, with no cell here, is silent on the default lock', () => {
    stubChartWidth(912);
    const base = feedSamples.objectTransfer;
    const named = party(DEFAULT_ADDRESS, 'Default Lock', '0c');
    const a: GlobalActivity = { ...base, participants: [...base.participants, named] };
    render(<BalanceChanges activity={a} fee="64119" compensation={null} />);
    expect(document.body).not.toHaveTextContent('Default Lock');
    // B4 second fix round (tests NOTE-8): and the party is drawn, in its row of the table
    const table = screen.getByRole('table', { name: 'Balance changes by participant' });
    expect(within(table).getAllByRole('row')).toHaveLength(base.participants.length + 3);
    expect(table).toHaveTextContent(`ckb1…${DEFAULT_ADDRESS.slice(-8)}`);
  });
});

// B4 second fix round (review-rules-fixes-logic NOTE 5, the coordinator's item 11): the words a
// lock on the default code hash by data is written in are in the budget of the column that can
// write them. In Balance changes' participant track the party and its lock are two units with a
// line between them where they do not fit together, and "lock 0x…" and "by data" are two: every
// run of the cell fits the track the grid gives it.
describe('the by-data words fit the columns that write them', () => {
  it('in Balance changes’ participant track', () => {
    stubChartWidth(912);
    const base = feedSamples.objectTransfer;
    const a: GlobalActivity = {
      ...base,
      // re-keyed (lane rd/api-lock-names, fix round ruling 2): the API names the code a pair runs,
      // and a lock on the default lock's code hash by data runs none, so it sends no name for it
      participants: [...base.participants, party(BY_DATA_ADDRESS, null, '0d')],
    };
    render(<BalanceChanges activity={a} fee="64119" compensation={null} />);
    const table = screen.getByRole('table', { name: 'Balance changes by participant' });
    const rows = within(table).getAllByRole('row');
    const track = Number.parseFloat(rows[0].style.gridTemplateColumns.split(' ')[0]);
    const cell = within(rows[rows.length - 2]).getAllByRole('cell')[0];
    expect(cell).toHaveTextContent(BY_DATA_WORDS);
    const past = lineRuns(cell, 1280).filter((r) => r.px > track + 1e-9);
    expect(past.map((r) => `${r.text} ${r.px.toFixed(1)}`)).toEqual([]);
  });
});

describe('the transaction’s cell tiles read the same decision', () => {
  /** An output of `lock` (by type unless said), owned at that lock's real address. */
  const output = (
    codeHash: string,
    isChange: boolean,
    seed: string,
    hashType: 'type' | 'data' | 'data1' | 'data2' = 'type'
  ) => {
    // B4 second fix round (tests MUST-1): the address spells the cell's own lock, args and all
    const args = `0x${[...seed]
      .map((c) => c.charCodeAt(0).toString(16))
      .join('')
      .padStart(40, '0')}`;
    return {
      capacity: '10000000000',
      commonKnowledgeSize: '61',
      status: 'live' as const,
      display: null,
      address: ckbAddress({ codeHash, hashType, args }),
      lock: { codeHash, hashType, args },
      type: null,
      isChange,
    };
  };
  const txOf = (outputs: Record<string, unknown>[]) =>
    ({
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
    }) as unknown as TransactionDetail;
  // the registry's names, each by its script's pair (lane rd/api-lock-names): the default lock and
  // JoyID by type; the default lock's code hash by data is no pair the registry names
  const LOOKUP = {
    [scriptLookupKey(DEFAULT_LOCK_CODE_HASH, 'type')]: { name: 'Default Lock' },
    [scriptLookupKey(JOYID_CODE_HASH, 'type')]: { name: 'JoyID' },
  } as never;

  // B4 second fix round (review-rules-fixes-logic MUST 2, tests MUST-1; the two refusals the
  // removed lockCodeHashesByAddress tests held): a tile's owner and its lock are one cell's, so a
  // cell with an address and no lock, or an address that spells another lock, is refused with the
  // transaction and the cell named; a lock with no address form (args over 599 bytes: the API
  // sends no address) is owned by its lock hash, as a row writes a party with no address.
  it('refuses a cell with an address and no lock, naming the transaction and the output', () => {
    const cell = { ...output(JOYID_CODE_HASH, false, 'nolock'), lock: null };
    expect(() => cellTiles(txOf([cell]), LOOKUP)).toThrow(
      `Transaction 0x${'ee'.repeat(32)} missing required field "output 0.lock"`
    );
  });

  it('refuses a cell whose address spells another lock than its own, naming both', () => {
    const joy = output(JOYID_CODE_HASH, false, 'spells');
    const cell = { ...joy, lock: { ...joy.lock, codeHash: DEFAULT_LOCK_CODE_HASH } };
    expect(() => cellTiles(txOf([cell]), LOOKUP)).toThrow(
      new RegExp(
        `^Transaction 0x(ee){32} output 0: address ${joy.address} spells the lock ${JOYID_CODE_HASH} by type, args ${joy.lock.args}; the cell's lock is ${DEFAULT_LOCK_CODE_HASH} by type`
      )
    );
  });

  // B4 second fix round (review-rules-fixes-tests SHOULD-5, the coordinator's item 7): a tile
  // names a lock on the default code hash by data as another lock, by its code hash and hash type
  // (lockWordsText's "by <hash type>"), and refuses a hash type the chain does not have, naming it.
  it('names a by-data lock on the default code hash with its hash type, as text', () => {
    const outputs = [
      output(DEFAULT_LOCK_CODE_HASH, false, 'bydata', 'data'),
      output(DEFAULT_LOCK_CODE_HASH, true, 'bydat1', 'data1'),
    ];
    expect(cellTiles(txOf(outputs), LOOKUP).outputs.map((t) => t.lockKind)).toEqual([
      'lock 0x9bd7e06f…a3cce8 by data',
      'lock 0x9bd7e06f…a3cce8 by data1 · change',
    ]);
  });

  it('refuses a lock hash type the chain does not have, naming the word and the cell', () => {
    const cell = output(JOYID_CODE_HASH, false, 'weird');
    const odd = { ...cell, lock: { ...cell.lock, hashType: 'data3' } };
    expect(() => cellTiles(txOf([odd]), LOOKUP)).toThrow(
      `Transaction 0x${'ee'.repeat(32)} output 0: lock hash type "data3" is not a CKB hash type`
    );
  });

  it('owns a cell with no address form by its lock hash, its lock in words', () => {
    const args = `0x${'ab'.repeat(600)}`;
    const cell = {
      ...output(JOYID_CODE_HASH, false, 'long'),
      address: null,
      lock: { codeHash: JOYID_CODE_HASH, hashType: 'type', args },
    };
    const [tile] = cellTiles(txOf([cell]), LOOKUP).outputs;
    render(
      <span>
        <span data-testid="owner">{tile.owner}</span>
        <span data-testid="lock">{tile.lockKind}</span>
      </span>
    );
    const owner = screen.getByTestId('owner').querySelector('a')!;
    expect(owner).toHaveAttribute('href', `/mainnet/address/0x${'1'.repeat(64)}`);
    expect(owner).toHaveTextContent(/^0x11111111…111111$/);
    expect(screen.getByTestId('lock')).toHaveTextContent(/^JoyID$/);
  });

  it('writes no lock beside a default-lock owner, "change" alone on its change, another lock’s name', () => {
    const outputs = [
      output(DEFAULT_LOCK_CODE_HASH, false, 'aaaaaa'),
      output(DEFAULT_LOCK_CODE_HASH, true, 'bbbbbb'),
      output(JOYID_CODE_HASH, false, 'cccccc'),
    ];
    const tiles = cellTiles(txOf(outputs), LOOKUP).outputs;
    // B4 second fix round (review-rules-fixes-tests SHOULD-5, the coordinator's item 7)
    const words = tiles.map((t) => {
      const { container } = render(<span>{t.lockKind}</span>);
      return container.textContent;
    });
    expect(words).toEqual(['', 'change', 'JoyID']);
  });
});
