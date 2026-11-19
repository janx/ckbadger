import { addressScript } from '@/lib/lock-script';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { CellScriptGroups, CellTransactions } from '@/components/cell/cell-transactions';
import type { Cell, TransactionDetail } from '@/lib/api';
import liveFixture from '@/__tests__/fixtures/d5/cell-live.json';
import spentFixture from '@/__tests__/fixtures/d5/cell-spent.json';
import { expectPrompt } from '@/__tests__/utils/prompt-heading';
import { canvasLines, seamLines, seamOverlay } from '@/__tests__/utils/tx-lines';
import { scriptHash } from '@/__tests__/utils/ckb-hash';

const getTransactionDetail = vi.fn();
const getTransactionCellDeps = vi.fn();
vi.mock('@/lib/api', () => ({
  api: {
    getTransactionDetail: (...args: unknown[]) => getTransactionDetail(...args),
    getTransactionCellDeps: (...args: unknown[]) => getTransactionCellDeps(...args),
    lookupScripts: async () => ({}),
  },
}));
vi.mock('@/components/transaction/tx-structure', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/components/transaction/tx-structure')>();
  return {
    TxStructure: (props: React.ComponentProps<typeof actual.TxStructure>) => (
      <div data-testid="structure">
        <actual.TxStructure {...props} />
      </div>
    ),
  };
});

const TYPE_ID_CODE_HASH = '0x00000000000000000000000000000000000000000000000000545950455f4944';
// B4 second fix round: a tile's cell carries the lock its captured address spells (a tile refuses
// an address that spells another lock). Third round (the coordinator's item 8): the lock groups'
// script is that lock too (it was the same code hash with no args, so no cell's lock was its
// group's); the live and the spent cell have one owner.
const lock = addressScript(liveFixture.address);
const typeId = { codeHash: TYPE_ID_CODE_HASH, hashType: 'type', args: '0x' };
/** The real script hashes of `lock` and of `typeId` (blake2b, CKB personalisation). */
const LOCK_HASH = '0xaa368106113229017f9e5ef36fdd33cc71e28cad8e738d8fbf9bd79a5cdad8ab';
const TYPE_ID_HASH = '0x4d9407e453331c7879396c04ef5dcd1597ee896821a0970e6c6fe9223867fcbb';
/**
 * As the API lists them (transaction_structure::script_groups): every cell's lock is in a group —
 * the inputs' ran, the outputs' did not (a second group of the same hash) — and the Type ID group
 * lists the input and the output that carry it. Lock groups read the witness at their first input
 * (WitnessArgs), as the type group does.
 */
function lockGroups(cellbase: boolean) {
  return [
    ...(cellbase
      ? []
      : [
          {
            scriptHash: LOCK_HASH,
            kind: 'lock',
            ran: true,
            inputIndices: [0],
            outputIndices: [],
            witnessIndex: 0,
            script: lock,
            cycles: null,
          },
        ]),
    {
      scriptHash: LOCK_HASH,
      kind: 'lock',
      ran: false,
      inputIndices: [],
      outputIndices: [0, 1],
      witnessIndex: null,
      script: lock,
      cycles: null,
    },
  ];
}
function tx(hash: string, cellbase = false): TransactionDetail {
  return {
    hash,
    status: 'committed',
    fee: '0',
    isCellbase: cellbase,
    timestamp: '2024-01-01T00:00:00Z',
    // as the API serves them: always both, the sums of the cells (a cellbase consumes none: 0)
    inputsCapacity: cellbase ? '0' : '10000000000',
    outputsCapacity: '20000000000',
    inputs: cellbase
      ? []
      : [
          {
            previousOutput: { txHash: spentFixture.txHash, index: spentFixture.outputIndex },
            capacity: '10000000000',
            commonKnowledgeSize: '61',
            address: spentFixture.address,
            lock: addressScript(spentFixture.address),
            type: typeId,
            display: null,
          },
        ],
    outputs: [0, 1].map((index) => ({
      capacity: '10000000000',
      commonKnowledgeSize: '61',
      address: liveFixture.address,
      lock: addressScript(liveFixture.address),
      ...(index === 0 ? { type: typeId } : {}),
      status: 'live',
      display: null,
    })),
    scriptGroups: [
      ...lockGroups(cellbase),
      {
        scriptHash: TYPE_ID_HASH,
        kind: 'type',
        ran: !cellbase,
        inputIndices: cellbase ? [] : [0],
        outputIndices: [0],
        witnessIndex: cellbase ? null : 0,
        script: typeId,
        cycles: cellbase ? null : '123',
      },
    ],
    witnessDetails: cellbase ? [] : [{ index: 0, size: 42, kind: 'opaque', fields: [] }],
  } as unknown as TransactionDetail;
}
/** The executed and the attached lock groups of one script hash, beside the Type ID group. */
function attachedGroups(detail: TransactionDetail): TransactionDetail {
  const hash = LOCK_HASH;
  return {
    ...detail,
    scriptGroups: [
      {
        scriptHash: hash,
        kind: 'lock',
        ran: true,
        inputIndices: [0],
        outputIndices: [],
        witnessIndex: 0,
        script: lock,
        cycles: '123',
      },
      {
        scriptHash: hash,
        kind: 'lock',
        ran: false,
        inputIndices: [],
        // every output's lock is in a group: output 1's too
        outputIndices: [0, 1],
        witnessIndex: null,
        script: lock,
        cycles: null,
      },
      // the Type ID group of input 0 and output 0, as before
      ...detail.scriptGroups!.filter((group) => group.kind === 'type'),
    ],
  } as unknown as TransactionDetail;
}
/**
 * Where a fixture transaction disagrees with itself (lane B4 third round, the coordinator's item
 * 8): a group whose hash is not its script's, or a cell whose lock (or type) is not the script of
 * the group that lists it.
 */
function disagreements(detail: TransactionDetail): string[] {
  type Script = { codeHash: string; hashType: string; args: string };
  const groups = detail.scriptGroups! as unknown as {
    kind: 'lock' | 'type';
    scriptHash: string;
    script: Script;
    inputIndices: number[];
    outputIndices: number[];
  }[];
  const out: string[] = [];
  for (const g of groups)
    if (g.scriptHash !== scriptHash(g.script))
      out.push(`${g.kind} group ${g.scriptHash} is the hash of no script it holds`);
  const sides = [
    ['input', detail.inputs ?? [], (g: (typeof groups)[number]) => g.inputIndices],
    ['output', detail.outputs ?? [], (g: (typeof groups)[number]) => g.outputIndices],
  ] as const;
  for (const [side, cells, indices] of sides)
    (cells as unknown as { lock: Script; type?: Script | null }[]).forEach((cell, i) => {
      for (const kind of ['lock', 'type'] as const) {
        const script = cell[kind];
        if (!script) continue;
        const listing = groups.filter((g) => g.kind === kind && indices(g).includes(i));
        if (listing.length === 0) out.push(`${side} ${i}'s ${kind} is in no group`);
        for (const g of listing)
          if (JSON.stringify(g.script) !== JSON.stringify(script))
            out.push(`${side} ${i}'s ${kind} is not the script of its group ${g.scriptHash}`);
      }
    });
  return out;
}
/**
 * The cell's transactions as the page draws them (lane D3, re-pinned on purpose: it was the one
 * section, the groups inside each transaction's structure): the embed in tier 1, then the script
 * groups of the developer tier, in the transaction page's rows.
 */
function view(cell: Cell) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <CellTransactions cell={cell} />
      <CellScriptGroups cell={cell} />
    </QueryClientProvider>
  );
}
describe('CellTransactions', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getTransactionDetail.mockImplementation(async (hash: string) => tx(hash));
    getTransactionCellDeps.mockResolvedValue([]);
  });
  it('uses the real Cell API live producer and shows current candidates without claiming a future execution', async () => {
    const cell = {
      ...liveFixture,
      currentSpendRequirements: [
        {
          kind: 'lock',
          referenceHash: lock.codeHash,
          hashType: 'type',
          resolution: 'ambiguous',
          versionHashes: [`0x${'aa'.repeat(32)}`, `0x${'bb'.repeat(32)}`],
          candidateCells: [
            {
              txHash: `0x${'cc'.repeat(32)}`,
              outputIndex: 1,
              versionHash: `0x${'aa'.repeat(32)}`,
              live: true,
            },
          ],
        },
      ],
    } as unknown as Cell;
    view(cell);
    expect(
      (await screen.findByTestId('structure')).querySelector('[data-cell-id="out:1"]')
    ).toHaveAttribute('data-highlighted', 'true');
    expect(screen.getByText(/Ambiguous current code cells/)).toBeInTheDocument();
    expect(screen.getByTitle(/^Candidate cell 0x(cc){32}:1/)).toHaveTextContent(
      '0xcccccccc…cccccc:1'
    );
    expect(screen.getByText(/Built-in Type ID/)).toBeInTheDocument();
    expect(screen.queryByText(/Spent transaction/)).not.toBeInTheDocument();
    // Phase 5 (D11): a live cell's transactions are the one that created it.
    expectPrompt(screen.getByRole('heading', { name: 'Transactions 1' }), 'Transactions', '1');
  });
  // C11: each transaction's two sheets end their heads with the API's exact sums, as read. Round
  // 2: the "Capacity entering the pool" line that repeated inputsCapacity under the spending
  // transaction (and was left out for the cellbase) is gone; the consumed head states it once, the
  // cellbase's as the API's 0.
  it('heads the sheets with the API’s exact totals, a cellbase’s consumed 0 included', async () => {
    const cell = spentFixture as unknown as Cell;
    getTransactionDetail.mockImplementation(async (hash: string) => tx(hash, hash === cell.txHash));
    view(cell);
    await screen.findAllByText(/in:0/);
    const totals = (side: string) =>
      Array.from(document.querySelectorAll(`[data-sheet="${side}"] [data-sheet-total]`)).map(
        (t) => t.textContent
      );
    // the creating cellbase, then the spending transaction
    // re-pinned on purpose (lane P1, lane D3's note): a sheet's total is exact, its trailing zeros
    // dropped (it was the column form, "100.00000000 CKB")
    expect(totals('consumed')).toEqual(['0 CKB', '100 CKB']);
    expect(totals('created')).toEqual(['200 CKB', '200 CKB']);
    expect(screen.queryByText(/entering the pool/i)).toBeNull();
    // C11 round 3: a line across a seam only where a cell continues: none in the cellbase (it
    // consumes nothing), the Type ID's one in the spending transaction
    const structures = Array.from(document.querySelectorAll<HTMLElement>('[data-tx-canvas]'));
    expect(structures.map((s) => seamLines(s).length)).toEqual([0, 1]);
    expect(seamOverlay(structures[0])).toBeNull();
    expect(seamLines(structures[1])[0]).toHaveAttribute('data-continuity-kind', 'type-id');
    // beyond them, only the onward bridge that carries the cell from its creating cellbase on
    const bridge = structures[0].querySelector('[data-onward-bridge] path');
    expect(canvasLines(structures[0].parentElement!)).toEqual([bridge]);
    expect(canvasLines(structures[1].parentElement!)).toEqual(seamLines(structures[1]));
  });
  it('uses the real Cell API spent producer and focuses its exact spending input', async () => {
    const cell = spentFixture as unknown as Cell;
    getTransactionDetail.mockImplementation(async (hash: string) => tx(hash, hash === cell.txHash));
    view(cell);
    expect((await screen.findAllByText(/in:0/)).length).toBeGreaterThan(0);
    // Re-pinned on purpose (lane D3, PA-24): the groups are the developer tier's rows in the
    // transaction page's words (they were the structure's own list: "Witness #0 · 42 bytes",
    // "Cellbase group: no script execution").
    const [created, spent] = await Promise.all(
      ['created', 'spent'].map((t) =>
        screen.findByRole('heading', { name: `Script groups of the transaction that ${t} it` })
      )
    );
    const rows = (heading: HTMLElement) =>
      Array.from(heading.closest('section')!.querySelectorAll<HTMLElement>('[data-group-row]'));
    // the spending transaction's lock group and its type group both read witness 0
    await waitFor(() =>
      expect(
        rows(spent).filter((r) => /#0 · opaque bytes, 42\sB/.test(r.textContent!))
      ).toHaveLength(2)
    );
    // the creating cellbase runs none of its groups: its outputs' lock group and its Type ID group
    await waitFor(() => expect(rows(created)).toHaveLength(2));
    for (const row of rows(created)) expect(row).toHaveTextContent('attached, runs when spent');
    expect(getTransactionDetail).toHaveBeenCalledWith(cell.consumedByTx);
    // Phase 5 (D11): a spent cell's are two, the one that created it and the one that spent it.
    expectPrompt(screen.getByRole('heading', { name: 'Transactions 2' }), 'Transactions', '2');
  });
  // Lane U follow-up (user report: "two Script groups sections"): a spent cell shows its creating
  // and its spending transaction, and each listed its groups under the same bare "Script groups".
  // Each list is named by its transaction (the Cell board: "Script groups of that transaction"),
  // beside that transaction's cells; nothing in either list is dropped.
  it('names each transaction’s script groups by that transaction, never two bare "Script groups"', async () => {
    const cell = spentFixture as unknown as Cell;
    view(cell);
    await screen.findAllByText(/in:0/);
    const headings = screen.getAllByRole('heading', { name: /script groups/i });
    expect(headings.map((h) => h.textContent)).toEqual([
      'Script groups of the transaction that created it',
      'Script groups of the transaction that spent it',
    ]);
    expect(screen.queryByRole('heading', { name: 'Script groups' })).toBeNull();
    // both transactions keep their group rows: witness, code and cycles (in each, the lock group
    // and the Type ID group read witness 0; the Type ID group ran 123 cycles). Re-pinned on
    // purpose (lane D3, PA-24): in the transaction page's rows and words, "#0 · opaque bytes,
    // 42 B" (it was the structure's "Witness #0 · 42 bytes")
    await waitFor(() =>
      expect(
        Array.from(document.querySelectorAll('[data-group-row]')).filter((r) =>
          /#0 · opaque bytes, 42\sB/.test(r.textContent!)
        )
      ).toHaveLength(4)
    );
    expect(screen.getAllByText('123 cycles')).toHaveLength(2);
  });
  // U-16 (UI review 2026-11-14): a group's code dep wrote its full out point, which wrapped across
  // two lines in the middle ("0x71a7ba8f…" / "…72e46c:0 (dep_group)"). The dep is a reference
  // here, so it reads as the transaction page writes it: its kind, then the out point shortened and
  // linked to its cell, one unit.
  it('writes a group’s code dep as the transaction page does: its kind and the short out point', async () => {
    const DEP = `0x${'71a7ba8f'.repeat(8)}`;
    getTransactionCellDeps.mockResolvedValue([
      // re-pinned on purpose (lane D3): the dep as the API sends it, with the output locks it holds
      // code for and its resolved cells (the transaction page's rows read both)
      {
        outPointTxHash: DEP,
        outPointIndex: 0,
        depType: 'dep_group',
        scriptHashes: [LOCK_HASH],
        outputLockScriptHashes: [],
        resolvedCells: [],
      },
    ]);
    view(liveFixture as unknown as Cell);
    const short = `${DEP.slice(0, 10)}…${DEP.slice(-6)}:0`;
    const link = (await screen.findAllByRole('link', { name: short }))[0];
    expect(link).toHaveAttribute('href', `/mainnet/cell/${DEP}-0`);
    expect(link.firstElementChild).toHaveAttribute('data-nobreak');
    expect(link.parentElement).toHaveTextContent(
      new RegExp(`^dep group ${short.replace(/[.]/g, '\\.')}$`)
    );
    expect(screen.queryByText(new RegExp(DEP))).toBeNull();
    expect(screen.queryByText(/\(dep_group\)/)).toBeNull();
  });

  // B4 third round (the coordinator's item 8): the fixture agrees with itself, as the API's
  // transactions do: each group's hash is its script's, and each cell's lock and type are the
  // scripts of the groups that list it; the cells' own fixtures carry their lock's hash
  it('its fixtures agree with themselves', () => {
    const hash = `0x${'ee'.repeat(32)}`;
    expect(disagreements(tx(hash))).toEqual([]);
    expect(disagreements(tx(hash, true))).toEqual([]);
    expect(disagreements(attachedGroups(tx(hash)))).toEqual([]);
    for (const cell of [liveFixture, spentFixture])
      expect(cell.lockScriptHash).toBe(scriptHash(cell.lock));
  });

  it('keeps executed and attached lock groups with the same script hash distinct', async () => {
    const hash = `0x${'77'.repeat(32)}`;
    getTransactionDetail.mockImplementation(async () => attachedGroups(tx(hash)));
    view(liveFixture as unknown as Cell);
    // Re-pinned on purpose (lane D3): the groups are the developer tier's rows (they were the
    // structure's list, whose hover lit the cells in the embed; the embed now draws this cell
    // alone and the cell page links nothing to it); each row keeps its own group and its cells
    await screen.findByTestId('structure');
    const groups = await waitFor(() => {
      const found = document.querySelectorAll<HTMLElement>('[data-group-row]');
      expect(found).toHaveLength(3);
      return Array.from(found);
    });
    // the groups' real hashes (B4 third round: they were 0x11… and 0x33…, the hashes of no
    // script in them)
    expect(groups[2]).toHaveAttribute('data-group-row', `type:run:${TYPE_ID_HASH}`);
    expect(groups[0]).toHaveAttribute('data-group-row', `lock:run:${LOCK_HASH}`);
    expect(groups[1]).toHaveAttribute('data-group-row', `lock:attached:${LOCK_HASH}`);
    // the lock that ran is input 0's; the attached one guards outputs 0 and 1
    const dots = (row: HTMLElement) => Array.from(row.querySelectorAll('[data-dot]'));
    expect(dots(groups[0]).map((d) => d.getAttribute('data-dot'))).toEqual(['ran']);
    expect(dots(groups[1]).map((d) => d.getAttribute('data-dot'))).toEqual([
      'attached',
      'attached',
    ]);
  });
});
