import type { Cell, CursorPaginatedResponse } from '@/lib/api';
import type { ScriptRefHashType } from '@/lib/script-ref';

type Page = CursorPaginatedResponse<Cell>;
interface ByScriptParams {
  codeHash: string;
  hashType: ScriptRefHashType;
  scriptKind?: 'lock' | 'type' | 'both';
  limit?: number;
  cursor?: string;
  order?: 'asc' | 'desc';
}

/**
 * Lane S: the script page also asks `GET /cells/by-script?order=desc` for its "Newest cells". A
 * test that serves a fixture page of the oldest-first list answers that request as the API does
 * for a universe of just those rows: the same rows newest first, each marked with the index it
 * came from (`matchedScriptKind`, which the API sets on every by-script row), at most `limit`.
 */
export function byScript(
  oldestFirst: (params: ByScriptParams) => Page | Promise<Page>
): (params: ByScriptParams) => Promise<Page> {
  return async (params) => {
    const page = await oldestFirst(params);
    if (params.order !== 'desc') return page;
    if (params.scriptKind !== 'lock' && params.scriptKind !== 'type')
      throw new Error(`order=desc needs script_kind lock or type, got ${params.scriptKind}`);
    const kind = params.scriptKind;
    const rows = [...page.data]
      .reverse()
      .slice(0, params.limit ?? page.data.length)
      .map((cell) => ({ ...cell, matchedScriptKind: kind }));
    return { ...page, data: rows, hasMore: rows.length < page.data.length };
  };
}

/**
 * The cell detail (`GET /cells/{tx}/{i}`) a "Newest cells" row reads, for a fixture list row:
 * the row's own fields plus the detail's protocol slugs (none: the fixtures' cells are of no
 * registered protocol). Undefined when the outpoint is not one of `rows`.
 */
export function rowDetail(
  rows: readonly Cell[],
  txHash: string,
  outputIndex: number
): Cell | undefined {
  const row = rows.find((cell) => cell.txHash === txHash && cell.outputIndex === outputIndex);
  return row && { ...row, protocolScript: { lock: null, type: null } };
}
