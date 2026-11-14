import { Component, type ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { onlineManager } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { server } from '@/__tests__/msw/server';
import { blockUncle } from '@/__tests__/msw/fixtures/block-uncle';
import UncleDetailPage from '@/app/blocks/[id]/uncles/[index]/client-page';
import { loadUnderStillClock, T0, tick } from '@/__tests__/utils/still-clock';

const params = vi.hoisted(() => ({ current: { id: '8775638', index: '0' } }));
vi.mock('@/src/navigation', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/src/navigation')>()),
  useParams: () => params.current,
  usePathname: () => `/blocks/${params.current.id}/uncles/${params.current.index}`,
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));

class Boundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };
  static getDerivedStateFromError(error: Error) {
    return { error };
  }
  render() {
    return this.state.error ? <p role="alert">{this.state.error.message}</p> : this.props.children;
  }
}

describe('Uncle detail page', () => {
  it('reads the uncle through its including block, with header and DAO evidence', async () => {
    params.current = { id: '8775638', index: '0' };
    render(<UncleDetailPage />);
    // PB-06 / PA-32: an entity h1 without a seal takes the phone step too (30 → 26 px below md).
    expect(await screen.findByRole('heading', { name: 'Uncle #8,775,637' })).toHaveClass(
      'text-entity',
      'max-md:text-index'
    );
    const sentence = screen.getByText(/^Uncle at height #8,775,637, included by/);
    expect(sentence).toHaveTextContent('on the canonical chain is');
    expect(within(sentence).getByRole('link', { name: '#8,775,638' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/8775638'
    );
    expect(screen.getByText('uncle 0', { exact: false })).toBeInTheDocument();
    expect(screen.getAllByText('2026-02-22 07:59:40 UTC+8').length).toBeGreaterThan(0);
    expect(screen.getAllByText('5,414 · 1 / 1,800')).toHaveLength(2);
    expect(screen.getByText('0xaabbccddeeff00112233')).toBeInTheDocument();
    expect(screen.getByText('1,210,829,853,049,231,829')).toBeInTheDocument();
    const decoded = screen.getByLabelText('Decoded DAO header');
    expect(decoded).toHaveTextContent('65,862,172,919.14818600');
    expect(decoded).toHaveTextContent('1.2065094518945692');
    expect(decoded).toHaveTextContent('S · compensation still owed + the burnt share');
    expect(screen.getByText(blockUncle.dao.raw)).toBeInTheDocument();
  });

  // The review's item E: the plate's id is the uncle's copy control; a click writes the WHOLE hash
  it('copies the whole uncle hash when the plate’s id is clicked', async () => {
    params.current = { id: '8775638', index: '0' };
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    const { container } = render(<UncleDetailPage />);
    await screen.findByRole('heading', { name: 'Uncle #8,775,637' });
    fireEvent.click(container.querySelector('[data-plate-hash]') as HTMLElement);
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(blockUncle.hash));
    expect(blockUncle.hash).toBe(`0x${'ee'.repeat(32)}`);
  });

  it('writes the header epoch like the facts and never breaks DAO prose mid-word (BA-41/BA-42)', async () => {
    params.current = { id: '8775638', index: '0' };
    render(<UncleDetailPage />);
    await screen.findByRole('heading', { name: 'Uncle #8,775,637' });
    // facts strip + raw header row: one epoch format on the page
    expect(screen.getAllByText('5,414 · 1 / 1,800')).toHaveLength(2);
    const decoded = screen.getByLabelText('Decoded DAO header');
    expect(decoded.closest('.break-all')).toBeNull();
    expect(screen.getByText(blockUncle.dao.raw)).toHaveClass('break-all');
  });

  it('shows the checked endpoint for an index the block does not have', async () => {
    params.current = { id: '8775638', index: '3' };
    render(<UncleDetailPage />);
    // the checked endpoint stands in the facts (the eyebrow shows the route, which here reads the same)
    expect(
      (await screen.findAllByText('/blocks/8775638/uncles/3')).some((el) =>
        el.closest('.error-facts')
      )
    ).toBe(true);
    expect(screen.getByText('no uncle at this block and index.')).toBeInTheDocument();
  });

  it('refuses decoded DAO fields that disagree with the raw header bytes', async () => {
    params.current = { id: '8775638', index: '0' };
    server.use(
      http.get('/api/:network/v1/blocks/:id/uncles/:index', () =>
        HttpResponse.json({ ...blockUncle, dao: { ...blockUncle.dao, s: '0' } })
      )
    );
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    render(
      <Boundary>
        <UncleDetailPage />
      </Boundary>
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'DAO S disagrees with the raw header bytes'
    );
    spy.mockRestore();
  });

  // F2 blocks (BA-43): the sibling relation, read from R10's sameParentAs / msAfterSibling.
  it('names the canonical block at its height as its sibling on the same parent, and the delay', async () => {
    params.current = { id: '8775638', index: '0' };
    render(<UncleDetailPage />);
    await screen.findByRole('heading', { name: 'Uncle #8,775,637' });
    const sentence = screen.getByText(/^Uncle at height #8,775,637, included by/);
    expect(sentence).toHaveTextContent(
      /on the canonical chain is 0x.+, its sibling: both were mined on parent #8,775,636 0x.+, and this uncle came 92 ms after it\./
    );
    expect(screen.getByText('92 ms after its sibling')).toBeInTheDocument();
  });

  it('says the canonical block at its height has another parent when they are not siblings', async () => {
    params.current = { id: '8775638', index: '0' };
    server.use(
      http.get('/api/:network/v1/blocks/:id/uncles/:index', () =>
        HttpResponse.json({ ...blockUncle, sameParentAs: null, msAfterSibling: null })
      )
    );
    render(<UncleDetailPage />);
    await screen.findByRole('heading', { name: 'Uncle #8,775,637' });
    const sentence = screen.getByText(/^Uncle at height #8,775,637, included by/);
    expect(sentence).toHaveTextContent('which has another parent');
    expect(sentence).not.toHaveTextContent('sibling');
  });

  it('refuses sibling facts that contradict each other', async () => {
    params.current = { id: '8775638', index: '0' };
    server.use(
      http.get('/api/:network/v1/blocks/:id/uncles/:index', () =>
        HttpResponse.json({ ...blockUncle, msAfterSibling: null })
      )
    );
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    render(
      <Boundary>
        <UncleDetailPage />
      </Boundary>
    );
    expect(await screen.findByRole('alert')).toHaveTextContent('sibling facts disagree');
    spy.mockRestore();
  });

  it('shows the packed epoch with its reading in the header (BA-41, R2b-a rawEpoch)', async () => {
    params.current = { id: '8775638', index: '0' };
    render(<UncleDetailPage />);
    await screen.findByRole('heading', { name: 'Uncle #8,775,637' });
    expect(screen.getByText('0x7080001001526')).toBeInTheDocument();
    expect(screen.getByText('0x7080001001526').parentElement).toHaveTextContent(
      '0x7080001001526 = 5,414 · 1 / 1,800'
    );
  });

  // Lane E4: the Time fact's age reads the page's one clock, so it moves with no data change; it
  // was written once at render and stood still. The clock stops with the page.
  it('moves the Time fact’s age with no data change, and leaves no timer behind (E4)', async () => {
    params.current = { id: '8775638', index: '0' };
    vi.useFakeTimers({ now: T0 });
    try {
      server.use(
        http.get('*/blocks/:id/uncles/:index', () =>
          HttpResponse.json({ ...blockUncle, timestamp: new Date(T0 - 20_000).toISOString() })
        )
      );
      const { unmount } = render(<UncleDetailPage />);
      const age = () => screen.queryByText(/^\d+[smh] ago$/)?.textContent;
      await loadUnderStillClock(() => age() !== undefined);
      expect(age()).toBe('20s ago');
      tick(1000);
      expect(age()).toBe('21s ago');
      tick(38_000);
      expect(age()).toBe('59s ago');
      tick(1000);
      expect(age()).toBe('1m ago');
      unmount();
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });

  // Review fix: a read paused between retries (a hidden tab, the browser offline) is pending
  // without fetching, so isLoading is false: the page drew nothing at all while it waited.
  it('says it is loading while its read is paused offline, then shows the uncle once back online', async () => {
    params.current = { id: '8775638', index: '0' };
    onlineManager.setOnline(false);
    try {
      render(<UncleDetailPage />);
      expect(await screen.findByText('Loading uncle…')).toHaveAttribute('role', 'status');
      act(() => onlineManager.setOnline(true));
      expect(await screen.findByRole('heading', { name: 'Uncle #8,775,637' })).toBeInTheDocument();
      expect(screen.queryByText('Loading uncle…')).toBeNull();
    } finally {
      onlineManager.setOnline(true);
    }
  });
});
