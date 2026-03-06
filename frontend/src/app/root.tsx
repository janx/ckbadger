import { RouterProvider, createBrowserRouter } from 'react-router-dom';
import { Providers } from '@/src/app/providers';
import { createAppRouter } from '@/src/routes/router';

const router = createBrowserRouter(createAppRouter());

export function AppRoot() {
  return (
    <Providers>
      <RouterProvider router={router} />
    </Providers>
  );
}
