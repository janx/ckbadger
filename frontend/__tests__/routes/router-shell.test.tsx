import { RouterProvider, createMemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { createAppRouter } from '@/src/routes/router';

vi.mock('@/app/page', () => ({
  default: function MockHomePage() {
    return (
      <main>
        <header role="banner">mock home</header>
      </main>
    );
  },
}));

describe('SPA router shell', () => {
  it('renders the home route through the SPA shell', async () => {
    const router = createMemoryRouter(createAppRouter(), {
      initialEntries: ['/'],
    });

    render(<RouterProvider router={router} />);

    expect(await screen.findByRole('banner')).toBeInTheDocument();
  });
});
