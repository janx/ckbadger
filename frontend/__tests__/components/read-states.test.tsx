import type { ReactNode } from 'react';
import { afterEach, describe, expect, it } from 'vitest';
import { act, render, screen } from '@testing-library/react';
import { onlineManager, QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query';
import { AssetReadState } from '@/components/asset/detail-shell';
import { AddressLoadState } from '@/components/address/address-holdings';

// A1 review fix, round 2: the two section read states ask isAwaited themselves, so a read that is
// merely paused (offline, or a hidden tab between retries) says it is loading, not "unavailable",
// and a sentence is shown only once the read has answered that way.

type Answer = 'data' | 'nothing' | 'failure';

function answer(kind: Answer): Promise<string | null> {
  if (kind === 'failure') return Promise.reject(new Error('store read failed'));
  return Promise.resolve(kind === 'data' ? 'the data' : null);
}

function Section({
  state,
  kind,
  enabled = true,
}: {
  state: 'asset' | 'address';
  kind: Answer;
  enabled?: boolean;
}) {
  const read = useQuery({
    queryKey: ['section', state, kind, enabled],
    queryFn: () => answer(kind),
    enabled,
  });
  if (read.data) return <p>{read.data}</p>;
  return state === 'asset' ? (
    <AssetReadState error={read.error} read={read} label="things" />
  ) : (
    <AddressLoadState error={read.error} read={read} label="things" />
  );
}

function renderSection(ui: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={client}>{ui}</QueryClientProvider>);
}

afterEach(() => {
  onlineManager.setOnline(true);
});

describe.each(['asset', 'address'] as const)('the %s read state', (state) => {
  it('says it is loading while its read is paused, then gives way to the data', async () => {
    onlineManager.setOnline(false);
    renderSection(<Section state={state} kind="data" />);
    expect(await screen.findByRole('status')).toHaveTextContent(/^Loading things…$/);
    expect(screen.queryByText(/unavailable/)).toBeNull();
    act(() => onlineManager.setOnline(true));
    expect(await screen.findByText('the data')).toBeInTheDocument();
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('says nothing of a disabled read: nothing has answered it', () => {
    const { container } = renderSection(<Section state={state} kind="data" enabled={false} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('says a failed read failed, once it has', async () => {
    renderSection(<Section state={state} kind="failure" />);
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not load things: store read failed'
    );
  });

  it('says "unavailable" only for a read that answered with nothing', async () => {
    renderSection(<Section state={state} kind="nothing" />);
    expect(await screen.findByText('things unavailable.')).toHaveAttribute('role', 'status');
  });
});

// Lane P1 follow-up (the ink-role gate's quiet <p> sites, R3): a failed read is a statement in the
// reading ink; the loading and the unavailable lines stay quiet, a transient and an empty line.
describe('the address read state’s inks', () => {
  it('writes a failure in the reading ink', async () => {
    renderSection(<Section state="address" kind="failure" />);
    expect(await screen.findByRole('alert')).toHaveClass('text-text');
    expect(screen.getByRole('alert')).not.toHaveClass('text-text-dim');
  });

  it('keeps the unavailable line quiet', async () => {
    renderSection(<Section state="address" kind="nothing" />);
    expect(await screen.findByRole('status')).toHaveClass('text-text-dim');
  });
});
