import type { ActivityProtocolAction, CollectionFeedContext, ItemFeedContext } from '@/lib/api';

/** `.bit` ActionData actions in the identity boards' words (IdentityBit, ID-C1 / ID-C4). */
export const DOTBIT_ACTION_HEADLINES: Record<string, string> = {
  confirm_proposal: 'Registered',
  renew_account: 'Renewed',
  edit_records: 'Records edited',
  edit_manager: 'Manager changed',
  transfer_account: 'Transferred',
  enable_sub_account: 'Sub-accounts enabled',
  recycle_expired_account: 'Recycled',
};

const NEIGHBOUR_HEADLINES: Record<NonNullable<ItemFeedContext['ringRelink']>['by'], string> = {
  register: 'Neighbour registered',
  register_subname: 'Neighbour registered a sub-name',
  recycle: 'Neighbour recycled',
  unattributed: 'Ring pointer changed',
};

/** A `.cell` action in words; `ring_relink` names the neighbour whose pointer changed. */
export function dotcellActionLabel(action: string, name: string | null = null): string {
  if (action === 'ring_relink')
    return name ? `ring pointer changed for ${name}` : 'ring pointer changed';
  return action.replace(/_/g, ' ');
}

export interface ItemFeedHeadline {
  text: string;
  /** Protocol actions with no board wording, shown verbatim in mono (visible, never hidden). */
  raw?: string[];
  /** Someone else's action that only rewrote this item's cell (API `actedOn`): dimmed. */
  neighbour: boolean;
}

/** Which item a feed row belongs to: its kind, and how its protocol actions are named. */
export interface ItemFeedSubject {
  kind: 'identity' | 'object';
  /** `.cell`: rows are named by the item's own bare protocol actions. */
  protocolName?: string;
  /** `.bit`: rows are named by their `dotbit:` actions in the board's words. */
  protocol?: 'dotbit';
}

const sentence = (labels: string[]) =>
  labels.map((label, i) => (i === 0 ? label : label.charAt(0).toLowerCase() + label.slice(1)));

function dotbitHeadline(actions: string[], prefix: string): Pick<ItemFeedHeadline, 'text' | 'raw'> {
  const known: string[] = [];
  const raw: string[] = [];
  for (const action of actions) {
    const label = DOTBIT_ACTION_HEADLINES[action];
    if (label) known.push(label);
    else raw.push(`dotbit:${action}`);
  }
  const words = sentence(prefix ? [prefix, ...known] : known);
  const text = prefix ? `${words[0]} ${words.slice(1).join(' · ')}`.trim() : words.join(' · ');
  return raw.length ? { text, raw } : { text };
}

/**
 * Headline of a row in one item's own feed. The API states what the tx meant for this item
 * (`itemContext`): its own protocol actions, a ring relink, and whose action it was (`actedOn`).
 * This only names them (ID-C1, ID-C4, OB-C14); `txActions` are the tx's protocol actions, read
 * only to name what a `.bit` neighbour did.
 */
export function itemFeedHeadline(
  context: ItemFeedContext | undefined,
  txActions: readonly ActivityProtocolAction[] | null,
  txHash: string,
  item: ItemFeedSubject
): ItemFeedHeadline {
  if (!context?.action)
    throw new Error(`${txHash}: item feed row has no item action for ${item.kind}`);
  if (context.actedOn !== 'self' && context.actedOn !== 'neighbour')
    throw new Error(`${txHash}: item feed row has no actedOn`);
  const neighbour = context.actedOn === 'neighbour';
  if (context.ringRelink) {
    const text = NEIGHBOUR_HEADLINES[context.ringRelink.by];
    if (!text)
      throw new Error(`${txHash}: unknown ring relink ${JSON.stringify(context.ringRelink.by)}`);
    return { text, neighbour };
  }
  if (item.protocol === 'dotbit') {
    if (neighbour) {
      if (txActions === null)
        throw new Error(`${txHash}: neighbour .bit row has no transaction activity to name`);
      const actions = [
        ...new Set(txActions.filter((pa) => pa.protocol === 'dotbit').map((pa) => pa.action)),
      ];
      if (!actions.length)
        throw new Error(`${txHash}: neighbour .bit row has no .bit action in its transaction`);
      return { ...dotbitHeadline(actions, 'Neighbour'), neighbour };
    }
    if (context.protocolActions.length) {
      const actions = context.protocolActions.map((entry) => {
        if (!entry.startsWith('dotbit:'))
          throw new Error(`${txHash}: .bit row carries a non-.bit action ${JSON.stringify(entry)}`);
        return entry.slice('dotbit:'.length);
      });
      return { ...dotbitHeadline(actions, ''), neighbour };
    }
  }
  if (item.protocolName && context.protocolActions.length)
    return {
      text: `${item.protocolName} · ${context.protocolActions.map((action) => dotcellActionLabel(action)).join(', ')}`,
      neighbour,
    };
  return {
    text: `${item.kind === 'identity' ? 'Identity' : 'Object'} ${context.action.toLowerCase()}`,
    neighbour,
  };
}

const COLLECTION_KINDS = [
  ['minted', 'mint', 'Minted'],
  ['transferred', 'transfer', 'Transferred'],
  ['updated', 'update', 'Updated'],
  ['burned', 'burn', 'Burned'],
] as const;

/**
 * Headline of a collection feed row from the API's `collectionContext` (OB-C13): what the tx did
 * to this collection's items only — "Minted 1", "Transferred 12 · burned 1".
 */
export function collectionFeedHeadline(context: CollectionFeedContext | undefined, txHash: string) {
  if (!context) throw new Error(`${txHash}: collection feed row has no collection context`);
  const present = COLLECTION_KINDS.filter(([count]) => {
    const n = context[count];
    if (!Number.isSafeInteger(n) || n < 0)
      throw new Error(`${txHash}: invalid ${count} count ${JSON.stringify(n)}`);
    return n > 0;
  });
  const expected = present.length === 1 ? present[0][1] : present.length > 1 ? 'mixed' : null;
  if (expected === null || context.action !== expected)
    throw new Error(`${txHash}: collection action "${context.action}" disagrees with its counts`);
  for (const [count] of COLLECTION_KINDS) {
    const listed = context.items.filter((item) => item.change === count).length;
    if (listed !== context[count])
      throw new Error(`${txHash}: ${listed} ${count} item listed for a count of ${context[count]}`);
  }
  return sentence(present.map(([count, , word]) => `${word} ${context[count]}`)).join(' · ');
}
