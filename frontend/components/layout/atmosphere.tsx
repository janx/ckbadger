/**
 * The fixed layers behind and over every page (Midnight, continued, move 5; material pass v2,
 * move 1; visual finish V1), mounted once at the route root (AppFrame). Under the content: the
 * night sky — the ground gradient and three ambient pools in the page's hue (`.ambient-glow`,
 * z −1; the pools read the page root's `data-tint`) — and the vignette (`.crt-vignette`, z −1
 * too: rendered right after the sky, it darkens the sky toward the corners and never the ink).
 * Over the content: the CRT grain (`.crt-grain`, a 256 px fractal-noise tile screened at 3.5 %)
 * and the scanlines. All four are decoration (aria-hidden, no pointer events); the rules live in
 * app/globals.css.
 */
export function Atmosphere() {
  return (
    <>
      <div className="ambient-glow" aria-hidden="true" data-testid="atmosphere-sky" />
      <div className="crt-vignette" aria-hidden="true" />
      <div className="crt-grain" aria-hidden="true" data-testid="atmosphere-grain" />
      <div className="crt-scanlines" aria-hidden="true" />
    </>
  );
}
