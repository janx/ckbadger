import { Component, type ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { http, HttpResponse } from 'msw';
import { server } from '@/__tests__/msw/server';
import { blockUncle } from '@/__tests__/msw/fixtures/block-uncle';
import UncleDetailPage from '@/app/blocks/[id]/uncles/[index]/client-page';

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
    expect(await screen.findByRole('heading', { name: 'Uncle #8,775,637' })).toBeInTheDocument();
    const sentence = screen.getByText(/^Uncle at height #8,775,637, included by/);
    expect(sentence).toHaveTextContent('on the canonical chain is');
    expect(within(sentence).getByRole('link', { name: '#8,775,638' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/8775638'
    );
    expect(screen.getByText('uncle 0', { exact: false })).toBeInTheDocument();
    expect(screen.getAllByText('2026-02-21 23:59:40 UTC').length).toBeGreaterThan(0);
    expect(screen.getAllByText('5,414 · 1 / 1,800')).toHaveLength(2);
    expect(screen.getByText('0xaabbccddeeff00112233')).toBeInTheDocument();
    expect(screen.getByText('1,210,829,853,049,231,829')).toBeInTheDocument();
    const decoded = screen.getByLabelText('Decoded DAO header');
    expect(decoded).toHaveTextContent('65,862,172,919.14818600');
    expect(decoded).toHaveTextContent('1.2065094518945692');
    expect(decoded).toHaveTextContent('S · compensation still owed + the burnt share');
    expect(screen.getByText(blockUncle.dao.raw)).toBeInTheDocument();
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
    expect((await screen.findByText('checked')).nextElementSibling).toHaveTextContent(
      '/blocks/8775638/uncles/3'
    );
    expect(
      screen.getByText('This explorer could not open the uncle at this address.')
    ).toBeInTheDocument();
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
});
