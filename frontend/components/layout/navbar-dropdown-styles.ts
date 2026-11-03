/** A header nav link or dropdown trigger: sentence case, normal tracking, a tall hit area. */
export const NAVBAR_LINK_CLASS = 'py-3 font-mono text-body transition-colors';

export const NAVBAR_LINK_DEFAULT_CLASS = 'text-text hover:text-jade';

/** TX-02: the current page reads bright (with aria-current); jade is the hover colour only. */
export const NAVBAR_LINK_ACTIVE_CLASS = 'text-text-bright';

export const NAVBAR_DROPDOWN_TRIGGER_CLASS =
  'flex items-center gap-1 rounded-md border px-2 py-1.5 font-mono text-small uppercase tracking-[0.12em] transition';

/**
 * Lane U (U4): a nav entry's menu (Inventory), in the network switcher's panel materials and the
 * nav links' inks — no new colours.
 */
export const NAVBAR_MENU_PANEL_CLASS =
  'border-base-border bg-base-surface min-w-[11rem] rounded-md border p-1.5 shadow-lg';

export const NAVBAR_MENU_ITEM_CLASS =
  'block whitespace-nowrap rounded-sm px-2.5 py-2 font-mono text-body transition-colors hover:bg-base-elevated focus-visible:bg-base-elevated focus-visible:text-jade focus-visible:outline-none';

export const NAVBAR_MENU_ITEM_DEFAULT_CLASS = 'text-text hover:text-jade';

/** The page the menu item opens is the current page: bright, as a current nav link reads. */
export const NAVBAR_MENU_ITEM_ACTIVE_CLASS = 'text-text-bright';
