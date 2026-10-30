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

const BTCFS = /^btcfs:\/\/([0-9a-fA-F]{64})i(0|[1-9]\d*)$/;

/** Strict parse; anything but 64 hex digits, `i` and a decimal index throws. */
export function parseBtcfsUri(uri: string): BtcfsInscription {
  const match = BTCFS.exec(uri);
  if (!match) throw new Error(`Invalid BTCFS inscription URI ${JSON.stringify(uri)}`);
  const index = Number(match[2]);
  if (!Number.isSafeInteger(index))
    throw new Error(`BTCFS inscription index out of range in ${JSON.stringify(uri)}`);
  return { txid: match[1].toLowerCase(), index };
}

export function bitcoinTxHref(txid: string): string {
  if (!/^[0-9a-f]{64}$/.test(txid)) throw new Error(`Invalid Bitcoin txid ${JSON.stringify(txid)}`);
  return `https://mempool.space/tx/${txid}`;
}

/** Omiga's page for a DOB, keyed by the spore cell's type script hash. */
export function omigaDobHref(typeScriptHash: string): string {
  if (!/^0x[0-9a-f]{64}$/.test(typeScriptHash))
    throw new Error(`Invalid spore type script hash ${JSON.stringify(typeScriptHash)}`);
  return `https://omiga.io/info/dobs/${typeScriptHash}`;
}
