import { beforeEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '../msw/server';
import { render, screen, within } from '../utils/test-utils';
import { FiberChannelBoard, formatEpochDelay } from '@/components/fiber/fiber-channel-board';
import forceClosed from '../fixtures/fiber-force-closed.json';
import type { FiberChannelDetail, FiberChannelEvidence } from '@/lib/api';
import producerDetail from '../fixtures/fiber-open-detail.json';
import producerEvidence from '../fixtures/fiber-evidence-open.json';
import c07e from '../fixtures/fiber-channel-c07e.json';

vi.mock('@/components/fiber/fiber-channel-step', () => ({
  FiberChannelStep: (props: {
    n: number;
    title: string;
    txHash: string;
    epoch?: string;
    consumes?: { txHash: string; index: number; role: string };
    carries?: { index: number; role: string };
    box: { head: string; lines: React.ReactNode[] };
  }) => (
    <div
      data-testid={`step-${props.n}`}
      data-epoch={props.epoch ?? ''}
      data-consumes={props.consumes ? `${props.consumes.txHash}:${props.consumes.index}` : ''}
      data-carries={props.carries ? String(props.carries.index) : ''}
    >
      {props.n} · {props.title} {props.txHash} | {props.box.head}
      {props.box.lines.map((line, i) => (
        <span key={i}> / {line}</span>
      ))}
    </div>
  ),
}));

const channel = c07e.channel as FiberChannelDetail;
const evidence = c07e.evidence as FiberChannelEvidence;

const waitingEvidence: FiberChannelEvidence = {
  channelId: forceClosed.channelId,
  funding: {
    txHash: forceClosed.fundingTxHash,
    outputIndex: 0,
    blockNumber: 10,
    capacity: forceClosed.capacity,
    lockCodeHash: `0x${'11'.repeat(32)}`,
    lockHashType: 'type',
    lockArgs: `0x${'22'.repeat(20)}`,
    isLive: false,
    consumedByTx: forceClosed.closeTxHash,
  },
  currentCommitment: {
    txHash: `0x${'55'.repeat(32)}`,
    outputIndex: 1,
    blockNumber: 25,
    capacity: forceClosed.capacity,
    lockCodeHash: `0x${'33'.repeat(32)}`,
    lockHashType: 'type',
    lockArgs: `0x${'44'.repeat(57)}`,
    isLive: true,
    consumedByTx: null,
    pubkeyHash: `0x${'44'.repeat(20)}`,
    delayEpoch: forceClosed.delayEpoch!,
    delayEpochDecoded: forceClosed.delayEpochDecoded!,
    version: '9007199254740993',
    settlementHash: `0x${'66'.repeat(20)}`,
    settlementFlag: 1,
    lockFormat: 'full57',
    epochLock: 'subsequentSettlementNoReappliedDelay',
    readyEpoch: null,
    epochLockMetAtTip: null,
    tipEpoch: '120+1/3',
  },
  openingPartyFlows: [],
  stages: [
    {
      stage: 'currentCommitment',
      txHash: `0x${'55'.repeat(32)}`,
      blockNumber: 25,
      timestamp: '1700000200000',
      inputs: 1,
      outputs: 2,
      witnessCount: 1,
      epoch: '120+0/3',
      fee: '1000',
      channelInput: forceClosed.capacity,
      feeSource: 'channel_only',
      partyFlows: [],
    },
  ],
  settlementOutputs: [],
};

describe('FiberChannelBoard', () => {
  // tier 2 reads each stage's own transaction for its witness sizes
  beforeEach(() => {
    const txs = c07e.transactions as Record<string, Record<string, unknown>>;
    server.use(
      http.get('/api/:network/v1/transactions/:hash/detail', ({ params }) =>
        txs[String(params.hash)]
          ? HttpResponse.json(txs[String(params.hash)])
          : HttpResponse.json({ error: 'not_found' }, { status: 404 })
      )
    );
  });

  it('opens with a hero: title and state, the id, one short lede, the capacity as its display figure with the pair beside it (F2, R13)', () => {
    render(<FiberChannelBoard channel={channel} evidence={evidence} />);
    const hero = screen.getByRole('region', { name: 'Fiber channel' });
    expect(within(hero).getByRole('heading', { level: 1, name: 'Fiber channel' })).toHaveClass(
      'text-entity'
    );
    expect(within(hero).getByText('force-closed · settled')).toBeInTheDocument();
    expect(within(hero).getByText(channel.channelId)).toHaveClass('w-fit', 'max-w-full');
    expect(within(hero).getByText('capacity')).toHaveClass('w-fit');
    // R13: one lede, dim, ≤ 90 characters — the channel's story in one line
    const lede = within(hero).getByText(
      'Force-closed 8 d 3 h after it opened; settled after 6 epochs.'
    );
    expect(lede).toHaveClass('text-body', 'text-text-dim', 'max-w-[560px]');
    // one line in its 560 px (about 8.3 px a character at 13 px), as the DAO page's lede
    expect(lede.textContent!.length).toBeLessThanOrEqual(67);
    // the display figure: the channel's capacity, exact, bright (R4, R11)
    const capacity = within(hero).getByText(
      (_, el) => el?.getAttribute('data-role') === 'capacity'
    );
    expect(capacity).toHaveTextContent(/^1,062\.00000000 CKB$/);
    expect(capacity).toHaveClass('text-index', 'sm:text-display', 'text-text-bright');
    expect(capacity.querySelector('.text-hero')).toHaveTextContent('.00000000');
    // the pair on the figure's baseline row
    const pair = screen.getByTestId('channel-pair');
    expect(pair.parentElement).toHaveClass('flex', 'items-baseline', 'justify-between');
    expect(pair.parentElement!.contains(capacity)).toBe(true);
    expect(within(pair).getByRole('link', { name: 'ckb1qzda0c…ppedrr' })).toBeInTheDocument();
    expect(within(pair).getByRole('link', { name: 'ckb1qzda0c…5eyy6y' })).toBeInTheDocument();
    // no raw four-cell developer strip in tier 1 and no "0 d" durations
    expect(screen.queryByText('Funding cell capacity')).toBeNull();
    expect(screen.queryByText(/\b0 d\b/)).toBeNull();
  });

  it('moves the full story and the fee caveat to tier 2, every sentence kept (F2)', () => {
    const { container } = render(<FiberChannelBoard channel={channel} evidence={evidence} />);
    const tier1 = container.querySelector('[data-tier="1"]') as HTMLElement;
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    expect(container.querySelector('#evidence')!.contains(tier2)).toBe(true);
    const summary = within(tier2).getByTestId('channel-summary');
    // Material pass v2 leftovers: the story is set in the shared tier-2 Prose piece (.prose-tier2:
    // 14 px on a 1.7 line, 64 characters, dim), not by a body class on each paragraph.
    expect(summary.closest('[data-prose]')).toHaveClass('prose-tier2');
    expect(summary).not.toHaveClass('text-body', 'text-text-dim');
    expect(summary).toHaveTextContent(
      'ckb1qzda0c…ppedrr put 1,000.00000569 CKB net into the opening transaction and ckb1qzda0c…5eyy6y 62.00000351 CKB.'
    );
    expect(summary).toHaveTextContent(
      '8 d 3 h later the channel was closed without a joint signature: the commitment carried the last agreed split, and after a wait of 6 epochs the settlement paid it out.'
    );
    expect(tier2).toHaveTextContent(
      'The chain never sees a Fiber payment, only the split when the channel opens and the split when it closes. Both splits include fees, and the chain does not say which side paid them, so the amount that changed sides is not shown.'
    );
    expect(tier1).not.toHaveTextContent(/net into the opening transaction/);
    expect(tier1).not.toHaveTextContent(/the chain does not say which side paid them/);
    // the words share the right column with the witnesses, beside the locks (page height)
    const words = within(tier2).getByRole('heading', { name: 'The channel, in words' });
    const witnesses = within(tier2).getByRole('heading', { name: 'Witnesses and timing' });
    const locks = within(tier2).getByRole('heading', { name: 'The two locks' });
    const column = (h: HTMLElement) => h.closest('[data-column]');
    expect(column(words)).toBe(column(witnesses));
    expect(column(words)).not.toBe(column(locks));
    // (the tier-1 prose budget is checked on the page with the real step diagrams)
  });

  it('keeps the story in tier 2 while the evidence is still loading', () => {
    const { container } = render(<FiberChannelBoard channel={channel} />);
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    expect(within(tier2).getByTestId('channel-summary')).toHaveTextContent(
      /^8 d 3 h later the channel was closed without a joint signature/
    );
  });

  it('draws what each side put in at open as one split bar (FF-C3)', () => {
    const { container } = render(<FiberChannelBoard channel={channel} evidence={evidence} />);
    expect(screen.getByRole('heading', { name: 'What moved inside the channel' })).toHaveClass(
      'text-section'
    );
    const bar = container.querySelector('[data-split="left"]') as HTMLElement;
    expect(parseFloat(bar.style.width)).toBeGreaterThan(94);
    expect(
      screen.getByText(/^at open, 2025-03-06 · net input to the opening transaction/)
    ).toBeInTheDocument();
  });

  it('draws the split at the close beside the split at open, from the stages’ party flows (FF-C3)', () => {
    const { container } = render(<FiberChannelBoard channel={channel} evidence={evidence} />);
    const bars = container.querySelectorAll('[data-split="left"]');
    expect(bars).toHaveLength(2);
    const close = bars[1].closest('[data-split-bar]') as HTMLElement;
    expect(
      Array.from(close.querySelectorAll('[data-ckb-amount]')).map((el) => el.textContent)
    ).toEqual(['899.99998733 CKB', '161.99999545 CKB']);
    expect(close).toHaveTextContent(
      'at the close, 2025-03-15 · net received in the settlement transaction, payout and change alike'
    );
    // evidence first: the bars, then one caption under them (R13, R14)
    const section = screen.getByRole('heading', { name: 'What moved inside the channel' })
      .parentElement!.parentElement!;
    const caption = section.querySelector('p[data-role="caption"]')!;
    expect(caption).toHaveTextContent(
      'The chain sees no Fiber payment, only the split at the open and at the close.'
    );
    expect(caption).toHaveClass('text-body', 'max-w-[640px]');
    expect(caption.textContent!.length * 8).toBeLessThanOrEqual(640);
    expect(close.compareDocumentPosition(caption) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('dates every step by its epoch and says what each side received at the end (FF-C4)', () => {
    render(<FiberChannelBoard channel={channel} evidence={evidence} />);
    expect(screen.getByTestId('step-1')).toHaveAttribute('data-epoch', '11590+1185/1305');
    expect(screen.getByTestId('step-2')).toHaveAttribute('data-epoch', '11639+836/1434');
    expect(screen.getByTestId('step-3')).toHaveAttribute('data-epoch', '11645+1005/1710');
    expect(screen.getByTestId('step-3')).toHaveTextContent('ppedrr received 899.99998733 CKB net');
    expect(screen.getByTestId('step-3')).toHaveTextContent('5eyy6y received 161.99999545 CKB net');
  });

  it('times the wait in tier 2 from the stages’ epochs and says where each fee came from (FF-C5)', () => {
    render(<FiberChannelBoard channel={channel} evidence={evidence} />);
    expect(screen.getByText('the wait').nextElementSibling).toHaveTextContent(
      'commitment at epoch 11,639 · 836 / 1,434 · settled at epoch 11,645 · 1,005 / 1,710'
    );
    expect(screen.getByText('force close').nextElementSibling).toHaveTextContent(
      'fee 455 shannons, out of the channel'
    );
  });

  it('tells the chain story in three steps: timeline, then open → force close → settle (FF-C4)', () => {
    render(<FiberChannelBoard channel={channel} evidence={evidence} />);
    expect(screen.getByRole('heading', { name: 'On chain, in three steps' })).toBeInTheDocument();
    expect(
      screen.getByRole('img', { name: /^Opened, force-closed 8 d 3 h later/ })
    ).toBeInTheDocument();
    expect(screen.getByTestId('step-1')).toHaveTextContent(
      `1 · Open ${channel.fundingTxHash} | → step 2, 8 d 3 h later`
    );
    expect(screen.getByTestId('step-1')).toHaveAttribute('data-carries', '0');
    expect(screen.getByTestId('step-2')).toHaveTextContent(
      `2 · Force close ${channel.closeTxHash} | → step 3, after 6 epochs`
    );
    expect(screen.getByTestId('step-2')).toHaveAttribute(
      'data-consumes',
      `${channel.fundingTxHash}:0`
    );
    expect(screen.getByTestId('step-2')).toHaveAttribute('data-carries', '0');
    expect(screen.getByTestId('step-3')).toHaveTextContent(
      `3 · Settle ${channel.settlementTxHash} | Closed`
    );
    expect(screen.getByTestId('step-3')).toHaveAttribute(
      'data-consumes',
      `${channel.closeTxHash}:0`
    );
  });

  it('keeps the lock evidence in tier 2: 96 px boundary, quiet 13 px headings (FF-C5, FF-C8, R6)', () => {
    const { container } = render(<FiberChannelBoard channel={channel} evidence={evidence} />);
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    expect(tier2).toHaveClass('mt-24', 'border-t', 'border-base-border');
    expect(tier2.parentElement!.className).not.toMatch(/space-y-/);
    for (const name of ['The two locks', 'Witnesses and timing']) {
      const heading = screen.getByRole('heading', { name });
      expect(tier2).toContainElement(heading);
      expect(heading).toHaveClass('text-body', 'text-text');
    }
    expect(screen.getByText('Fiber Funding Lock · args, 20 bytes')).toBeInTheDocument();
    expect(
      screen.getByText('blake160 of the two parties’ aggregated public key')
    ).toBeInTheDocument();
    // final outputs name the party by address, not a lock hash (FF-C6)
    const outputs = screen.getByRole('heading', { name: 'Final transaction outputs' })
      .parentElement!.parentElement!;
    expect(within(outputs).getAllByRole('link', { name: 'ckb1qzda0c…ppedrr' })).toHaveLength(2);
    // every block height is written with separators (FF-C6)
    expect(container.textContent).not.toMatch(/#1563/);
  });

  it('reads the commitment delay as its bytes plus the decoded meaning, no decimal u64 (FF-C1)', () => {
    render(
      <FiberChannelBoard channel={forceClosed as FiberChannelDetail} evidence={waitingEvidence} />
    );
    expect(screen.getByText('Delay · 8 bytes, LE').nextElementSibling).toHaveTextContent(
      '0x01000000000100a0 = 0xa000010000000001 · relative · 1 + 0/1 epoch'
    );
    expect(screen.queryByText('11529216145580097537')).toBeNull();
    expect(formatEpochDelay({ relative: false, epochs: 180, index: 3, length: 10 }, 'x')).toBe(
      'absolute · 180 + 3/10 epochs'
    );
    expect(() =>
      formatEpochDelay({ relative: true, epochs: 1, index: 2, length: 2 }, '11529216145580097537')
    ).toThrow('Fiber delay 11529216145580097537: invalid decoded epoch');
  });

  it('keeps a later commitment distinct from the force close while the settlement waits', () => {
    render(
      <FiberChannelBoard channel={forceClosed as FiberChannelDetail} evidence={waitingEvidence} />
    );
    expect(screen.getByTestId('step-2')).toHaveTextContent('2 · Force close');
    expect(screen.getByTestId('step-3')).toHaveTextContent(
      `3 · Updated commitment ${waitingEvidence.currentCommitment!.txHash}`
    );
    expect(screen.getByTestId('step-3')).toHaveAttribute('data-carries', '1');
    expect(
      screen.getByRole('heading', { name: 'On chain, so far in three steps' })
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Subsequent settlement cell: original delay is not reapplied/)
    ).toBeInTheDocument();
    expect(screen.getByText('9007199254740993')).toBeInTheDocument();
    expect(
      screen.getByRole('link', { name: `${waitingEvidence.currentCommitment!.txHash}:1` })
    ).toHaveAttribute('href', `/mainnet/cell/${waitingEvidence.currentCommitment!.txHash}-1`);
  });

  it('shows the settlement hash and flag of the 58-byte 0.7.0 layout as of the 57-byte one', () => {
    const v1: FiberChannelEvidence = {
      ...waitingEvidence,
      currentCommitment: {
        ...waitingEvidence.currentCommitment!,
        lockArgs: `0x${'44'.repeat(57)}01`,
        lockFormat: 'full58',
      },
    };
    render(<FiberChannelBoard channel={forceClosed as FiberChannelDetail} evidence={v1} />);
    expect(screen.getByText(/Fiber Commitment Lock · args, 58 bytes/)).toHaveTextContent('full58');
    expect(screen.getByText('Settlement hash / flag').nextElementSibling).toHaveTextContent(
      `0x${'66'.repeat(20)} · 1`
    );
  });

  it('names a legacy layout by its own length (56-byte daric args)', () => {
    const daric: FiberChannelEvidence = {
      ...waitingEvidence,
      currentCommitment: {
        ...waitingEvidence.currentCommitment!,
        lockArgs: `0x${'44'.repeat(56)}`,
        settlementHash: null,
        settlementFlag: null,
        lockFormat: 'legacy56',
        epochLock: 'legacyFormatRuleUnverified',
      },
    };
    render(<FiberChannelBoard channel={forceClosed as FiberChannelDetail} evidence={daric} />);
    expect(screen.queryByText('Settlement hash / flag')).toBeNull();
    expect(
      screen.getByText('Legacy 56-byte form: the deployed delay rule is not asserted here.')
    ).toBeInTheDocument();
  });

  it('renders the producer’s open channel: one step, still open, opening flows in tier 2', () => {
    expect(producerDetail.channelId).toBe(producerEvidence.channelId);
    render(
      <FiberChannelBoard
        channel={producerDetail as FiberChannelDetail}
        evidence={producerEvidence as FiberChannelEvidence}
      />
    );
    expect(
      screen.getByRole('heading', { name: 'On chain, so far in one step' })
    ).toBeInTheDocument();
    expect(screen.getByTestId('step-1')).toHaveTextContent(/Still open after/);
    const amounts = Array.from(document.querySelectorAll('[data-ckb-amount]')).map(
      (el) => el.textContent
    );
    expect(amounts).toEqual(
      expect.arrayContaining(['1,000.00000000 CKB', '699.00000000 CKB', '+301.00000000 CKB'])
    );
    expect(screen.getByText(/output-only lock did not necessarily sign/)).toBeInTheDocument();
    expect(screen.getByText(/not a proven channel balance or payment/)).toBeInTheDocument();
    // Responsive fix plan 2026-11-01 (A3, report C#20): "net" never breaks from its value.
    const flow = screen.getByText((_, el) => el?.getAttribute('data-flow') === 'net');
    expect(flow).toHaveTextContent(/^net \+301\.00000000 CKB$/);
    expect(flow).toHaveClass('whitespace-nowrap');
    for (const kind of ['in', 'back'])
      expect(screen.getByText((_, el) => el?.getAttribute('data-flow') === kind)).toHaveClass(
        'whitespace-nowrap'
      );
  });

  it('shows a token-funded channel’s token amount beside its CKB capacity (FF-C7)', () => {
    render(
      <FiberChannelBoard
        channel={{
          ...channel,
          udtTypeHash: `0x${'7f'.repeat(32)}`,
          udtAmount: '123450000',
        }}
        evidence={evidence}
        token={{ symbol: 'USDI', decimals: 6 }}
      />
    );
    expect(screen.getByTestId('channel-pair')).toHaveTextContent('holds 123.450000 USDI');
  });
});
