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
 * An error in its own words: its message, and its name before it where it is not a plain Error (a
 * TypeError is a fault of the page's own code, not a refusal of what it read).
 */
export function errorWords(error: Error): string {
  const text = readErrorText(error);
  return error.name && error.name !== 'Error' ? `${error.name}: ${text}` : text;
}

/**
 * A part of a page that cannot be drawn says so in its place (review 45fd ruling D7; lane B4 fix
 * rounds): what could not be drawn, where (the data it was drawing: a token and its view, a
 * transaction, a cell), and the error in its own words, never the page. A refusal names its row
 * (an address, a lock), so the upstream fault is found from the sentence.
 */
export function PartReadError({
  what,
  where,
  error,
}: {
  what: string;
  where: string;
  error: Error;
}) {
  return (
    // a sentence that explains what is missing and why: the reading ink (R3)
    <span role="alert" className="text-text text-small block [overflow-wrap:anywhere]">
      Could not draw {what} ({where}): {errorWords(error)}
    </span>
  );
}

/** Calls `render` while React renders it, so what it throws lands in the boundary around it. */
function Rendered({ render }: { render: () => ReactNode }) {
  return <>{render()}</>;
}

/**
 * A part of a section that a refusal while it draws fails in its place (`PartReadError`), never
 * the page. `render` runs inside the boundary (a column or a row computed while drawing is caught
 * too). It starts again when `where`, the data it draws, changes (a new view, page or cursor), and
 * it holds the part alone: the section's heading and its pager stay outside it.
 */
export function PartReadBoundary({
  what,
  where,
  render,
}: {
  what: string;
  where: string;
  render: () => ReactNode;
}) {
  return (
    <ErrorBoundary
      key={where}
      fallbackRender={(error) => <PartReadError what={what} where={where} error={error} />}
    >
      <Rendered render={render} />
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
