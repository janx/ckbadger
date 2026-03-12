'use client';

import { useRouteError, isRouteErrorResponse } from 'react-router-dom';
import { NotFoundPage } from '@/components/not-found-page';

export function RouteErrorBoundary() {
  const error = useRouteError();

  let errMessage: string;

  if (isRouteErrorResponse(error)) {
    errMessage = error.statusText || `route_error: status ${error.status}`;
  } else if (error instanceof Error) {
    errMessage = error.message;
  } else {
    errMessage = 'unexpected_error: something went wrong';
  }

  return <NotFoundPage errMessage={errMessage} />;
}
