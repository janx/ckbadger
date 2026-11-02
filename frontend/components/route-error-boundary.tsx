'use client';

import { useRouteError, isRouteErrorResponse } from 'react-router-dom';
import { NotFoundPage } from '@/components/not-found-page';
import { CrashPage } from '@/components/crash-page';
import { isNotFoundApiError } from '@/lib/api-errors';

export function RouteErrorBoundary() {
  const error = useRouteError();

  // C2: anything that reaches the boundary carrying HTTP 404 is a not-found — a route error
  // response, `notFound()` (RouteNotFoundError), or an API not-found thrown during render.
  if (isNotFoundApiError(error)) return <NotFoundPage footer />;

  let errMessage: string;

  if (isRouteErrorResponse(error)) {
    errMessage = error.statusText || `route_error: status ${error.status}`;
  } else if (error instanceof Error) {
    errMessage = error.message;
  } else {
    errMessage = 'unexpected_error: something went wrong';
  }

  return <CrashPage message={errMessage} />;
}
