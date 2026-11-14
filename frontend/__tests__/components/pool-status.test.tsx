import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import {
  PoolStatusBadge,
  PoolInterpretationNotice,
  PoolTruncatedNotice,
  PoolUnavailableNotice,
  TimeInPool,
  poolStatusLabel,
} from '@/components/ui/pool-status';

describe('pool status labels', () => {
  it('names each state the node can report', () => {
    expect(poolStatusLabel('pending')).toBe('Pending');
    expect(poolStatusLabel('proposed')).toBe('Proposed');
    // The node has it in a block; ckbadger has not indexed it yet. It is no
    // longer "pending", and it is not yet a confirmed row either.
    expect(poolStatusLabel('committed_awaiting_index')).toBe('Confirming');
  });
});

describe('PoolStatusBadge', () => {
  it('marks an unconfirmed transaction', () => {
    render(<PoolStatusBadge status="pending" />);
    expect(screen.getByText('Pending')).toBeInTheDocument();
  });
});

describe('TimeInPool', () => {
  it('reports how long the node has held the transaction', () => {
    const fiveMinutesAgo = new Date(Date.now() - 5 * 60 * 1000).toISOString();
    render(<TimeInPool since={fiveMinutesAgo} />);
    expect(screen.getByText(/in pool for 5m/)).toBeInTheDocument();
  });

  // The lead's follow-up: the pool's wait was a second relative-age rule with a Math.max(0, …)
  // clamp, so a pending-since time further ahead than consensus allows read "in pool for 0s". It
  // takes format-age's one form and that form's treatment of a time ahead of this device's clock.
  it('reads its wait in the one age form, and says when this device’s clock is behind', () => {
    const { rerender } = render(<TimeInPool since={new Date(Date.now() + 2_000).toISOString()} />);
    // within the 15 s consensus allows: no age yet
    expect(screen.getByText('in pool for 0s')).toBeInTheDocument();
    rerender(<TimeInPool since={new Date(Date.now() + 60_000).toISOString()} />);
    expect(screen.getByText("this device's clock is behind")).toBeInTheDocument();
    expect(screen.queryByText(/in pool for/)).toBeNull();
  });
});

describe('PoolInterpretationNotice', () => {
  it('renders nothing for a complete interpretation', () => {
    const { container } = render(
      <PoolInterpretationNotice interpretation={{ status: 'complete' }} />
    );
    expect(container.textContent).toBe('');
  });

  it('says which layer could not be read, and why', () => {
    render(
      <PoolInterpretationNotice
        interpretation={{
          status: 'partial',
          reasons: [{ code: 'unresolved_input' }],
        }}
      />
    );
    expect(screen.getByText('partial: an input is not yet resolvable')).toBeInTheDocument();
  });

  it('shows a code it has no wording for verbatim rather than dropping it', () => {
    // DAO phase-2 compensation is computed exactly now, so the API no longer
    // sends `dao_compensation_unavailable`; were it to, it is not special.
    render(
      <PoolInterpretationNotice
        interpretation={{
          status: 'partial',
          reasons: [{ code: 'dao_compensation_unavailable' }, { code: 'x', detail: 'y' }],
        }}
      />
    );
    expect(screen.getByText('partial: dao_compensation_unavailable; x (y)')).toBeInTheDocument();
  });

  it('names the outpoint it could not resolve', () => {
    render(
      <PoolInterpretationNotice
        interpretation={{
          status: 'partial',
          reasons: [{ code: 'unresolved_input', detail: '0xabc:1' }],
        }}
      />
    );
    expect(screen.getByText(/0xabc:1/)).toBeInTheDocument();
  });
});

describe('PoolUnavailableNotice', () => {
  it('says the view is unavailable rather than implying an empty pool', () => {
    render(<PoolUnavailableNotice />);
    expect(screen.getByText(/pool view unavailable/i)).toBeInTheDocument();
  });
});

describe('PoolTruncatedNotice', () => {
  it('says the unconfirmed segment is incomplete', () => {
    render(<PoolTruncatedNotice />);
    expect(screen.getByText('More unconfirmed transactions than shown.')).toBeInTheDocument();
  });
});
