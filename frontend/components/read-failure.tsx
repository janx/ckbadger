'use client';

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
