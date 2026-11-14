/** SVG text styles of the pipeline canvas (kept under components/ so Tailwind sees them). */
export const T10 = 'fill-text-dim text-meta';
export const T10B = 'fill-text text-meta';
export const T10C = 'fill-text-bright text-meta';
export const T10G = 'fill-gold text-meta';
export const T11C = 'fill-text-bright text-meta';
/**
 * A block number inside the drawn window, on its tile: the gold's bright step (more ink by day,
 * more light by night), which reads at 4.5:1 on the tile where plain gold is 4.15:1 by day (R4).
 */
export const T11G = 'fill-warning-bright text-meta';

/** The footer row (legend or pinned selection): let content wrap whenever its panel is narrow. */
export const FOOTER_ROW = 'flex min-h-6 flex-wrap items-center gap-y-1 font-mono';

export const fmtInt = (n: number | bigint) => n.toLocaleString('en-US');
