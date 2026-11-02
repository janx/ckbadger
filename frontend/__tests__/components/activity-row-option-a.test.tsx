// Tables round 2026-11-02, lane F2 (T7, ruling D1 · option A, Feed.dc.html column A): every
// participant stays in the row, one 18 px line each — address · lock-kind tag · signed CKB · each
// asset delta · occupied — under one headline line that links to the transaction and carries the
// tx-level facts inline; block · age on the right, the tx hash in the row's hover. jsdom does not
// lay out, so these assert the structure and classes; the survey gate measures the heights.
import { fireEvent, render, within } from '@/__tests__/utils/test-utils';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import { ActivityRow, type ActivityRowData } from '@/components/activity/activity-row';
import type { ItemDelta, ParticipantInfo } from '@/lib/api';

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
  it('links the headline to the transaction and puts block · age on the right, the hash in hover', () => {
    const { container } = render(<ActivityRow activity={scriptCall} />);
    const article = container.querySelector('article') as HTMLElement;
    expect(article).toHaveAttribute('title', TX);
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
    expect(evidence).toHaveTextContent(/^#20,904,081 · \d+s ago$/);
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

  it('keeps a caller’s own evidence (block pages) and then leaves the headline unlinked', () => {
    const { container } = render(
      <ActivityRow activity={transfer} evidence={<>#3 · {TX.slice(0, 10)}</>} />
    );
    const headline = container.querySelector('[data-role="headline"]') as HTMLElement;
    expect(headline.tagName).toBe('SPAN');
    expect(container.querySelector('[data-role="evidence"]')).toHaveTextContent('#3 · 0x8a39e8e3');
  });
});
