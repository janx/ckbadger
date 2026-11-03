/** Rows per page on a topic or list page (blocks, transactions, activities, inventory, …). */
export const DEFAULT_PAGE_SIZE = 50;

/**
 * Rows per page on an entity page (address, token, object, identity, script): the feeds and
 * holder lists under the hero. Twenty, not fifty, so the page's second tier — the evidence for
 * developers — stays within a few screens instead of ten (review 2026-11-03, item 2). A list page
 * is a list by nature and keeps {@link DEFAULT_PAGE_SIZE}; a phone reads twenty on both.
 */
export const ENTITY_PAGE_SIZE = 20;
