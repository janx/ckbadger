// Tables round 2026-11-02, lane F2 (T7, ruling D1 · option A, Feed.dc.html column A): every
// participant stays in the row, one 18 px line each — address · lock-kind tag · signed CKB · each
// asset delta · occupied — under one headline line that links to the transaction and carries the
// tx-level facts inline; block · date on the right (consistency C3: the absolute UTC+8 date, was the
// age), the tx hash and the age in the row's hover. jsdom does not lay out, so these assert the
// structure and classes; the survey gate measures the heights.
import { fireEvent, render, within } from '@/__tests__/utils/test-utils';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import { ActivityRow, type ActivityRowData } from '@/components/activity/activity-row';
import type { ItemDelta, ParticipantInfo } from '@/lib/api';
import { formatUtc8Date } from '@/lib/utils';

const MINUS = '−';
const TX = '0x8a39e8e3' + 'ab'.repeat(26) + 'd67305';
const CKB = (whole: string) => `${whole}00000000`;

function party(tail: string, over: Partial<ParticipantInfo> = {}): ParticipantInfo {
  return {
    address: `ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq${tail}`,
    lockHash: `0x${tail.padStart(64, '0')}`,
    lockHashPrefix: null,
    lockKind: 'Default Lock',
    roles: [],
    tags: 0,
    ckbDelta: '0',
    usedDelta: '0',
    itemDeltas: [],
    ...over,
  };
}

function token(symbol: string, delta: string, hash: string): ItemDelta {
  return { kind: 'token', typeScriptHash: hash, delta, symbol, decimals: 8 } as ItemDelta;
}

function row(
  participants: ParticipantInfo[],
  over: Partial<ActivityRowData> = {}
): ActivityRowData {
  return {
    txHash: TX,
    blockNumber: 20904081,
    timestamp: String(Date.now() - 13_000),
    isCellbase: false,
    protocolActions: [],
    typeCalls: [],
    lockCalls: [],
    participants,
    ...over,
  };
}

/** The board's 1-participant row: a .bit script call. */
const scriptCall = row([party('r43nr7', { ckbDelta: '-30000' })], {
  typeCalls: [
    {
      typeCodeHash: '0x' + '3a'.repeat(32),
      typeHashType: 'type',
      typeArgs: '0x02',
      scriptHash: '0x' + '22'.repeat(32),
      scriptName: '.bit Time Index State',
    },
    {
      typeCodeHash: '0x' + '9e'.repeat(32),
      typeHashType: 'type',
      typeArgs: '0x02',
      scriptHash: '0x' + '91'.repeat(32),
      scriptName: '.bit Time Info',
    },
  ],
});

/** The board's 2-participant row: a CKB transfer, occupied on the receiver. */
const transfer = row([
  party('qkp5em', { ckbDelta: '-57249230668' }),
  party('2jzelt', { ckbDelta: '57244230668', usedDelta: CKB('61') }),
]);

/** The board's 5-participant row: UTXOSwap remove liquidity settled. */
const RGBPP = '0x' + '77'.repeat(32);
const LP = '0x' + '88'.repeat(32);
const swap = row(
  [
    party('9rvpf8', {
      lockKind: 'UTXOSwap Proxy Lock',
      ckbDelta: '-276411026713',
      itemDeltas: [token('RGB++', '-1300019363874', RGBPP)],
    }),
    party('s7p9j4', {
      lockKind: 'UTXOSwap Intent Lock',
      ckbDelta: `-${CKB('288')}`,
      usedDelta: `-${CKB('231')}`,
      itemDeltas: [token('LP-UTXOSwap V1', '-591345087427', LP)],
    }),
    party('68r5ue', { ckbDelta: '-9030' }),
    party('d8csr7', {
      lockKind: 'JoyID',
      ckbDelta: '305211026713',
      usedDelta: CKB('207'),
      itemDeltas: [token('RGB++', '1300019363874', RGBPP)],
    }),
    party('w8xxul', { lockKind: null }),
  ],
  {
    blockNumber: 20724160,
    protocolActions: [
      {
        protocol: 'utxoswap',
        protocolName: 'UTXOSwap',
        action: 'remove_liquidity_settled',
        metadata: {},
      },
    ],
  }
);

const lines = (c: HTMLElement) => [...c.querySelectorAll<HTMLElement>('[data-role="participant"]')];
const head = (c: HTMLElement) => c.querySelector('[data-role="row-head"]') as HTMLElement;

describe('option A: the headline line', () => {
  it('links the headline to the transaction and puts block · date on the right, the hash and age in hover', () => {
    const { container } = render(<ActivityRow activity={scriptCall} />);
    const article = container.querySelector('article') as HTMLElement;
    // Consistency C3: the age left the visible row for its hover, beside the hash (was: TX alone).
    expect(article).toHaveAttribute(
      'title',
      expect.stringMatching(new RegExp(`^${TX} · \\d+s ago$`))
    );
    const headline = within(article).getByRole('link', { name: 'Script call' });
    expect(headline).toHaveAttribute('href', `/mainnet/tx/${TX}`);
    expect(headline).toHaveAttribute('data-role', 'headline');
    expect(headline).toHaveClass('text-body', 'text-text-bright');
    const evidence = article.querySelector('[data-role="evidence"]') as HTMLElement;
    expect(evidence).toHaveClass('text-meta', 'text-text-dim');
    expect(within(evidence).getByRole('link', { name: '#20,904,081' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/20904081'
    );
    // Consistency C3: the absolute UTC+8 date (was "13s ago"); the age is the date's own hover.
    const date = formatUtc8Date(scriptCall.timestamp!);
    expect(evidence).toHaveTextContent(new RegExp(`^#20,904,081 · ${date}$`));
    expect(evidence).not.toHaveTextContent('ago');
    expect(within(evidence).getByText(date)).toHaveAttribute(
      'title',
      expect.stringMatching(/^\d+s ago$/)
    );
    // The hash is in no visible text of the row.
    expect(article).not.toHaveTextContent(/0x8a39e8e3/);
    expect(head(container)).toHaveClass('leading-[18px]');
  });

  it("carries a script call's type names inline, dim, the args in each name's hover", () => {
    const { container } = render(<ActivityRow activity={scriptCall} />);
    const line = head(container);
    expect(line).toHaveTextContent(
      /^Script call · \.bit Time Index State · \.bit Time Info#20,904,081/
    );
    const types = line.querySelector('[data-role="detail"]') as HTMLElement;
    expect(types).toHaveClass('text-small', 'text-text-dim');
    expect(within(types).getByRole('link', { name: '.bit Time Index State' })).toHaveAttribute(
      'title',
      '.bit Time Index State(0x02)'
    );
    // No separate "type" line under the headline any more.
    expect(container.querySelectorAll('[data-role="detail"]')).toHaveLength(1);
    expect(container).not.toHaveTextContent(/\(0x02\)/);
  });

  it('carries a protocol action’s facts inline after the headline', () => {
    const { container } = render(<ActivityRow activity={feedSamples.daoWithdrawRequest} />);
    const line = head(container);
    expect(within(line).getByRole('link', { name: /DAO Withdraw Request$/ })).toBeInTheDocument();
    expect(line.querySelector('[data-role="detail"]')).toHaveTextContent(
      '26,050.00000000 CKB · deposited at #19,695,550'
    );
  });

  it('names a lock call inline after the type calls, labelled, args in its hover', () => {
    const { container } = render(<ActivityRow activity={feedSamples.objectTransfer} />);
    const details = [...head(container).querySelectorAll('[data-role="detail"]')].map(
      (e) => e.textContent
    );
    expect(details).toEqual(['CKBFS', 'lock Nervape Shadow Lock']);
    expect(
      within(head(container)).getByRole('link', { name: 'Nervape Shadow Lock' })
    ).toHaveAttribute('title', `Nervape Shadow Lock(0x0131da${'00'.repeat(28)}5d74)`);
  });

  it('says a script called more than once by its name once, with the count, every call in its hover', () => {
    // Without the args on the row, two calls of one lock read ".bit Lock · .bit Lock"; one name
    // and its count say the same, and the hover keeps each call whole.
    const bitLock = (args: string) => ({
      lockCodeHash: '0x' + '93'.repeat(32),
      lockHashType: 'type',
      lockArgs: args,
      scriptHash: '0x' + args.slice(2).padStart(64, '0'),
      scriptName: '.bit Lock',
      decoded: null,
      decodeError: null,
    });
    const { container } = render(
      <ActivityRow
        activity={row([party('r43nr7')], {
          lockCalls: [bitLock('0x05' + 'aa'.repeat(20)), bitLock('0x05' + 'bb'.repeat(20))],
        })}
      />
    );
    const detail = head(container).querySelector('[data-role="detail"]') as HTMLElement;
    expect(detail).toHaveTextContent(/^lock \.bit Lock ×2$/);
    expect(within(detail).getAllByRole('link')).toHaveLength(1);
    expect(within(detail).getByRole('link', { name: '.bit Lock' })).toHaveAttribute(
      'title',
      `.bit Lock(0x05${'aa'.repeat(20)})\n.bit Lock(0x05${'bb'.repeat(20)})`
    );
  });

  it('keeps a caller’s own evidence (block pages) and then leaves the headline unlinked', () => {
    const { container } = render(
      <ActivityRow activity={transfer} evidence={<>#3 · {TX.slice(0, 10)}</>} />
    );
    const headline = container.querySelector('[data-role="headline"]') as HTMLElement;
    expect(headline.tagName).toBe('SPAN');
    expect(container.querySelector('[data-role="evidence"]')).toHaveTextContent('#3 · 0x8a39e8e3');
  });
});

describe('option A: every participant, one line each (D1)', () => {
  it.each([
    ['1', scriptCall],
    ['2', transfer],
    ['5', swap],
  ])('draws all %s participants, one line each, every address shown', (_n, activity) => {
    const { container } = render(<ActivityRow activity={activity} />);
    const shown = lines(container);
    expect(shown).toHaveLength(activity.participants.length);
    expect(container.querySelectorAll('[data-party]')).toHaveLength(activity.participants.length);
    for (const p of activity.participants)
      expect(container).toHaveTextContent(`${p.address!.slice(0, 8)}...${p.address!.slice(-6)}`);
  });

  it('puts CKB, every asset delta and occupied on the participant’s own line, in that order', () => {
    const { container } = render(<ActivityRow activity={swap} />);
    const joy = lines(container).find((l) => l.textContent?.includes('d8csr7')) as HTMLElement;
    const order = [...joy.children].map((e) => e.getAttribute('data-role'));
    expect(order).toEqual(['party', 'lock-kind', 'ckb', 'item', 'occupied']);
    expect(joy.querySelector('[data-role="party"]')).toHaveAttribute('data-party');
    expect(within(joy).getByText('JoyID')).toHaveAttribute('data-role', 'lock-kind');
    expect(joy.querySelector('[data-ckb-amount]')).toHaveTextContent('+3,052.11026713 CKB');
    // An asset delta is a signed delta: jade / rouge by sign like the CKB, its symbol dim.
    const rgbpp = within(joy).getByRole('link', { name: '+13,000.19363874 RGB++' });
    expect(rgbpp).toHaveClass('text-positive');
    expect(within(rgbpp).getByText('RGB++')).toHaveClass('text-text-dim');
    expect(joy.querySelector('[data-role="occupied"]')).toHaveTextContent('occupied +207');
    // No second line per asset: the intent lock's LP delta is on its own participant line.
    const intent = lines(container).find((l) => l.textContent?.includes('s7p9j4')) as HTMLElement;
    const lp = within(intent).getByRole('link', { name: `${MINUS}5,913.45087427 LP-UTXOSwap V1` });
    expect(lp).toHaveClass('text-negative');
    expect(within(lp).getByText('LP-UTXOSwap V1')).toHaveClass('text-text-dim');
    expect(intent.querySelector('[data-role="occupied"]')).toHaveTextContent(
      `occupied ${MINUS}231`
    );
    // A party with no registered lock has no tag, and its zero is still its fact.
    const zero = lines(container).find((l) => l.textContent?.includes('w8xxul')) as HTMLElement;
    expect(zero.querySelector('[data-role="lock-kind"]')).toBeNull();
    // Consistency plan C4: a ghost "0 CKB" (was "0.00000000 CKB").
    expect(zero.querySelector('[data-ckb-amount]')).toHaveTextContent(/^0 CKB$/);
  });

  // Material pass v2 (move 5): the lock kind is dim small caps after the address, not a chip —
  // boxes stay on controls.
  it('draws one 18 px wrapping line per participant, indented 18 px (hanging when it wraps), with the lock kind in small caps', () => {
    const { container } = render(<ActivityRow activity={transfer} />);
    for (const line of lines(container)) {
      expect(line).toHaveClass('flex', 'flex-wrap', 'items-center', 'text-small');
      expect(line).toHaveClass('leading-[18px]');
      // 18 px indent for the address; a line that wraps (phone) hangs 12 px further in, so its
      // continuation never reads as the next participant.
      expect(line).toHaveClass('pl-[30px]');
      expect(line.querySelector('[data-role="party"]')).toHaveClass('-ml-3');
      expect([...line.classList].filter((c) => /^(sm|md|lg):/.test(c))).toEqual([]);
      expect(line.querySelector('[data-role="lock-kind"]')).toHaveClass(
        'text-text-dim',
        'small-caps',
        'lowercase',
        'whitespace-nowrap'
      );
      expect(line.querySelector('[data-role="lock-kind"]')).not.toHaveClass(
        'border',
        'rounded-[3px]',
        'text-meta'
      );
    }
    // Occupied last, 11 px, in the board's ghost tone (≈ #4a5268), readable — not the 1.7:1 token.
    const occupied = container.querySelector('[data-role="occupied"]');
    expect(occupied).toHaveClass('text-meta', 'text-text-dim/60', 'whitespace-nowrap');
    expect(occupied).not.toHaveClass('text-text-ghost');
    // The three/four/five-track participant grid is gone.
    const box = container.querySelector('[data-role="participants"]') as HTMLElement;
    expect(box.className).not.toMatch(/grid/);
    expect(box).toHaveClass('flex', 'flex-col');
  });

  it('keeps the over-six fold: the five largest and "+N participants" that opens the rest', () => {
    const { container } = render(<ActivityRow activity={feedSamples.eightParties} />);
    expect(lines(container)).toHaveLength(5);
    fireEvent.click(within(container).getByRole('button', { name: '+3 participants' }));
    expect(lines(container)).toHaveLength(8);
  });

  it('pads the row 8 px and keeps the fresh tint', () => {
    const { container } = render(<ActivityRow activity={transfer} fresh />);
    const article = container.querySelector('article') as HTMLElement;
    expect(article).toHaveClass('py-2', 'bg-jade/5');
    expect(article).not.toHaveClass('py-3');
  });
});

// Lane F follow-up 2 (2026-11-03, user: "展开了还是很丑"): a dCKB batch withdraw carries 51 DAO
// withdraw requests, one per deposit. T7's "one thing per line": the head names the action once
// with its count and exact total, nothing of the list inline; below it a folded list control
// "51 deposits ↓" opens ONE DEPOSIT PER 18 px LINE (amount decimal-aligned · deposited at #block
// as a link · other facts), "fewer ↑" folds it back. 2–6 actions keep the inline form while the
// rendered head stays one line (headline, facts and evidence on one 18 px line), else the list.
// Mixed actions name each action per line. Every action stays reachable.
// (Was, follow-up 1: five lines inline and "+46 more deposits" opening the rest inline.)
describe('option A: a run of protocol actions becomes a list, one action per line', () => {
  const withdraw = (n: number) => ({
    protocol: 'dao',
    protocolName: 'Nervos DAO',
    action: 'withdraw_request',
    metadata: { capacity: `${(n + 1) * 100}00000000`, depositBlock: String(6_000_000 + n) },
  });
  const deposit = (n: number) => ({
    protocol: 'dao',
    protocolName: 'Nervos DAO',
    action: 'deposit',
    metadata: { capacity: `${(n + 1) * 1000}00000000` },
  });
  const batch = row([party('r43nr7', { ckbDelta: '-1000' })], {
    protocolActions: Array.from({ length: 51 }, (_, n) => withdraw(n)),
  });
  const headMain = (c: HTMLElement) =>
    c.querySelector('[data-role="row-head"] [data-role="head-main"]') as HTMLElement;
  const listRows = (c: HTMLElement) => [
    ...c.querySelectorAll<HTMLElement>('[data-role="protocol-line"]'),
  ];
  const toggle = (c: HTMLElement) =>
    c.querySelector('[data-role="protocol-list-toggle"]') as HTMLElement | null;

  it('heads 51 deposits as the action once, its count and exact total; nothing of the list inline', () => {
    const { container } = render(<ActivityRow activity={batch} />);
    // Σ (n + 1) × 100 CKB for n = 0…50 = 132,600 CKB, exact.
    expect(headMain(container)).toHaveTextContent(
      /^DAO Withdraw Request · 51 deposits · 132,600\.00000000 CKB$/
    );
    expect(head(container)).not.toHaveTextContent('deposited at');
    expect(within(head(container)).queryByRole('button')).toBeNull();
    const total = head(container).querySelector('[data-role="protocol-total"]') as HTMLElement;
    expect(total).toHaveAttribute('title', '132,600.00000000 CKB, the sum of 51 deposits');
  });

  it('folds the list by default and opens one deposit per 18 px line, folding back with "fewer ↑"', () => {
    const { container } = render(<ActivityRow activity={batch} />);
    expect(listRows(container)).toHaveLength(0);
    const control = toggle(container)!;
    expect(control).toHaveTextContent('51 deposits ↓');
    expect(control).toHaveAttribute('aria-expanded', 'false');
    // The list sits under the head, with the participants, never inside the head line.
    expect(head(container).contains(control)).toBe(false);
    fireEvent.click(control);
    const rows = listRows(container);
    expect(rows).toHaveLength(51);
    expect(toggle(container)).toHaveTextContent('fewer ↑');
    expect(toggle(container)).toHaveAttribute('aria-expanded', 'true');
    expect(rows[0]).toHaveTextContent(/^100\.00000000 CKB · deposited at #6,000,000$/);
    expect(rows[50]).toHaveTextContent(/^5,100\.00000000 CKB · deposited at #6,000,050$/);
    for (const [n, r] of rows.entries()) {
      expect(r).toHaveClass('leading-[18px]', 'text-small');
      const block = within(r).getByRole('link', {
        name: `#${(6_000_000 + n).toLocaleString('en-US')}`,
      });
      expect(block).toHaveAttribute('href', `/mainnet/blocks/${6_000_000 + n}`);
      // One action of one kind: no line repeats its name.
      expect(r).not.toHaveTextContent('DAO Withdraw Request');
    }
    // Decimal-aligned: every amount sits in one right-aligned, tabular column of one width.
    const amounts = rows.map(
      (r) => r.querySelector('[data-role="protocol-amount"]') as HTMLElement
    );
    expect(new Set(amounts.map((a) => a.style.minWidth))).toEqual(new Set(['18ch']));
    for (const a of amounts) expect(a).toHaveClass('text-right', 'tabular-nums', 'inline-block');
    fireEvent.click(toggle(container)!);
    expect(listRows(container)).toHaveLength(0);
    expect(toggle(container)).toHaveTextContent('51 deposits ↓');
  });

  it('names each action on its line when the run is mixed, and heads the count without a total', () => {
    const mixed = row([party('r43nr7')], {
      protocolActions: [
        withdraw(0),
        withdraw(1),
        withdraw(2),
        deposit(0),
        deposit(1),
        deposit(2),
        deposit(3),
      ],
    });
    const { container } = render(<ActivityRow activity={mixed} />);
    expect(headMain(container)).toHaveTextContent(/^DAO Withdraw Request · 7 actions$/);
    fireEvent.click(toggle(container)!);
    const rows = listRows(container).map((r) => r.textContent);
    expect(rows).toHaveLength(7);
    expect(rows[0]).toBe('DAO Withdraw Request · 100.00000000 CKB · deposited at #6,000,000');
    expect(rows[3]).toBe('DAO Deposit · 1,000.00000000 CKB locked');
    expect(rows[6]).toBe('DAO Deposit · 4,000.00000000 CKB locked');
  });

  it('keeps two actions inline while the head stays one line at 1440', () => {
    const two = row([party('r43nr7')], { protocolActions: [deposit(0), deposit(1)] });
    const { container } = render(<ActivityRow activity={two} />);
    expect(headMain(container)).toHaveTextContent(
      /^DAO Deposit · 1,000\.00000000 CKB locked · DAO Deposit · 2,000\.00000000 CKB locked$/
    );
    expect(toggle(container)).toBeNull();
    expect(listRows(container)).toHaveLength(0);
  });

  // The rule is the rendered line itself (jsdom lays nothing out: these stub the head's boxes).
  const layOut = (wraps: { main?: boolean; evidence?: boolean }) =>
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
      this: HTMLElement
    ) {
      const role = this.getAttribute('data-role');
      const height = role === 'head-main' && wraps.main ? 36 : 18;
      const top = role === 'evidence' && wraps.evidence ? 18 : 0;
      return {
        x: 0,
        y: top,
        top,
        left: 0,
        right: 0,
        bottom: top + height,
        width: 0,
        height,
        toJSON: () => ({}),
      } as DOMRect;
    });

  it('keeps a ~145-character head inline when it lays out on one line (live: two withdraw requests)', () => {
    // Regression (dCKB 0xfe6f0cd0): a character budget of 130 listed a head that fit one line.
    layOut({});
    const two = row([party('r43nr7')], {
      protocolActions: [
        { ...withdraw(0), metadata: { capacity: '54500000000000', depositBlock: '6012189' } },
        { ...withdraw(1), metadata: { capacity: '54512000000000', depositBlock: '6193839' } },
      ],
    });
    const { container } = render(<ActivityRow activity={two} />);
    expect(headMain(container).textContent!.length).toBeGreaterThan(130);
    expect(toggle(container)).toBeNull();
    expect(headMain(container)).toHaveTextContent(
      /^DAO Withdraw Request · 545,000\.00000000 CKB · deposited at #6,012,189 · DAO Withdraw Request · /
    );
    vi.restoreAllMocks();
  });

  it('lists three actions whose inline head wraps (was: three runs inline)', () => {
    layOut({ main: true });
    const three = row([party('r43nr7')], {
      protocolActions: [withdraw(0), withdraw(1), withdraw(2)],
    });
    const { container } = render(<ActivityRow activity={three} />);
    expect(headMain(container)).toHaveTextContent(
      /^DAO Withdraw Request · 3 deposits · 600\.00000000 CKB$/
    );
    fireEvent.click(toggle(container)!);
    expect(listRows(container).map((r) => r.textContent)).toEqual([
      '100.00000000 CKB · deposited at #6,000,000',
      '200.00000000 CKB · deposited at #6,000,001',
      '300.00000000 CKB · deposited at #6,000,002',
    ]);
    vi.restoreAllMocks();
  });

  it('lists a short run whose evidence the line can no longer hold', () => {
    layOut({ evidence: true });
    const two = row([party('r43nr7')], { protocolActions: [deposit(0), deposit(1)] });
    const { container } = render(<ActivityRow activity={two} />);
    expect(headMain(container)).toHaveTextContent(
      /^DAO Deposit · 2 deposits · 3,000\.00000000 CKB$/
    );
    expect(toggle(container)).toHaveTextContent('2 deposits ↓');
    vi.restoreAllMocks();
  });

  it('lists actions that read alike once with their count, as the inline head does (A12c)', () => {
    const run = row([party('r43nr7')], {
      protocolActions: [...Array.from({ length: 7 }, (_, n) => withdraw(n)), withdraw(6)],
    });
    const { container } = render(<ActivityRow activity={run} />);
    expect(headMain(container)).toHaveTextContent(/^DAO Withdraw Request · 8 deposits · /);
    fireEvent.click(toggle(container)!);
    const rows = listRows(container).map((r) => r.textContent);
    expect(rows).toHaveLength(7);
    expect(rows[6]).toBe('700.00000000 CKB · deposited at #6,000,006 ×2');
  });

  it('keeps one action inline, as before', () => {
    const { container } = render(
      <ActivityRow activity={row([party('r43nr7')], { protocolActions: [withdraw(0)] })} />
    );
    expect(headMain(container)).toHaveTextContent(
      /^DAO Withdraw Request · 100\.00000000 CKB · deposited at #6,000,000$/
    );
    expect(toggle(container)).toBeNull();
  });
});
