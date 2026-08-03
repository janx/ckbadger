import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '../utils/test-utils';
import FiberChannelsPage from '@/app/fiber/channels/client-page';
import { server } from '../msw/server';
import { http, HttpResponse } from 'msw';

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

describe('FiberChannelsPage', () => {
  beforeEach(() => {
    server.use(
      http.get('/api/:network/v1/fiber/stats', () =>
        HttpResponse.json({
          totalChannels: 42,
          openChannels: 10,
          totalCapacityLocked: '500000000000',
        })
      ),
      http.get('/api/:network/v1/fiber/channels', () =>
        HttpResponse.json({
          data: [],
          total: 0,
          limit: 50,
          hasMore: false,
          nextCursor: null,
        })
      )
    );
  });

  it('renders stats from the fiber stats API response shape', async () => {
    render(<FiberChannelsPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Fiber Channels')).toBeInTheDocument();
    expect(
      screen.getByText(
        'Follow the living circuitry of Fiber on Nervos, where nodes whisper value through channels like signals across a sleepless mind.'
      )
    ).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByText('Closed Channels')).toBeInTheDocument();
      expect(screen.getByText('32')).toBeInTheDocument();
      expect(screen.getByText('5,000.00000000')).toBeInTheDocument();
    });
  });
});
