'use client';

import type { ReactNode } from 'react';
import { ErrorBoundary } from '@/components/error-boundary';
import { readErrorText } from '@/lib/read-error';

/**
 * An entity page's own read that failed without a definite answer — a 5xx, a dropped connection
 * — said one way on every entity page (U-06): what could not be read, and the failure in its own
 * words (`readErrorText`: the API's message, or the gateway's status), never an exception's class
 * name. A 404 or a 400 is not this: those are the not-found page's.
 */
export function ReadFailure({ what, error }: { what: string; error: Error }) {
  return (
    // a sentence that explains what is missing and why: the reading ink (R3)
    <p role="alert" className="text-text text-small max-w-measure py-3 [overflow-wrap:anywhere]">
      Could not read this {what}: {readErrorText(error)}
    </p>
  );
}

/**
 * A part of a page that cannot be read says so in its place (review 45fd ruling D7; lane B4 second
 * fix round): what could not be read and the refusal in its own words, never the page. A refusal
 * names its row (an address, a lock), so the upstream fault is found from the sentence.
 */
export function PartReadError({ what, error }: { what: string; error: Error }) {
  return (
    // a sentence that explains what is missing and why: the reading ink (R3)
    <span role="alert" className="text-text text-small block [overflow-wrap:anywhere]">
      Could not read {what}: {readErrorText(error)}
    </span>
  );
}

/** A section a refusal while it draws fails in its place (`PartReadError`), not the page. */
export function PartReadBoundary({ what, children }: { what: string; children: ReactNode }) {
  return (
    <ErrorBoundary fallbackRender={(error) => <PartReadError what={what} error={error} />}>
      {children}
    </ErrorBoundary>
  );
}

/**
 * A part a page decides before it draws it (whether a list holds it), read, or the error it cannot
 * be read with, which the page draws in the part's place (`PartReadError`).
 */
export function readPart<T>(
  read: () => T
): { value: T; error?: undefined } | { value?: undefined; error: Error } {
  try {
    return { value: read() };
  } catch (error) {
    return { error: error instanceof Error ? error : new Error(String(error)) };
  }
}
