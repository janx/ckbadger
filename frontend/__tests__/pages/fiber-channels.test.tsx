import { describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor, fireEvent, within } from '../utils/test-utils';
import FiberChannelsPage from '@/app/fiber/channels/client-page';
import { server } from '../msw/server';
import { http, HttpResponse } from 'msw';
import network from '../fixtures/fiber-network.json';
import channel from '../fixtures/fiber-open.json';
import closedChannel from '../fixtures/fiber-force-closed.json';

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const page = (rows: unknown[]) => ({ data: rows, limit: 50, hasMore: false, nextCursor: null });

describe('FiberChannelsPage', () => {
  it('does not show a stale Open row beneath a newly selected filter while it loads', async () => {
    let releaseAll: (() => void) | undefined;
    server.use(
      http.get('/api/:network/v1/fiber/network', () => HttpResponse.json(network)),
      http.get('/api/:network/v1/fiber/channels', async ({ request }) => {
        const filter = new URL(request.url).searchParams.get('state');
        if (filter === null)
          await new Promise<void>((resolve) => {
            releaseAll = resolve;
          });
        return HttpResponse.json(page([filter === 'open' ? channel : closedChannel]));
      })
    );
    render(<FiberChannelsPage />);
    expect(await screen.findByTitle(channel.channelId)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /^All/ }));
    await waitFor(() => expect(releaseAll).toBeTypeOf('function'));
    expect(screen.queryByTitle(channel.channelId)).not.toBeInTheDocument();
    releaseAll?.();
    expect(await screen.findByTitle(closedChannel.channelId)).toBeInTheDocument();
  });

  it('leads with the board hero: open now, channels ever, parties, typical lifetime (FF-F5, FF-F9)', async () => {
    server.use(
      http.get('/api/:network/v1/fiber/network', () => HttpResponse.json(network)),
      http.get('/api/:network/v1/fiber/channels', () => HttpResponse.json(page([channel])))
    );
    render(<FiberChannelsPage />);
    expect(screen.getByRole('heading', { level: 1, name: 'Fiber channels' })).toHaveClass(
      'text-[26px]'
    );
    expect(
      screen.getByText(/Only opening and closing a channel touch the chain/)
    ).toBeInTheDocument();
    // no jade uppercase eyebrow in tier 1 (FF-F6)
    expect(screen.queryByText(/Payment channels · on-chain view/)).toBeNull();
    await waitFor(() => expect(screen.getAllByRole('term')).toHaveLength(4));
    expect(screen.getAllByRole('term').map((t) => t.textContent)).toEqual([
      'Open now',
      'Channels ever',
      'Parties',
      'Typical lifetime',
    ]);
    const hero = screen.getAllByRole('term')[0].closest('dl')!;
    expect(within(hero).getByText(/^since 2023-11-15$/)).toBeInTheDocument();
    expect(within(hero).getByText('in 1 pair')).toBeInTheDocument();
    expect(within(hero).getByText('1 m 40 s')).toBeInTheDocument();
    expect(within(hero).getByText('2 of 2 closed within an hour')).toBeInTheDocument();
    expect(
      Array.from(hero.querySelectorAll('[data-ckb-amount]')).map((el) => el.textContent)
    ).toEqual(['3,000.00000000 CKB']);
    expect(screen.queryByText(/0 d 0 h/)).toBeNull();
  });

  it('defaults the table to Open with state counts and the board columns (FF-F4, FF-F6)', async () => {
    const requested: string[] = [];
    server.use(
      http.get('/api/:network/v1/fiber/network', () => HttpResponse.json(network)),
      http.get('/api/:network/v1/fiber/channels', ({ request }) => {
        requested.push(new URL(request.url).searchParams.get('state') ?? 'all');
        return HttpResponse.json(page([channel]));
      })
    );
    render(<FiberChannelsPage />);
    expect(await screen.findByRole('heading', { name: 'Channels' })).toHaveClass('text-[15px]');
    await waitFor(() => expect(requested[0]).toBe('open'));
    const open = await screen.findByRole('button', { name: 'Open 1' });
    expect(open).toHaveAttribute('aria-pressed', 'true');
    for (const name of [
      'Closed together 1',
      'Force-closed, settled 0',
      'Force-closed, not settled 1',
      'All 3',
    ])
      expect(screen.getByRole('button', { name })).toHaveAttribute('aria-pressed', 'false');
    const table = await screen.findByRole('table', { name: 'Open channels' });
    expect(
      within(table)
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual(['Channel', 'Parties', 'Capacity', 'Opened', 'Open for']);
    expect(within(table).getByText('2023-11-15')).toBeInTheDocument();
    expect(within(table).getByText('3 m 20 s')).toBeInTheDocument();
    expect(within(table).getByText('0xaaaaaaaa…aaaaaa')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Closed together 1' }));
    await waitFor(() => expect(requested).toContain('cooperativelyClosed'));
    expect(
      await screen.findByRole('table', { name: 'Closed together channels' })
    ).toBeInTheDocument();
    expect(screen.getByRole('columnheader', { name: 'Lifetime' })).toBeInTheDocument();
  });
});
