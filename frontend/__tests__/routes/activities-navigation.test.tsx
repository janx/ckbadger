import { MemoryRouter, useLocation, useRoutes } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { createAppRouter } from '@/src/routes/router';

vi.mock('@/lib/api', () => ({
  api: {
    getGlobalActivities: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/components/layout/site-footer', () => ({
  SiteFooter: () => <div data-testid="site-footer">SiteFooter</div>,
}));

vi.mock('@/components/not-found-page', () => ({
  NotFoundPage: () => <div>not found page</div>,
}));

vi.mock('@/components/activities-stream-explorer', () => ({
  ActivitiesStreamExplorer: () => <div data-testid="activities-stream">stream</div>,
}));

function AppHarness() {
  const location = useLocation();
  const element = useRoutes(createAppRouter());

  return (
    <>
      <div data-testid="pathname">{location.pathname}</div>
      {element}
    </>
  );
}

describe('activities navigation', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.scrollTo = vi.fn();
  });

  it('renders the activities route instead of the 404 page', async () => {
    render(
      <MemoryRouter initialEntries={['/activities']}>
        <AppHarness />
      </MemoryRouter>
    );

    expect(await screen.findByText('Activities')).toBeInTheDocument();
    expect(screen.getByTestId('pathname')).toHaveTextContent('/activities');
    expect(screen.queryByText('not found page')).not.toBeInTheDocument();
  });
});
