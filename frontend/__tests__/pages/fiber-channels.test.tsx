import { describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor, fireEvent } from '../utils/test-utils';
import FiberChannelsPage from '@/app/fiber/channels/client-page';
import { server } from '../msw/server';
import { http, HttpResponse } from 'msw';
import network from '../fixtures/fiber-network.json';
import channel from '../fixtures/fiber-open.json';

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

describe('FiberChannelsPage', () => {
  it('uses API-produced on-chain network and channel contracts', async () => {
    const requested: string[] = [];
    server.use(
      http.get('/api/:network/v1/fiber/network', () => HttpResponse.json(network)),
      http.get('/api/:network/v1/fiber/channels', ({ request }) => {
        requested.push(new URL(request.url).searchParams.get('state') ?? 'all');
        return HttpResponse.json({ data: [channel], limit: 50, hasMore: false, nextCursor: null });
      })
    );
    render(<FiberChannelsPage />);
    expect(screen.getByRole('heading', { name: 'Fiber channels' })).toBeInTheDocument();
    expect(screen.getByText(/cannot reveal off-chain payments/)).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText('On-chain party pairs')).toBeInTheDocument());
    expect(screen.getAllByText(/3,000\.00000000/).length).toBeGreaterThan(0);
    expect(screen.getByText('Unknown address · 0xaaaaaaaa…aaaaaaaa')).toBeInTheDocument();
    const open = screen.getByRole('button', { name: 'Open' });
    fireEvent.click(open);
    await waitFor(() => expect(requested).toContain('open'));
    expect(open).toHaveAttribute('aria-pressed', 'true');
  });
});
