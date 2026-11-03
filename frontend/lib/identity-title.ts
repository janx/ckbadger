import type { ItemKind, ItemProtocol } from '@/lib/api';
import { readDidDocument, splitHandle } from '@/lib/did-document';
import { identityNameParts } from '@/components/identity/identity-header';

/** The four identity item kinds, as the item read paths name them. */
export type IdentityItemKind = Extract<ItemKind, 'dotbit' | 'bit-cell' | 'did' | 'dotcell'>;

type IdentityItemProtocol = Exclude<ItemProtocol, { kind: 'spore' } | { kind: 'm-nft' }>;

/**
 * An identity's name as its page titles it (ID-C10, ID-D1): `.bit` / `.bit Cell` by the latest
 * version's decoded account, did:ckb by the first handle of its DID document (else the DID), `.cell`
 * by its name; `name` and the standard's `suffix` apart, `full` the two together. `title` is the
 * name the caller already has, used until the protocol read arrives (and always for `.cell`). The
 * one naming path for the identity page and the cell page's inventory line (lane U, U6).
 */
export function identityTitleParts(
  kind: IdentityItemKind,
  title: string,
  protocol: IdentityItemProtocol | undefined
): { name: string; suffix: string; full: string } {
  if (kind === 'did') {
    const last = protocol?.kind === 'did_ckb' ? protocol.versions.at(-1)!.decoded : undefined;
    const handle = last ? readDidDocument(last.document).handles[0] : undefined;
    if (handle) return { ...splitHandle(handle), full: handle };
    return { name: last?.did ?? title, suffix: '', full: last?.did ?? title };
  }
  const account =
    protocol?.kind === 'dotbit' || protocol?.kind === 'bit_cell'
      ? protocol.versions.at(-1)!.decoded.account
      : title;
  const parts = identityNameParts(account, kind === 'dotcell' ? '.cell' : '.bit');
  return { ...parts, full: account };
}
