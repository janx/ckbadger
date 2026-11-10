/**
 * The keyboard shortcuts, one list: the command palette's help (`?`) lists them all, and the
 * status line at the foot of every page shows the ones with a `short` label as key hints.
 */
export interface ShortcutHint {
  keys: string;
  description: string;
  /** The status line's one-word name for it; only the shortcuts it shows have one. */
  short?: string;
}

export const SHORTCUT_HINTS: readonly ShortcutHint[] = [
  { keys: 'Ctrl/Cmd+K', description: 'Open command palette' },
  { keys: '?', description: 'Open keyboard shortcuts help', short: 'help' },
  { keys: '/', description: 'Focus global search bar', short: 'search' },
  { keys: 'g b', description: 'Go to Blocks', short: 'blocks' },
  { keys: 'g t', description: 'Go to Transactions' },
  { keys: 'g d', description: 'Go to DAO' },
  { keys: 'g i', description: 'Go to Inventory' },
  { keys: 'g s', description: 'Go to Scripts' },
  { keys: 'g c', description: 'Go to Charts' },
  { keys: 'g p', description: 'Go to Peers' },
  { keys: 'g h', description: 'Go to Home' },
  { keys: 'Esc', description: 'Close panel' },
];

/** The status line's key hints, in the help's order: `?` help, `/` search, `g b` blocks. */
export const STATUS_KEY_HINTS: readonly { keys: string; short: string }[] = SHORTCUT_HINTS.flatMap(
  (hint) => (hint.short === undefined ? [] : [{ keys: hint.keys, short: hint.short }])
);
