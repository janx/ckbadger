// F2 blocks: the Blocks / Block / BlockTxs board pieces, each on its own.
import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { RewardBreakdown, RewardPaidBy } from '@/components/blocks/reward-breakdown';
import { BlockTransactionsTable } from '@/components/blocks/block-transactions-table';
import { positionSentence } from '@/components/blocks/block-position';
import { BlocksLimitsFootnote, ScaleHeader } from '@/components/blocks/block-visual';
import { ActivityRow } from '@/components/activity/activity-row';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { ExactCkb } from '@/components/activity-event-row';
import { ListTable } from '@/components/ui/list-table';
import type { Transaction } from '@/lib/api';

const rewardBlock = {
  number: 20_827_932,
  miningReward: '68432120795',
  miningRewardTxHash: `0x${'c7'.repeat(32)}`,
  rewardBreakdown: {
    primary: '65142942227',
    secondary: '3286129910',
    committed: '3036000',
    proposal: '12658',
  },
  rewardPaidAtBlock: 20_827_943,
  rewardFinalizesAt: 20_827_943,
  minerAddress: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqgp909qk',
  totalFees: '5060000',
  transactionsCount: 4,
};
const wrap = (ui: React.ReactElement) => render(<MemoryRouter>{ui}</MemoryRouter>);

describe('RewardBreakdown', () => {
  it('refuses shares that do not add up to the reward', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      wrap(<RewardBreakdown block={{ ...rewardBlock, miningReward: '68432120796' }} />)
    ).toThrow('do not add up');
  });
  it('draws only the non-zero shares, each at least 3 px', () => {
    wrap(
      <RewardBreakdown
        block={{
          ...rewardBlock,
          miningReward: '68429072137',
          rewardBreakdown: { ...rewardBlock.rewardBreakdown, committed: '0', proposal: '0' },
        }}
      />
    );
    const parts = screen.getByRole('img').querySelectorAll('[data-reward-part]');
    expect(Array.from(parts).map((p) => p.getAttribute('data-reward-part'))).toEqual([
      'primary',
      'secondary',
    ]);
    expect(screen.getByText('Proposal fees, 40%')).toBeInTheDocument();
  });
  it('says where an unfinalized reward will be paid', () => {
    wrap(<RewardBreakdown block={{ ...rewardBlock, miningReward: null, rewardBreakdown: null }} />);
    expect(screen.getByText(/paid by the cellbase of/)).toHaveTextContent(
      'The reward is paid by the cellbase of #20,827,943; it is not indexed yet.'
    );
  });
  it('names the paying cellbase and its tx', () => {
    wrap(<RewardPaidBy block={rewardBlock} />);
    expect(screen.getByRole('link', { name: '#20,827,943' }).getAttribute('href')).toMatch(
      /\/blocks\/20827943$/
    );
  });
});

describe('positionSentence', () => {
  const block = { number: 932, proposalsCount: 2, unclesCount: 1 };
  it('reads the incoming and outgoing proposal links and the uncles', () => {
    expect(
      positionSentence(block, [
        { from: 930, to: 932 },
        { from: 932, to: 934 },
      ])
    ).toBe('committed what #930 proposed, proposed what #934 committed, and included 1 uncle');
  });
  it('says what is missing instead of inventing a link', () => {
    expect(positionSentence({ number: 5, proposalsCount: 0, unclesCount: 0 }, [])).toBe(
      'committed no proposed transactions, proposed nothing, and included no uncle'
    );
    expect(positionSentence({ number: 5, proposalsCount: 3, unclesCount: 2 }, [])).toBe(
      'committed no proposed transactions, proposed 3 transactions, none committed yet, and included 2 uncles'
    );
  });
});

describe('BlockTransactionsTable', () => {
  const tx = (index: number, over: Partial<Transaction> = {}): Transaction =>
    ({
      hash: `0x${String(index).repeat(64)}`,
      blockNumber: 7,
      index,
      inputsCount: 1,
      outputsCount: 1,
      fee: '0',
      txSize: 300,
      cycles: 0,
      cyclesStatus: 'complete',
      outputCapacity: '100',
      proposalOrigin: index ? { blockNumber: 5, distance: 2, uncle: null } : null,
      isCellbase: index === 0,
      ...over,
    }) as unknown as Transaction;
  it('fails on a transaction from another block', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      wrap(<BlockTransactionsTable transactions={[tx(0, { blockNumber: 8 })]} blockNumber={7} />)
    ).toThrow('is not in block 7');
  });
  it('writes "+distance" beside the proposing block', () => {
    wrap(
      <BlockTransactionsTable transactions={[tx(0), tx(1, { cycles: 1000 })]} blockNumber={7} />
    );
    const rows = screen.getByRole('table').querySelectorAll('tbody tr');
    expect(rows[1]).toHaveTextContent('#5 +2');
    expect(rows[0]).toHaveTextContent('cellbase');
  });
});

describe('Blocks list pieces', () => {
  it('heads a bar column with 0 … scale as wide as its track', () => {
    render(<ScaleHeader label="Cycles" scale={8_000_000} trackWidth={96} />);
    expect(screen.getByText('8,000,000').parentElement).toHaveStyle({ width: '96px' });
    expect(() => ScaleHeader({ label: 'x', scale: 0 })).toThrow('invalid column scale');
  });
  it('rejects a page whose blocks disagree on the consensus limits', () => {
    const row = {
      number: 1,
      size: 600,
      cycles: null,
      maxBlockBytes: 597_000,
      maxBlockCycles: '3500000000',
    };
    expect(() =>
      BlocksLimitsFootnote({ blocks: [row, { ...row, number: 2, maxBlockBytes: 1 }] })
    ).toThrow('consensus limits differ');
    render(<BlocksLimitsFootnote blocks={[row]} />);
    expect(screen.getByText(/cycles not run yet for these blocks/)).toBeInTheDocument();
  });
});

describe('additive shared props (F2 blocks)', () => {
  it('ActivityRow takes its evidence line from the page', () => {
    wrap(
      <ActivityRow
        activity={{
          txHash: `0x${'ab'.repeat(32)}`,
          blockNumber: 7,
          timestamp: '1700000000000',
          isCellbase: true,
          protocolActions: [],
          typeCalls: [],
          lockCalls: [],
          participants: [],
        }}
        headline="Cellbase"
        evidence={<>#0 · tx</>}
      />
    );
    expect(document.querySelector('[data-role="evidence"]')).toHaveTextContent(/^#0 · tx$/);
  });
  it('CursorPagination takes a range sentence and button words', () => {
    render(
      <CursorPagination
        summary="50 newest of 9"
        hasMore
        hasPrevious={false}
        onNext={() => {}}
        onPrevious={() => {}}
        previousLabel="Newer"
        nextLabel="Older →"
      />
    );
    expect(screen.getByText('50 newest of 9')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Older →' })).toBeEnabled();
  });
  it('ExactCkb drops the unit when the header names it', () => {
    const { container } = render(<ExactCkb shannons={BigInt(150000000)} unit={false} />);
    expect(container).toHaveTextContent(/^\+1\.50000000$/);
  });
  it('ListTable heads a column with its rich header', () => {
    render(
      <ListTable
        label="t"
        rows={[1]}
        rowKey={String}
        columns={[{ key: 'a', label: 'A', header: <b>A rich</b>, minWidth: 10, render: String }]}
      />
    );
    expect(screen.getByRole('columnheader')).toHaveTextContent('A rich');
  });
});
