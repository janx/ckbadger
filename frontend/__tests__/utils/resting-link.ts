/**
 * Visual finish V5 (resting chroma): a link inside a table, a feed or a list row carries no aqua at
 * rest. Its resting look (bright over a faint aqua underline) was what an Address drew for a caller
 * that stated no look, and went with the last of them (lane cd-fixes-controls, review-p-tests
 * SHOULD-4): a link rests in the look its caller states (components/ui/link.tsx).
 */

/** A class that rests a link in the aqua family (#68ccf0 aqua = interactive = info, and -dim). */
export const AQUA_AT_REST = /^(text-aqua|text-interactive|text-info)(-dim)?(\/\d+)?$/;
