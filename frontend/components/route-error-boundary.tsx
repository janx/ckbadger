'use client';

import { useRouteError, isRouteErrorResponse } from 'react-router-dom';
import { NotFoundPage } from '@/components/not-found-page';
import { CrashPage } from '@/components/crash-page';

export function RouteErrorBoundary() {
  const error = useRouteError();

  if (isRouteErrorResponse(error) && error.status === 404) return <NotFoundPage />;

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
