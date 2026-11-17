/** SVG text styles of the pipeline canvas (kept under components/ so Tailwind sees them). */
export const T10 = 'fill-text-dim text-meta';
export const T10B = 'fill-text text-meta';
export const T10C = 'fill-text-bright text-meta';
export const T10G = 'fill-gold text-meta';
export const T11C = 'fill-text-bright text-meta';
/**
 * A block number inside the drawn window, on its plate: the pipeline's word gold, the plain gold by
 * night and the gold's bright step by day (6.19 / 5.32 : 1 on the plate, R4).
 */
export const T11G = 'fill-[var(--pipeline-word-gold)] text-meta';

/** The line under the canvas (legend or pinned selection): its content wraps whenever it is narrow. */
export const FOOTER_ROW = 'flex min-h-6 flex-wrap items-center gap-y-1 font-mono';

export const fmtInt = (n: number | bigint) => n.toLocaleString('en-US');
