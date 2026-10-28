/** SVG text styles of the pipeline canvas (kept under components/ so Tailwind sees them). */
export const T10 = 'fill-text-dim text-[10px]';
export const T10B = 'fill-text text-[10px]';
export const T10C = 'fill-text-bright text-[10px]';
export const T10G = 'fill-gold text-[10px]';
export const T11C = 'fill-text-bright text-[11px]';
export const T11G = 'fill-gold text-[11px]';

/** The footer row (legend or pinned selection): wrap on narrow screens so every key remains visible. */
export const FOOTER_ROW =
  'flex min-h-6 flex-wrap items-center gap-y-1 font-mono md:h-6 md:flex-nowrap md:overflow-hidden md:whitespace-nowrap';

export const fmtInt = (n: number | bigint) => n.toLocaleString('en-US');
