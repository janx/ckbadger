/**
 * How a protocol id the indexer emits (`ProtocolAction::new("<id>", …)`, and the `protocol` of
 * decoded lock args) is named to people. Machine formats (the agent markdown's `<id>:<action>`
 * tokens) keep the raw id.
 */
const PROTOCOL_DISPLAY_NAMES: Record<string, string> = {
  dao: 'Nervos DAO',
  dotcell: '.cell',
  fiber: 'Fiber',
  rgbpp: 'RGB++',
  stablepp: 'Stable++',
  utxoswap: 'UTXOSwap',
};

/** The protocol's name; an id this table does not know yet is shown as the id the API gave. */
export function protocolDisplayName(id: string): string {
  return PROTOCOL_DISPLAY_NAMES[id] ?? id;
}
