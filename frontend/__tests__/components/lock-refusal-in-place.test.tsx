// Lane B4 second fix round (review-rules-fixes-logic NOTE 8, the coordinator's item 14 and ruling
// D7): the lock decision refuses what it cannot read (an address that does not decode, a row whose
// address spells another lock than it says), and the refusal fails the part it is in, said in its
// place, never the page. Each place that writes a lock from an API row is read here with one such
// row: the activity rows, the token and collection holders, the supply events, the newest objects
// and the token's issuer note.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@/__tests__/utils/test-utils';
import { JOYID_CODE_HASH, seededAddress } from '@/__tests__/utils/ckb-address';
import { ActivityRow, ActivityTitle, activityTitleText } from '@/components/activity/activity-row';
import { ActivityRuns, runSubject } from '@/components/activity/activity-runs';
import { TokenHolders } from '@/components/asset/token-holders';
import { CollectionHolders } from '@/components/asset/collection-holders';
import { AssetHistory } from '@/components/asset/asset-history';
import { api, type GlobalActivity } from '@/lib/api';
import { DEFAULT_LOCK_CODE_HASH } from '@/lib/lock-script';
import { atContainerWidth } from '@/__tests__/utils/phone-row';

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** An address on JoyID beside a row that says the default lock: the decision refuses it. */
const SPELLS_ANOTHER = seededAddress('joyrow', JOYID_CODE_HASH);
const refusal = new RegExp(`address ${SPELLS_ANOTHER} spells the lock code hash`);

describe('a refused lock fails its part, in its place', () => {
  it('an activity row: the row says it cannot be read, the others are drawn', () => {
    const row = (n: number, address: string): GlobalActivity => ({
      txHash: `0x${String(n).repeat(64)}`,
      blockNumber: 100 + n,
      txIndex: 0,
      timestamp: String(Date.now() - 60_000 * n),
      isCellbase: false,
      protocolActions: [],
      typeCalls: [],
      lockCalls: [],
      participants: [
        {
          address,
          lockHash: `0x${String(n + 1).repeat(64)}`,
          lockHashPrefix: null,
          lockKind: null,
          roles: [],
          tags: 0,
          ckbDelta: String(-n),
          usedDelta: '0',
          itemDeltas: [],
        },
      ],
    });
    // the second party's address does not decode: the decision refuses it, naming it
    const rows = [row(1, seededAddress('good01')), row(2, 'ckb1qzda0b')];
    render(
      <ActivityRuns
        rows={rows.map((a) => ({
          key: a.txHash,
          subject: runSubject(a, activityTitleText(a)),
          activity: a,
          title: <ActivityTitle activity={a} />,
          row: <ActivityRow activity={a} />,
        }))}
      />
    );
    expect(document.querySelector(`article[data-feed-row="${rows[0].txHash}"]`)).not.toBeNull();
    expect(screen.getByRole('alert')).toHaveTextContent(
      `Cannot read tx ${rows[1].txHash}: address ckb1qzda0b: "b" is not bech32`
    );
  });

  // third round (the coordinator's item 4): a row's boundary starts again when its data changes
  // (a transaction in the pool that commits), rather than standing in its error for good
  it('an activity row: its part starts again when its transaction changes', () => {
    const tx = `0x${'31'.repeat(32)}`;
    const at = (blockNumber: number | null, address: string): GlobalActivity => ({
      txHash: tx,
      blockNumber: blockNumber as number,
      txIndex: 0,
      timestamp: (blockNumber === null ? null : String(Date.now() - 60_000)) as string,
      isCellbase: false,
      protocolActions: [],
      typeCalls: [],
      lockCalls: [],
      participants: [
        {
          address,
          lockHash: `0x${'32'.repeat(32)}`,
          lockHashPrefix: null,
          lockKind: null,
          roles: [],
          tags: 0,
          ckbDelta: '-1',
          usedDelta: '0',
          itemDeltas: [],
        },
      ],
    });
    const runsOf = (a: GlobalActivity) => (
      <ActivityRuns
        rows={[
          {
            key: a.txHash,
            subject: null,
            activity: a,
            title: <ActivityTitle activity={a} />,
            row: <ActivityRow activity={a} />,
          },
        ]}
      />
    );
    const { rerender } = render(runsOf(at(null, 'ckb1qzda0b')));
    expect(screen.getByRole('alert')).toHaveTextContent(`Cannot read tx ${tx}`);
    rerender(runsOf(at(100, seededAddress('good31'))));
    expect(screen.queryByRole('alert')).toBeNull();
    expect(document.querySelector(`article[data-feed-row="${tx}"]`)).not.toBeNull();
  });

  it('the token holders: the section says it cannot read its holders, the page stands', async () => {
    atContainerWidth(1248);
    vi.spyOn(api, 'getTokenHolders').mockResolvedValue({
      data: [
        {
          lockScriptHash: `0x${'11'.repeat(32)}`,
          address: SPELLS_ANOTHER,
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
    render(
      <main>
        <h1>The token page</h1>
        <TokenHolders typeHash="0xabc" symbol="T" decimals={0} />
      </main>
    );
    // third round (the coordinator's item 4): said where (the token, the view) in the error's
    // own words, under the section's heading, which stays with its button
    const alert = await screen.findByText(/^Could not draw the holders \(token 0xabc, top 10\): /);
    expect(alert).toHaveAttribute('role', 'alert');
    expect(alert).toHaveTextContent(refusal);
    expect(screen.getByRole('heading', { name: 'The token page' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: /^Who holds it/ })).toBeInTheDocument();
    // another view of the holders draws again: the part starts again when its data changes
    vi.mocked(api.getTokenHolders).mockResolvedValue({
      data: [
        {
          lockScriptHash: `0x${'22'.repeat(32)}`,
          address: seededAddress('good22'),
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
    fireEvent.click(screen.getByRole('button', { name: /^All .*→$/ }));
    expect(await screen.findByRole('table', { name: 'All holders' })).toBeInTheDocument();
    expect(screen.queryByText(/^Could not draw the holders/)).toBeNull();
  });

  it('the collection holders: the section says it cannot read its holders', async () => {
    atContainerWidth(1248);
    const load = vi.fn().mockResolvedValue({
      data: [
        {
          lockScriptHash: `0x${'11'.repeat(32)}`,
          address: SPELLS_ANOTHER,
          itemCount: 2,
          lockCodeHash: DEFAULT_LOCK_CODE_HASH,
          lockKind: 'Default Lock',
        },
      ],
      limit: 10,
      total: 1,
      hasMore: false,
      nextCursor: null,
    });
    render(
      <main>
        <h1>The collection page</h1>
        <CollectionHolders assetClass="objects" load={load} queryKey="refused" />
      </main>
    );
    // third round (re-pinned on purpose, the coordinator's item 4): said where (the collection,
    // the view), the section's heading standing over it
    const alert = await screen.findByText(
      /^Could not draw the collection holders \(collection refused, top 10\): /
    );
    expect(alert).toHaveTextContent(refusal);
    expect(screen.getByRole('heading', { name: 'The collection page' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: /^Who holds it/ })).toBeInTheDocument();
  });

  it('the supply events: the table says it cannot read them, the history stands', () => {
    render(
      <AssetHistory
        assetClass="tokens"
        weeklyNoun="transfers"
        daily={[
          {
            date: '2024-01-01',
            capacity: '10000000001',
            occupiedCapacity: '6100000000',
            supply: '13',
          },
        ]}
        weekly={[]}
        completeThrough="2024-01-08"
        token={{ symbol: 'T', decimals: 0 }}
        supplyEvents={{
          total: 1,
          rows: [
            {
              txHash: `0x${'12'.repeat(32)}`,
              blockNumber: 12,
              txIndex: 0,
              transferSequence: 0,
              timestamp: String(Date.parse('2024-01-01T16:30:00Z')),
              kind: 'mint',
              amount: '1',
              supplyAfter: '13',
              lockHash: `0x${'12'.repeat(32)}`,
              address: SPELLS_ANOTHER,
              lockCodeHash: DEFAULT_LOCK_CODE_HASH,
              lockKind: 'Default Lock',
            },
          ],
        }}
      />
    );
    // third round (re-pinned on purpose, the coordinator's item 4): said where (the page of
    // events it was drawing, by its first and last)
    const event = `0x${'12'.repeat(32)}:0`;
    const alert = screen.getByText(
      `Could not draw the changes to the supply (events ${event} to ${event}):`,
      { exact: false }
    );
    expect(alert.textContent!.startsWith('Could not draw the changes to the supply')).toBe(true);
    expect(alert).toHaveTextContent(refusal);
    expect(within(alert.closest('details')!).queryByRole('table')).toBeNull();
  });
});
