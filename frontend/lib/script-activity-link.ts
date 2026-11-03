import type { GlobalActivityFilter } from '@/lib/api';

type ClassFilter = Exclude<GlobalActivityFilter, 'all' | 'ckb' | 'script'>;

/**
 * Lane S (S1, ruling 4): the registry families (`ScriptFamilyDetail.familyId`) whose cells are one
 * activity class, and that class's `/activities?filter=` id. `protocol` is the API's one bucket for
 * RGB++, Fiber and UTXOSwap, so its link is labelled for what it opens, not for the one protocol.
 * A family not listed here (Default Lock, JoyID and every other generic lock, CKBFS, …) gets no
 * link: the script page then shows its cells list only.
 */
export const SCRIPT_ACTIVITY_FAMILIES: ReadonlyArray<readonly [string, ClassFilter]> = [
  ['nervos-dao', 'dao'],
  ['spore', 'object'],
  ['spore-cluster', 'object'],
  ['m-nft', 'object'],
  ['m-nft-class', 'object'],
  ['xudt', 'token'],
  ['simple-udt', 'token'],
  ['bit-account', 'identity'],
  ['bit-cell', 'identity'],
  ['did-ckb', 'identity'],
  ['dotcell-account', 'identity'],
  ['rgb', 'protocol'],
  ['fiber-funding-lock', 'protocol'],
  ['fiber-commitment-lock', 'protocol'],
  ['utxoswap-intent-lock', 'protocol'],
  ['utxoswap-proxy-lock', 'protocol'],
];

const LABEL: Record<ClassFilter, string> = {
  dao: 'Activity: Nervos DAO →',
  object: 'Activity: Objects →',
  token: 'Activity: Tokens →',
  identity: 'Activity: Identities →',
  protocol: 'Protocol activity →',
};

export interface ScriptActivityLink {
  filter: ClassFilter;
  label: string;
  href: string;
}

/** The activity feed a script family's cells belong to; null for a family mapped to none. */
export function scriptActivityLink(familyId: string): ScriptActivityLink | null {
  const entry = SCRIPT_ACTIVITY_FAMILIES.find(([id]) => id === familyId);
  if (!entry) return null;
  const filter = entry[1];
  return { filter, label: LABEL[filter], href: `/activities?filter=${filter}` };
}
