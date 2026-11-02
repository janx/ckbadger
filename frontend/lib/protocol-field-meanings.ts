import type { ItemProtocol } from '@/lib/api';
import type { ProtocolEvidenceField } from '@/components/asset/protocol-evidence';

/** Every protocol whose cell data the `/protocol` routes split into fields. */
export type ProtocolFieldKind = ItemProtocol['kind'] | 'spore_cluster';

/**
 * One sentence per field the API's `/protocol` routes emit (lane E, C5: the detail box's meaning),
 * from each protocol's layout as the indexer's parsers read it (`crates/indexer/src/parser/`:
 * spore, mnft, dotbit, bit_cell, did_ckb, dotcell). The byte facts (lengths, text) are read from
 * the bytes themselves by `fieldShape`; these sentences say only what the protocol defines.
 */
export const PROTOCOL_FIELD_MEANINGS: Record<
  ProtocolFieldKind,
  Readonly<Record<string, string>>
> = {
  spore_cluster: {
    name: 'The cluster’s name.',
    description:
      'The cluster’s description as its creator wrote it; a DOB cluster keeps its pattern here.',
    mutantId: 'The mutant every object minted into the cluster must also run; empty means none.',
  },
  spore: {
    contentType: 'The media type of the content.',
    content: 'The object’s content: the media itself, or the DNA a DOB decoder reads.',
    clusterId: 'The cluster the object belongs to; empty means none.',
  },
  'm-nft': {
    version: 'Layout version of the token data.',
    characteristic: 'Eight bytes the class may give each token as its traits.',
    configure: 'Permission bits; a zero bit allows the operation it names.',
    state: 'State bits: bit 0 is claimed, bit 1 is locked.',
  },
  dotbit: {
    accountDataHash: 'Hash of the account’s data, which the transaction’s witness carries.',
    accountId: 'The account’s id.',
    nextAccountId: 'The id of the next account in .bit’s ordered list of accounts.',
    expiredAt: 'When the account expires, in Unix seconds, little-endian.',
    account: 'The account’s name.',
  },
  bit_cell: {
    prefix: 'The first byte of the content layout, 0.',
    version: 'The content layout version, 1.',
    witnessHash: 'Hash of the witness that carries the account data; not the account id.',
    expiredAt: 'When the account expires, in Unix seconds, little-endian.',
    account: 'The account’s name.',
  },
  did_ckb: {
    union: 'The molecule union id: which version of the DID data follows.',
    document: 'The DID document, CBOR-encoded.',
  },
  dotcell: {
    layoutVersion: 'Layout version of the name data.',
    recordsHash: 'Blake2b of the records, which the transaction’s witness carries.',
    nextId: 'The next name’s id in the ordered ring; zero ends the ring.',
    expiredAt: 'When the name expires, in Unix seconds, five bytes little-endian.',
    owner: 'The first 20 bytes of the owner’s lock hash.',
    manager: 'The first 20 bytes of the manager’s lock hash.',
    label: 'The name without “.cell”.',
  },
};

/** The API's fields with their protocol meaning attached; a field it does not know keeps none. */
export function withFieldMeanings(
  kind: ProtocolFieldKind,
  fields: readonly Omit<ProtocolEvidenceField, 'meaning'>[]
): ProtocolEvidenceField[] {
  return fields.map((field) => {
    const meanings = PROTOCOL_FIELD_MEANINGS[kind];
    return Object.hasOwn(meanings, field.name)
      ? { ...field, meaning: meanings[field.name] }
      : { ...field };
  });
}
