/**
 * The page ends on an empty prompt (rubbing × Song print, phase 2): the cinnabar square and the
 * celadon block cursor after the last output — the session waits for the next command. Decoration
 * only (aria-hidden); the cursor's blink stops under reduced motion (globals.css `.cursor`).
 */
export function PromptEnd() {
  return (
    <div
      aria-hidden="true"
      data-testid="prompt-end"
      className="container mx-auto flex items-center gap-2.5 pb-10 pt-6"
    >
      <span className="prompt-square" />
      <span className="cursor" />
    </div>
  );
}
