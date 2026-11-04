'use client';

import { useRouteError, isRouteErrorResponse } from 'react-router-dom';
import { NotFoundPage } from '@/components/not-found-page';
import { CrashPage } from '@/components/crash-page';
import { isNotFoundApiError } from '@/lib/api-errors';

interface RouteErrorBoundaryProps {
  /**
   * The boundary of the frame route (`/:network`): the frame itself failed, so the error page is
   * drawn in its place with its own header and footer. The page routes' boundary (the default)
   * renders inside the frame, under its header (visual finish V6).
   */
  standalone?: boolean;
}

export function RouteErrorBoundary({ standalone = false }: RouteErrorBoundaryProps = {}) {
  const error = useRouteError();

  // C2: anything that reaches the boundary carrying HTTP 404 is a not-found — a route error
  // response, `notFound()` (RouteNotFoundError), or an API not-found thrown during render.
  if (isNotFoundApiError(error)) return <NotFoundPage standalone={standalone} />;

  let errMessage: string;

  if (isRouteErrorResponse(error)) {
    errMessage = error.statusText || `route_error: status ${error.status}`;
  } else if (error instanceof Error) {
    errMessage = error.message;
  } else {
    errMessage = 'unexpected_error: something went wrong';
  }

  return <CrashPage message={errMessage} standalone={standalone} />;
}
