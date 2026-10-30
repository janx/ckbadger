import { Component, type ReactNode } from 'react';
import { QueryClientProvider } from '@tanstack/react-query';
import {
  act,
  createTestQueryClient,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@/__tests__/utils/test-utils';
import { CollectionHolders } from '@/components/asset/collection-holders';
import type { CollectionHolder, CursorPaginatedResponse } from '@/lib/api';

const lock = '0x' + '12'.repeat(32);
const prefix = '0x' + '34'.repeat(20);
const page = (
  data: CollectionHolder[],
  extra: Partial<CursorPaginatedResponse<CollectionHolder>> = {}
): CursorPaginatedResponse<CollectionHolder> => ({
  data,
  limit: 10,
  total: 2,
  hasMore: false,
  nextCursor: null,
  ...extra,
});
const named = { lockScriptHash: null, address: null, ownerHashPrefix: prefix, itemCount: 6 };
const resolved = { lockScriptHash: lock, address: null, itemCount: 2 };
// Unmodified holderDistribution from the real collection-insights producer.
const distribution = [
  { holders: 0, items: 0, range: '1' },
  { holders: 1, items: 2, range: '2' },
  { holders: 0, items: 0, range: '3–5' },
  { holders: 1, items: 6, range: '6+' },
];

describe('Collection holders', () => {
  it('keeps whole-scope distribution and named-only prefixes without inventing addresses', async () => {
    const load = vi.fn().mockResolvedValue(page([named, resolved]));
    render(<CollectionHolders load={load} queryKey="cluster" distribution={distribution} />);
    const table = await screen.findByRole('table', { name: 'Top 10 collection holders' });
    expect(load).toHaveBeenCalledWith({ limit: 10, cursor: undefined });
    const rows = within(table).getAllByRole('row');
    expect(rows[1]).toHaveTextContent('unresolved');
    expect(within(rows[1]).queryByRole('link')).not.toBeInTheDocument();
    expect(within(rows[1]).getByTitle(prefix)).toBeInTheDocument();
    expect(within(rows[2]).getByRole('link')).toHaveAttribute('href', `/mainnet/address/${lock}`);
    expect(screen.getByText('Full collection: 2 holders · 8 items')).toBeInTheDocument();
    expect(within(table).queryByText(/%/)).not.toBeInTheDocument();
    const buckets = screen.getByRole('table', { name: 'Holders by how many items they hold' });
    expect(within(buckets).getAllByRole('row')).toHaveLength(5);
  });

  it('uses backend ordering and cursor pages, and resets mode and cursor for a new collection', async () => {
    const load = vi
      .fn()
      .mockImplementation(({ limit, cursor }) =>
        Promise.resolve(
          cursor
            ? page([resolved], { limit, total: 51 })
            : page([named], { limit, total: 51, hasMore: true, nextCursor: 'next-50' })
        )
      );
    const { rerender } = render(<CollectionHolders load={load} queryKey="first" />);
    await screen.findByRole('table', { name: 'Top 10 collection holders' });
    fireEvent.click(screen.getByRole('button', { name: 'All holders →' }));
    await waitFor(() => expect(load).toHaveBeenLastCalledWith({ limit: 50, cursor: undefined }));
    fireEvent.click(await screen.findByRole('button', { name: 'Next' }));
    await waitFor(() => expect(load).toHaveBeenLastCalledWith({ limit: 50, cursor: 'next-50' }));
    const table = await screen.findByRole('table', { name: 'All collection holders' });
    expect(within(table).getAllByRole('row')[1]).toHaveTextContent('51');
    expect(screen.getByRole('button', { name: 'Next' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Previous' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Previous' })).toBeDisabled());
    rerender(<CollectionHolders load={load} queryKey="second" />);
    await screen.findByRole('table', { name: 'Top 10 collection holders' });
    expect(load).toHaveBeenLastCalledWith({ limit: 10, cursor: undefined });
    expect(screen.queryByRole('button', { name: 'Previous' })).not.toBeInTheDocument();
  });

  it('shows refresh failures while retaining the previously read holder rows', async () => {
    const client = createTestQueryClient();
    const load = vi.fn().mockResolvedValue(page([resolved]));
    render(
      <QueryClientProvider client={client}>
        <CollectionHolders load={load} queryKey="retained" />
      </QueryClientProvider>
    );
    await screen.findByRole('table', { name: 'Top 10 collection holders' });
    load.mockRejectedValue(new Error('secondary read failed'));
    await act(async () => {
      await client.refetchQueries({ queryKey: ['collection-holders', 'retained'] });
    });
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not refresh collection holders: secondary read failed. Showing previously loaded data.'
    );
    expect(screen.getByRole('table', { name: 'Top 10 collection holders' })).toBeInTheDocument();
  });

  it('shows a read failure separately from an empty collection', async () => {
    const load = vi.fn().mockRejectedValue(new Error('unavailable'));
    render(<CollectionHolders load={load} queryKey="failure" />);
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not load collection holders');
    expect(screen.queryByText('No current holders.')).not.toBeInTheDocument();
  });

  it('sums full-scope counts exactly beyond a JavaScript safe integer', async () => {
    render(
      <CollectionHolders
        load={vi.fn().mockResolvedValue(page([], { total: 0 }))}
        queryKey="exact-totals"
        distribution={[
          { range: '1', holders: Number.MAX_SAFE_INTEGER, items: Number.MAX_SAFE_INTEGER },
          { range: '2', holders: 2, items: 4 },
        ]}
      />
    );
    expect(
      screen.getByText(
        'Full collection: 9,007,199,254,740,993 holders · 9,007,199,254,740,995 items'
      )
    ).toBeInTheDocument();
    expect(await screen.findByText('No current holders.')).toBeInTheDocument();
  });

  it.each([
    [{ holders: -1, items: 0, range: '1' }],
    [{ holders: 1, items: Number.MAX_SAFE_INTEGER + 1, range: '6+' }],
    [{ holders: 2, items: 1, range: '1' }],
    [{ holders: 0, items: 1, range: '1' }],
    [distribution[0], distribution[0]],
  ])('rejects invalid full-scope counts or repeated buckets %#', (...buckets) => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      expect(() =>
        render(
          <CollectionHolders
            load={vi.fn().mockResolvedValue(page([]))}
            queryKey="bad-distribution"
            distribution={buckets}
          />
        )
      ).toThrow(/collection|Collection/);
    } finally {
      error.mockRestore();
    }
  });

  it('fails fast when the distribution counts fewer holders than the list returned (#3)', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const load = vi.fn().mockResolvedValue(page([named, resolved]));
    class Boundary extends Component<{ children: ReactNode }, { message: string | null }> {
      state = { message: null as string | null };
      static getDerivedStateFromError(e: Error) {
        return { message: e.message };
      }
      render() {
        return this.state.message ? <p role="alert">{this.state.message}</p> : this.props.children;
      }
    }
    render(
      <Boundary>
        <CollectionHolders load={load} queryKey="stub" distribution={[]} />
      </Boundary>
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'distribution counts 0 holders but the list returned 2'
    );
    error.mockRestore();
  });

  it('lets an empty distribution with no listed holders read as genuinely no holders', async () => {
    const load = vi.fn().mockResolvedValue(page([], { total: 0 }));
    render(<CollectionHolders load={load} queryKey="none" distribution={[]} />);
    expect(await screen.findByText('Full collection: 0 holders · 0 items')).toBeInTheDocument();
  });
});
