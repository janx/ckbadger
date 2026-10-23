/** SVG text styles of the pipeline canvas (kept under components/ so Tailwind sees them). */
export const T10 = 'fill-text-dim text-[10px]';
export const T10B = 'fill-text text-[10px]';
export const T10C = 'fill-text-bright text-[10px]';
export const T10G = 'fill-gold text-[10px]';
export const T11C = 'fill-text-bright text-[11px]';
export const T11G = 'fill-gold text-[11px]';
export const TZ = 'fill-text-dim text-[12px] uppercase tracking-[0.05em]';

/** The footer row (legend or pinned selection): one fixed height, so pinning never resizes the panel. */
export const FOOTER_ROW = 'flex h-6 items-center overflow-hidden whitespace-nowrap font-mono';

export const fmtInt = (n: number) => n.toLocaleString('en-US');
