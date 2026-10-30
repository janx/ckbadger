// Kept apart from `@/lib/api` so the query client's retry rule does not depend on how a test
// mocks the API module.

/**
 * "The resource does not exist", read from the HTTP status the API answered
 * with — never from the rendered message text, which is prose and changes.
 */
export function isNotFoundApiError(error: unknown): error is { status: 404 } {
  if (!error || typeof error !== 'object') {
    return false;
  }
  return (error as { status?: unknown }).status === 404;
}
