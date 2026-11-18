import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query';
import { ApiRequestError } from '@/lib/api';
import { TopicHero } from '@/components/ui/topic-hero';
import { ReadoutRow } from '@/components/ui/readout-row';
import { HelpPopover } from '@/components/ui/help-popover';
import { DerivePendingBanner, StoreCatchingUpBanner } from '@/components/ui/store-state-banner';
import { WarmupPendingBanner } from '@/components/ui/warmup-pending-banner';
import { NetworkInitializingBanner } from '@/components/ui/network-initializing-banner';
import { PoolTruncatedNotice, PoolUnavailableNotice } from '@/components/ui/pool-status';
import { CellTile } from '@/components/ui/cell-tile';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import { HexCode } from '@/components/ui/hex-code';
import { Hash } from '@/components/ui/hash';
import { TintedJson } from '@/components/ui/tinted-json';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { Header } from '@/components/layout/header';
import { SealedHead } from '@/components/ui/page-seal';
import { DetailLayout } from '@/components/ui/detail-layout';
import { SEAL_IDS } from '@/__tests__/utils/seals';
import { ListTable } from '@/components/ui/list-table';
import { CapacityPartition } from '@/components/ui/capacity-partition';
import { CellBar } from '@/components/ui/cell-bar';
import { ParticipantRefView } from '@/components/participant-ref';
import { ItemDeltaFact } from '@/components/activity-event-row';
import { ScriptNames } from '@/components/ui/script-byte-map';
import { sealedTile } from '@/__tests__/utils/cell-tile-seal';
import { fullText } from '@/__tests__/utils/full-text';
import { BRIGHT, QUIET, READING, inkOf } from '@/__tests__/utils/ink-role';

/**
 * Readability ruling R3: the quiet ink is not the default. Each shared renderer gives a word the
 * ink of its role (the colour guidelines' "Text hierarchy"): the reading ink for a value, a link, a
 * date, a number and a sentence that explains; the quiet ink for a label, a repeated unit, a
 * separator and a note. Each case names the element's role and the ink that role takes.
 */

afterEach(cleanup);

/** The ink of the one text node under `root` that reads `text` (a word beside a figure). */
function inkOfText(root: Element, text: string): string {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const hits: Text[] = [];
  for (let n = walker.nextNode(); n; n = walker.nextNode())
    if (n.nodeValue?.trim() === text) hits.push(n as Text);
  expect(hits, `one text node "${text}"`).toHaveLength(1);
  return inkOf(hits[0].parentElement!);
}

describe('TopicHero (figure layout): its sentences explain, its labels and notes stay quiet', () => {
  const hero = () =>
    render(
      <TopicHero
        layout="figure"
        title="Fiber channels"
        lede="A Fiber channel touches the chain only when it opens and when it closes."
        figure={{
          label: 'open now',
          value: '38',
          unit: 'channels',
          companion: 'locking 43,889 CKB',
          tone: 'jade',
        }}
        figures={[{ label: 'Channels ever', value: '345', sub: 'since 2025-02-28' }]}
      />
    );

  it('reads the lede, a sentence that explains, in the reading ink', () => {
    hero();
    expect(inkOf(screen.getByText(/^A Fiber channel touches the chain/))).toBe(READING);
  });

  it('reads the companion line, the figure’s value in words, in the reading ink', () => {
    hero();
    expect(inkOf(screen.getByText('locking 43,889 CKB'))).toBe(READING);
  });

  it('keeps the figure’s unit, its eyebrow, the row’s labels and the inline note quiet', () => {
    hero();
    expect(inkOf(screen.getByText('channels'))).toBe(QUIET); // a unit
    expect(inkOf(screen.getByText('open now'))).toBe(QUIET); // a label
    expect(inkOf(screen.getByRole('term'))).toBe(QUIET); // a label
    expect(inkOf(screen.getByText(fullText(/since 2025-02-28/)))).toBe(QUIET); // a note beside a figure
    expect(inkOf(screen.getByText('38'))).toBe('text-jade'); // the figure scanned first
  });
});

describe('ReadoutRow: the idle line is read, "selected" and the hint are quiet', () => {
  it('reads an idle sentence in the reading ink: it says what the figure reads', () => {
    render(
      <ReadoutRow label="r" pinned={false} idle="a dot reads its script version">
        {null}
      </ReadoutRow>
    );
    expect(inkOf(screen.getByText('a dot reads its script version'))).toBe(READING);
  });

  it('reads an idle reading (a value) in the reading ink', () => {
    render(
      <ReadoutRow label="r" pinned={false} idle="2026-11-17 · 3 uncles">
        {null}
      </ReadoutRow>
    );
    expect(inkOf(screen.getByText(fullText('2026-11-17 · 3 uncles')))).toBe(READING);
  });

  it('keeps "selected", a label before the pinned reading, and the hint, a note, quiet', () => {
    render(
      <ReadoutRow label="r" pinned>
        kdeakp
      </ReadoutRow>
    );
    expect(inkOf(screen.getByText('selected', { exact: false }))).toBe(QUIET);
    expect(inkOf(screen.getByText('tap or hover'))).toBe(QUIET);
    expect(inkOf(screen.getByText('kdeakp', { exact: false }))).toBe(READING);
  });
});

describe('HelpPopover: its body explains, its title labels', () => {
  it('reads the body in the reading ink and keeps the title quiet', () => {
    render(
      <HelpPopover label="Explain HMul" title="HMul">
        <p>H-multiplier = CKB held in its cells ÷ occupied.</p>
      </HelpPopover>
    );
    fireEvent.click(screen.getByRole('button', { name: 'Explain HMul' }));
    const dialog = screen.getByRole('dialog', { name: 'HMul' });
    expect(inkOf(within(dialog).getByText(/^H-multiplier/))).toBe(READING);
    expect(inkOf(within(dialog).getByText('HMul'))).toBe(QUIET);
  });
});

function ThrowingQuery({ error }: { error: unknown }) {
  useQuery({
    queryKey: ['ink-roles-banner'],
    queryFn: async () => {
      throw error;
    },
  });
  return null;
}

function renderBanners(error: unknown) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <WarmupPendingBanner />
      <NetworkInitializingBanner />
      <StoreCatchingUpBanner />
      <DerivePendingBanner />
      <ThrowingQuery error={error} />
    </QueryClientProvider>
  );
}

describe('the banners: their detail is a sentence that explains', () => {
  it('reads the catching-up detail (the API’s own sentence) in the reading ink', async () => {
    renderBanners(new ApiRequestError(503, 'store_catching_up', 'catch-up failed 3 times; io'));
    expect(inkOf(await screen.findByText('catch-up failed 3 times; io'))).toBe(READING);
  });

  it('reads "Retrying automatically." of the derivation banner in the reading ink', async () => {
    renderBanners(
      new ApiRequestError(503, 'derive_pending', 'x', {
        aggregate: 'dao_live_agg',
        state: 'not_started',
        rowsDone: null,
        derivedFromTip: null,
      })
    );
    const banner = await screen.findByRole('status', { name: 'Being derived' });
    expect(inkOf(within(banner).getByText('Retrying automatically.'))).toBe(READING);
  });

  it('reads the warm-up detail in the reading ink', async () => {
    renderBanners(new ApiRequestError(503, 'warmup_pending', 'the holder index is loading'));
    expect(inkOf(await screen.findByText('the holder index is loading'))).toBe(READING);
  });

  it('reads the waiting-to-sync detail in the reading ink', async () => {
    renderBanners(new ApiRequestError(503, 'initializing', 'mainnet syncs first'));
    expect(inkOf(await screen.findByText('mainnet syncs first'))).toBe(READING);
  });

  it('keeps the banner’s own word, a label, in its accent', async () => {
    renderBanners(new ApiRequestError(503, 'initializing', 'mainnet syncs first'));
    await waitFor(() => expect(screen.getByText('Waiting to sync')).toBeInTheDocument());
    expect(inkOf(screen.getByText('Waiting to sync'))).toBe('text-emphasis');
  });
});

describe('the pool notices explain why the list is short', () => {
  it('reads both notices in the reading ink', () => {
    render(
      <>
        <PoolTruncatedNotice />
        <PoolUnavailableNotice />
      </>
    );
    expect(inkOf(screen.getByText('More unconfirmed transactions than shown.'))).toBe(READING);
    expect(inkOf(screen.getByText(/^Pool view unavailable/))).toBe(READING);
  });
});

describe('CellTile: its out point is a link, its state and bytes are values', () => {
  const HASH = `0x${'7b'.repeat(32)}`;
  const tile = (outPoint?: { txHash: string; index: number }) =>
    sealedTile({
      id: 'out:0',
      testId: 'tile',
      href: `/cell/${HASH}:0`,
      label: 'output 0',
      owner: 'ckb1…qypqxyz',
      lockKind: 'omnilock',
      kind: 'ckb',
      content: 'CKB',
      capacity: '6100000000',
      occupied: '6100000000',
      state: 'live',
      outPoint,
    });

  it('reads the out point, the tile’s link, in the reading ink; the label beside it is quiet', () => {
    render(<CellTile cell={tile({ txHash: HASH, index: 0 })} />);
    const t = screen.getByTestId('tile');
    expect(inkOf(within(t).getByRole('link', { name: /^0x7b7b…7b7b:0$/ }))).toBe(READING);
    expect(inkOf(within(t).getByText('output 0'))).toBe(QUIET);
  });

  it('reads the label in the reading ink where it is the link (no out point)', () => {
    render(<CellTile cell={tile()} />);
    expect(inkOf(screen.getByRole('link', { name: 'output 0' }))).toBe(READING);
  });

  it('reads the state and the occupied bytes, values, in the reading ink; "occupied" is a label', () => {
    render(<CellTile cell={tile({ txHash: HASH, index: 0 })} />);
    const t = screen.getByTestId('tile');
    const row = t.querySelector<HTMLElement>('[data-tile-row="occupied"]')!;
    expect(inkOf(within(row).getByText('live'))).toBe(READING);
    expect(inkOfText(row, '61')).toBe(READING);
    expect(inkOfText(row, 'occupied')).toBe(QUIET);
  });

  it('keeps the lock’s kind, a label beside the owner, quiet', () => {
    render(<CellTile cell={tile({ txHash: HASH, index: 0 })} />);
    expect(inkOf(screen.getByText('omnilock'))).toBe(QUIET);
  });
});

describe('PhoneRow: the quiet line’s figures are values; its separators are quiet', () => {
  it('reads the quiet line’s figures in the reading ink, a step under the body by size', () => {
    render(
      <PhoneRow
        testId="row"
        lines={[
          { key: 'identity', tone: 'head', left: '#20,895,267' },
          {
            key: 'figures',
            tone: 'quiet',
            left: <Dotted parts={['3,344 B', '0 cycles']} />,
            right: '2026-11-17',
          },
        ]}
      />
    );
    const row = screen.getByTestId('row');
    const quiet = row.querySelector('[data-line="figures"]')!;
    expect(quiet).toHaveClass('text-meta');
    expect(inkOf(within(row).getByText('3,344 B'))).toBe(READING);
    expect(inkOf(within(row).getByText('0 cycles'))).toBe(READING);
    expect(inkOf(within(row).getByText('2026-11-17'))).toBe(READING);
    for (const dot of Array.from(row.querySelectorAll('[data-dotted-separator]')))
      expect(inkOf(dot)).toBe(QUIET);
    expect(inkOf(within(row).getByText('#20,895,267'))).toBe(BRIGHT);
  });
});

describe('HexCode and Hash: an id is a value, never quiet', () => {
  const HASH = `0x${'12'.repeat(32)}`;

  it('has no quiet variant: a hash reads in the reading ink or brighter', () => {
    // asked for one anyway (the type is the caller's first refusal), it writes no quiet ink
    const quiet = { variant: 'dim' } as Record<string, string>;
    render(<HexCode value={HASH} truncate={false} {...quiet} />);
    expect(inkOf(screen.getByText(HASH))).not.toBe(QUIET);
    cleanup();
    render(<Hash hash={HASH} truncate={false} {...quiet} />);
    expect(inkOf(screen.getByText(HASH))).not.toBe(QUIET);
  });
});

describe('TintedJson: values read, punctuation is a separator', () => {
  it('reads keys and values in their inks and keeps the braces and commas quiet', () => {
    const text = JSON.stringify({ ver: 0, ok: true }, null, 2);
    const { container } = render(<TintedJson text={text} />);
    const pre = container.querySelector('pre')!;
    for (const value of Array.from(pre.querySelectorAll('[data-json="value"]')))
      expect(inkOf(value)).toBe('var(--c-text)');
    for (const key of Array.from(pre.querySelectorAll('[data-json="key"]')))
      expect(inkOf(key)).toBe('var(--c-cell-lock)');
    // what is left is punctuation: braces, commas, colons, white space
    const rest = Array.from(pre.childNodes)
      .filter((n) => n.nodeType === Node.TEXT_NODE)
      .map((n) => n.textContent)
      .join('');
    expect(rest.replace(/[\s{}[\],:]/g, '')).toBe('');
    expect(inkOf(pre)).toBe(QUIET);
  });
});

describe('the id line under an h1: the full id is a value', () => {
  it('reads a name plate’s id line in the reading ink; its kind word is a label, quiet', () => {
    render(
      <SealedHead
        id={SEAL_IDS.cell}
        kind="Cell"
        identity={<span data-testid="outpoint">{SEAL_IDS.cell.tx}:1</span>}
      >
        <h1>0xabcdef01…234567:1</h1>
      </SealedHead>
    );
    expect(inkOf(screen.getByTestId('outpoint'))).toBe(READING);
    expect(inkOf(screen.getByText('Cell'))).toBe(QUIET);
  });

  it('reads an unsealed head’s identity line (an id or the page’s subtitle) in the reading ink', () => {
    render(
      <DetailLayout title="Inventory" kind="index" identity="Everything people hold on CKB">
        <p>content</p>
      </DetailLayout>
    );
    expect(inkOf(screen.getByText('Everything people hold on CKB'))).toBe(READING);
  });
});

describe('ListTable: an empty list says so in a sentence', () => {
  it('reads the empty sentence in the reading ink', () => {
    render(
      <ListTable
        label="Blocks"
        columns={[{ key: 'n', label: 'Height', render: (r: { n: number }) => r.n, minWidth: 80 }]}
        rows={[]}
        rowKey={(r) => String(r.n)}
        empty="No blocks in this range."
        mobileRow={(r) => r.n}
      />
    );
    expect(inkOf(screen.getByText('No blocks in this range.'))).toBe(READING);
  });
});

describe('CapacityPartition and CellBar: their numbers read, their labels and units are quiet', () => {
  it('reads a part’s share, a percentage, in the reading ink; its name is a label', () => {
    render(
      <CapacityPartition
        subject="cell"
        total="20000000000"
        occupied="6100000000"
        free="13900000000"
      />
    );
    expect(inkOf(screen.getByText('30.50%'))).toBe(READING);
    expect(inkOf(screen.getByText('occupied'))).toBe(QUIET);
  });

  it('reads a part’s bytes in the reading ink; the unit after them is quiet', () => {
    const { container } = render(
      <CellBar subject="cell" anatomy={{ lock: '53', type: '0', data: '12' }} />
    );
    const lock = container.querySelector('[data-part-legend="lock"]')!;
    expect(inkOfText(lock, '53')).toBe(READING);
    expect(inkOfText(lock, 'B')).toBe(QUIET);
  });
});

describe('ParticipantRefView: a party is a link, its roles and "unresolved" are labels', () => {
  const LOCK = `0x${'57'.repeat(32)}`;
  const party = (p: { address?: string; lockHash?: string; lockHashPrefix?: string }) => ({
    address: p.address ?? null,
    lockHash: p.lockHash ?? null,
    lockHashPrefix: p.lockHashPrefix ?? null,
    roles: ['owner'],
  });

  it('reads a party linked by its lock hash in the reading ink, in a row or alone', () => {
    const { unmount } = render(
      <ParticipantRefView size="row" compact participant={party({ lockHash: LOCK })} />
    );
    expect(inkOf(screen.getByRole('link'))).toBe(READING);
    expect(inkOf(screen.getByText('owner'))).toBe(QUIET);
    unmount();
    render(<ParticipantRefView size="row" participant={party({ lockHash: LOCK })} />);
    expect(inkOf(screen.getByRole('link'))).toBe(READING);
  });

  it('reads a compact link to an address that is not a CKB address in the reading ink', () => {
    render(<ParticipantRefView size="row" compact participant={party({ address: 'xyz1qqq' })} />);
    expect(inkOf(screen.getByRole('link'))).toBe(READING);
  });

  it('reads a party known by its prefix alone, a value, in the reading ink; "unresolved" is a label', () => {
    render(
      <ParticipantRefView
        size="row"
        participant={party({ lockHashPrefix: `0x${'ab'.repeat(20)}` })}
      />
    );
    expect(inkOf(screen.getByTitle(`0x${'ab'.repeat(20)}`))).toBe(READING);
    expect(inkOf(screen.getByText('unresolved'))).toBe(QUIET);
  });

  it('keeps the page’s own party, written on every row, quiet (as a repeated unit)', () => {
    render(
      <ParticipantRefView
        size="row"
        compact
        linked={false}
        participant={party({ lockHash: LOCK })}
      />
    );
    expect(inkOf(screen.getByTitle(LOCK))).toBe(QUIET);
  });
});

describe('ItemDeltaFact: an object or an identity arrived or departed, a value', () => {
  it('reads "arrived" and "departed" in the reading ink', () => {
    render(
      <>
        <ItemDeltaFact
          size="row"
          item={{ kind: 'object', objectId: `0x${'cd'.repeat(32)}`, delta: 1 }}
        />
        <ItemDeltaFact
          size="row"
          item={{
            kind: 'identity',
            standard: 'dotbit',
            identityId: `0x${'ef'.repeat(32)}`,
            delta: -1,
          }}
        />
      </>
    );
    expect(inkOf(screen.getByText('arrived'))).toBe(READING);
    expect(inkOf(screen.getByText('departed'))).toBe(READING);
  });
});

describe('ScriptNames: its chain tiles and its last sentence explain', () => {
  it('reads the tiles’ words and the closing sentence in the reading ink; the tile titles stay bright', () => {
    render(<ScriptNames lockHash={`0x${'12'.repeat(32)}`} address="ckb1qyq" />);
    expect(inkOf(screen.getByText('blake2b-256 of it; what the indexes are keyed by'))).toBe(
      READING
    );
    expect(inkOf(screen.getByText(/^Change one byte of args and both names change/))).toBe(READING);
    expect(inkOf(screen.getAllByText('lock hash')[0])).toBe(BRIGHT);
    expect(inkOf(screen.getByText('both derived, neither stored'))).toBe(QUIET);
  });
});

describe('CursorPagination: the counts are numbers', () => {
  it('reads the range sentence and the page in the reading ink', () => {
    render(
      <CursorPagination
        hasMore
        hasPrevious
        onNext={vi.fn()}
        onPrevious={vi.fn()}
        total={100}
        totalLabel="transactions"
        page={2}
        pageSize={20}
        currentCount={20}
      />
    );
    expect(inkOf(screen.getByText(/^Showing 21–40 of 100 transactions/))).toBe(READING);
    expect(inkOf(screen.getByText('Page 2 of 5'))).toBe(READING);
  });

  it('reads a list’s own range sentence in the reading ink', () => {
    render(
      <CursorPagination
        hasMore
        hasPrevious={false}
        onNext={vi.fn()}
        onPrevious={vi.fn()}
        summary="50 newest of 20,876,569"
      />
    );
    expect(inkOf(screen.getByText('50 newest of 20,876,569'))).toBe(READING);
  });
});

vi.mock('@/src/navigation', () => ({
  usePathname: () => '/',
  useSearchAndHash: () => '',
  useRouter: () => ({ push: vi.fn() }),
}));
vi.mock('@/components/command-palette', () => ({ CommandPalette: () => null }));
vi.mock('@/components/search-bar', () => ({ SearchBar: () => null }));
vi.mock('@/components/stats-bar', () => ({ VitalsStrip: () => null }));

describe('the phone menu: a view under a section is a link', () => {
  it('reads every submenu link in the reading ink; a section with no page is its label, quiet', () => {
    const queryClient = new QueryClient();
    render(
      <QueryClientProvider client={queryClient}>
        <Header />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole('button', { name: 'Toggle menu' }));
    const phoneNav = screen
      .getAllByRole('navigation')
      .find((n) => n.getAttribute('aria-label') !== 'Main')!;
    for (const name of ['Transactions', 'Blocks', 'Forks', 'Tokens', 'Objects', 'Identities'])
      expect(inkOf(within(phoneNav).getByRole('link', { name })), name).toBe(READING);
    expect(inkOf(within(phoneNav).getByText('Inventory'))).toBe(QUIET);
  });
});
