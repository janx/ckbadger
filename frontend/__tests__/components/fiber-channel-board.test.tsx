import { describe, expect, it, vi } from 'vitest';
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
    consumes?: { txHash: string; index: number; role: string };
    carries?: { index: number; role: string };
    box: { head: string; lines: string[] };
  }) => (
    <div
      data-testid={`step-${props.n}`}
      data-consumes={props.consumes ? `${props.consumes.txHash}:${props.consumes.index}` : ''}
      data-carries={props.carries ? String(props.carries.index) : ''}
    >
      {props.n} · {props.title} {props.txHash} | {props.box.head}
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
  it('opens with the pair, the capacity and one summary sentence from API facts (FF-C3, FF-C6)', () => {
    render(<FiberChannelBoard channel={channel} evidence={evidence} />);
    const pair = screen.getByTestId('channel-pair');
    expect(within(pair).getByRole('link', { name: 'ckb1qzda0c…ppedrr' })).toBeInTheDocument();
    expect(within(pair).getByRole('link', { name: 'ckb1qzda0c…5eyy6y' })).toBeInTheDocument();
    expect(
      Array.from(pair.querySelectorAll('[data-ckb-amount]')).map((el) => el.textContent)
    ).toEqual(['1,062.00000000 CKB']);
    const summary = screen.getByTestId('channel-summary');
    expect(summary).toHaveTextContent(
      'ckb1qzda0c…ppedrr put 1,000.00000569 CKB net into the opening transaction and ckb1qzda0c…5eyy6y 62.00000351 CKB.'
    );
    expect(summary).toHaveTextContent(
      '8 d 3 h later the channel was closed without a joint signature: the commitment carried the last agreed split, and after a wait of 6 epochs the settlement paid it out.'
    );
    // no raw four-cell developer strip in tier 1 and no "0 d" durations
    expect(screen.queryByText('Funding cell capacity')).toBeNull();
    expect(screen.queryByText(/\b0 d\b/)).toBeNull();
  });

  it('draws what each side put in at open as one split bar (FF-C3)', () => {
    const { container } = render(<FiberChannelBoard channel={channel} evidence={evidence} />);
    expect(screen.getByRole('heading', { name: 'What moved inside the channel' })).toHaveClass(
      'text-[15px]'
    );
    const bar = container.querySelector('[data-split="left"]') as HTMLElement;
    expect(parseFloat(bar.style.width)).toBeGreaterThan(94);
    expect(
      screen.getByText(/^at open, 2025-03-06 · net input to the opening transaction/)
    ).toBeInTheDocument();
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

  it('keeps the lock evidence in tier 2: 72 px boundary, quiet 13 px headings (FF-C5, FF-C8)', () => {
    const { container } = render(<FiberChannelBoard channel={channel} evidence={evidence} />);
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    expect(tier2).toHaveClass('mt-[72px]');
    expect(tier2.parentElement!.className).not.toMatch(/space-y-/);
    for (const name of ['The two locks', 'Witnesses and timing']) {
      const heading = screen.getByRole('heading', { name });
      expect(tier2).toContainElement(heading);
      expect(heading).toHaveClass('text-[13px]', 'text-[#a0a8be]');
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
