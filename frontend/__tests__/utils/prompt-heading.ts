import { expect } from 'vitest';

/**
 * Rubbing × Song print, phase 5: a section heading written as a command line
 * (`PromptHeading { command, args, note }`). Asserts the h2 is a tier-1 prompt whose command and
 * args are the ones given (each its own span, the args dim), and returns them.
 */
export function expectPrompt(
  heading: HTMLElement,
  command: string | RegExp,
  args?: string | RegExp
): { command: HTMLElement; args: HTMLElement | undefined } {
  expect(heading).toHaveClass('prompt', 'text-section');
  const [commandEl, argsEl] = Array.from(heading.children) as HTMLElement[];
  expect(commandEl).toHaveTextContent(
    typeof command === 'string' ? new RegExp(`^${escape(command)}$`) : command
  );
  if (args === undefined) {
    expect(argsEl).toBeUndefined();
    return { command: commandEl, args: undefined };
  }
  expect(argsEl).toHaveClass('text-text-dim', 'font-normal', 'normal-case');
  expect(argsEl).toHaveTextContent(
    typeof args === 'string' ? new RegExp(`^${escape(args)}$`) : args
  );
  return { command: commandEl, args: argsEl };
}

/** The heading's right-prompt note: beside the heading in its row, never inside its name. */
export function promptNote(heading: HTMLElement): HTMLElement {
  const note = heading.parentElement?.querySelector<HTMLElement>(':scope > [data-prompt-note]');
  if (!note) throw new Error(`no right-prompt note beside "${heading.textContent}"`);
  expect(heading).not.toContainElement(note);
  return note;
}

function escape(s: string) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
