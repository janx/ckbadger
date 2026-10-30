/**
 * `btcfs://<txid>i<n>` names a DOB-protocol inscription: the n-th envelope (tag "DOB", not the
 * ordinals "ord") in the witness of Bitcoin transaction <txid>. Browsers cannot open the scheme,
 * so the only readable target is the Bitcoin transaction itself.
 */
export interface BtcfsInscription {
  /** 32-byte Bitcoin txid, lowercase hex without a prefix (the form Bitcoin explorers use). */
  txid: string;
  index: number;
}

// Case-insensitive like the indexer, which lowercases media text before matching schemes.
const BTCFS = /^btcfs:\/\/([0-9a-f]{64})i(0|[1-9]\d*)$/i;

/**
 * Strict parse of exactly `btcfs://<64 hex txid>i<decimal index>`. The URI comes from spore
 * content anyone can mint, so anything else is third-party data we cannot read, not a broken
 * invariant of ours: it returns null and the caller shows the raw text without a link.
 */
export function parseBtcfsUri(uri: string): BtcfsInscription | null {
  const match = BTCFS.exec(uri);
  if (!match) return null;
  const index = Number(match[2]);
  if (!Number.isSafeInteger(index)) return null;
  return { txid: match[1].toLowerCase(), index };
}

/**
 * The inscription's Bitcoin transaction on mempool.space, for mainnet objects. Which Bitcoin
 * network testnet DOB inscriptions name is not established, so other networks get no link (null).
 */
export function bitcoinTxHref(txid: string, network: string): string | null {
  if (!/^[0-9a-f]{64}$/.test(txid)) throw new Error(`Invalid Bitcoin txid ${JSON.stringify(txid)}`);
  return network === 'mainnet' ? `https://mempool.space/tx/${txid}` : null;
}

const OMIGA_HOSTS: Record<string, string> = {
  mainnet: 'https://omiga.io',
  testnet: 'https://test.omiga.io',
};

/** Omiga's page for a DOB on the page's network, keyed by the spore cell's type script hash. */
export function omigaDobHref(typeScriptHash: string, network: string): string | null {
  if (!/^0x[0-9a-f]{64}$/.test(typeScriptHash))
    throw new Error(`Invalid spore type script hash ${JSON.stringify(typeScriptHash)}`);
  const host = OMIGA_HOSTS[network];
  return host ? `${host}/info/dobs/${typeScriptHash}` : null;
}
