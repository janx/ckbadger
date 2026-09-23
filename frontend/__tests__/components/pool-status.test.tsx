import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import {
  PoolStatusBadge,
  PoolInterpretationNotice,
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
          reasons: [{ code: 'dao_compensation_unavailable' }],
        }}
      />
    );
    expect(screen.getByText(/DAO compensation not yet known/)).toBeInTheDocument();
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
