export const GLOBAL_SEARCH_INPUT_SELECTOR = '[data-ckbadger-global-search="true"]';

export function isEditableElement(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;

  if (target.isContentEditable || target.closest('[contenteditable="true"]')) {
    return true;
  }

  const tagName = target.tagName.toLowerCase();
  return tagName === 'input' || tagName === 'textarea' || tagName === 'select';
}

function isVisibleElement(element: HTMLElement): boolean {
  if (element.hasAttribute('hidden') || element.getAttribute('aria-hidden') === 'true') {
    return false;
  }

  if (element instanceof HTMLInputElement && element.type === 'hidden') {
    return false;
  }

  return true;
}

export function focusGlobalSearchInput(): boolean {
  const candidates = Array.from(
    document.querySelectorAll<HTMLInputElement>(GLOBAL_SEARCH_INPUT_SELECTOR)
  );

  const target = candidates.find((input) => !input.disabled && isVisibleElement(input));
  if (!target) return false;

  target.focus();
  target.select();
  return true;
}

/**
 * TX-04: the footer's "Keyboard shortcuts" asks the command palette (mounted once, in the header)
 * for the same help the "?" key opens.
 */
export const OPEN_SHORTCUTS_EVENT = 'ckbadger:open-shortcuts';

export function openKeyboardShortcuts(): void {
  window.dispatchEvent(new Event(OPEN_SHORTCUTS_EVENT));
}
