//! Fetch decoder binaries from CKB RPC.
//!
//! Supports two on-chain reference modes:
//! - **CodeHash**: look up a known deployment by its blake2b code hash
//! - **TypeId**: query the CKB indexer RPC for a live cell with the TypeID
//!   type script and read its data

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use tracing::{debug, warn};

use crate::cache::DecoderBinaryCache;
use crate::types::DecoderRef;

/// Standard DOB/0 decoder deployment on CKB mainnet.
///
/// code_hash and the transaction where the binary is deployed.
const DOB0_CODE_HASH: &str = "13cac78ad8482202f18f9df4ea707611c35f994375fa03ae79121312dda9925c";
const DOB0_TX_HASH: &str = "71023885a2178648be6a7f138ee49379000a82cda98dd8adabee99eaaca42fde";
const DOB0_OUTPUT_INDEX: usize = 0;

/// TypeID script code_hash (all-zeros prefix + "TYPE_ID" ASCII).
const TYPE_ID_CODE_HASH: &str = "00000000000000000000000000000000000000000000000000545950455f4944";

/// Fetch (or retrieve from cache) the decoder binary referenced by `decoder_ref`.
///
/// Cache lookup is attempted first. On miss the binary is fetched from the
/// CKB chain via JSON-RPC calls to `rpc_url`, verified, cached, and returned.
pub async fn fetch_decoder_binary(
    decoder_ref: &DecoderRef,
    rpc_url: &str,
    cache: &DecoderBinaryCache,
) -> Result<Vec<u8>> {
    let cache_key = match decoder_ref {
        DecoderRef::CodeHash(hash) => DecoderBinaryCache::code_hash_key(hash),
        DecoderRef::TypeId(hash) => DecoderBinaryCache::type_id_key(hash),
    };

    // Cache hit — return immediately
    if let Some(binary) = cache.get(&cache_key) {
        debug!(key = %cache_key, "decoder binary cache hit");
        return Ok(binary);
    }

    debug!(key = %cache_key, "decoder binary cache miss, fetching from chain");

    let client = reqwest::Client::new();

    let binary = match decoder_ref {
        DecoderRef::CodeHash(hash) => fetch_by_code_hash(hash, rpc_url, &client).await?,
        DecoderRef::TypeId(hash) => fetch_by_type_id(hash, rpc_url, &client).await?,
    };

    cache
        .put(&cache_key, &binary)
        .context("failed to cache fetched decoder binary")?;

    Ok(binary)
}

/// Fetch a decoder binary by its blake2b code_hash from a known deployment.
///
/// Currently supports the standard DOB/0 decoder. For unknown code hashes
/// an error is returned instructing the user to place the binary manually
/// in the cache directory.
async fn fetch_by_code_hash(
    hash: &[u8],
    rpc_url: &str,
    client: &reqwest::Client,
) -> Result<Vec<u8>> {
    let hash_hex = hex::encode(hash);

    let (tx_hash, output_index) = match hash_hex.as_str() {
        DOB0_CODE_HASH => (DOB0_TX_HASH, DOB0_OUTPUT_INDEX),
        _ => {
            warn!(
                code_hash = %hash_hex,
                "unknown decoder code_hash — place the binary manually in the decoder cache directory"
            );
            bail!(
                "unknown decoder code_hash {hash_hex}: \
                 place the RISC-V binary in the decoder cache directory as \
                 'code_hash_{hash_hex}.bin'"
            );
        }
    };

    let data = fetch_cell_data_from_tx(tx_hash, output_index, rpc_url, client).await?;

    // Verify blake2b hash matches the expected code_hash
    verify_blake2b_hash(&data, hash)
        .context("fetched binary blake2b hash does not match expected code_hash")?;

    Ok(data)
}

/// Fetch a decoder binary by TypeID using the CKB indexer `get_cells` RPC.
///
/// Constructs a search key for a live cell whose type script is the TypeID
/// script with `args = type_id_hash`, then reads the cell data from the
/// response.
async fn fetch_by_type_id(
    type_id_hash: &[u8],
    rpc_url: &str,
    client: &reqwest::Client,
) -> Result<Vec<u8>> {
    let args_hex = format!("0x{}", hex::encode(type_id_hash));
    let code_hash_hex = format!("0x{TYPE_ID_CODE_HASH}");

    let request = json!({
        "id": 1,
        "jsonrpc": "2.0",
        "method": "get_cells",
        "params": [
            {
                "script": {
                    "code_hash": code_hash_hex,
                    "hash_type": "type",
                    "args": args_hex,
                },
                "script_type": "type",
                "with_data": true,
            },
            "asc",
            "0x1"
        ]
    });

    let response: Value = client
        .post(rpc_url)
        .json(&request)
        .send()
        .await
        .context("get_cells RPC request failed")?
        .json()
        .await
        .context("failed to parse get_cells RPC response")?;

    let cells = response["result"]["objects"]
        .as_array()
        .context("get_cells returned no objects array")?;

    if cells.is_empty() {
        bail!(
            "no live cell found for type_id {}",
            hex::encode(type_id_hash)
        );
    }

    let output_data_hex = cells[0]["output_data"]
        .as_str()
        .context("get_cells result missing output_data")?;

    decode_hex_data(output_data_hex)
}

/// Fetch a transaction by hash and extract the output data at `output_index`.
async fn fetch_cell_data_from_tx(
    tx_hash: &str,
    output_index: usize,
    rpc_url: &str,
    client: &reqwest::Client,
) -> Result<Vec<u8>> {
    let tx_hash_hex = format!("0x{tx_hash}");

    let request = json!({
        "id": 1,
        "jsonrpc": "2.0",
        "method": "get_transaction",
        "params": [tx_hash_hex]
    });

    let response: Value = client
        .post(rpc_url)
        .json(&request)
        .send()
        .await
        .context("get_transaction RPC request failed")?
        .json()
        .await
        .context("failed to parse get_transaction RPC response")?;

    let outputs_data = response["result"]["transaction"]["outputs_data"]
        .as_array()
        .context("get_transaction returned no outputs_data array")?;

    let data_hex = outputs_data
        .get(output_index)
        .and_then(|v| v.as_str())
        .with_context(|| format!("outputs_data[{output_index}] missing in tx 0x{tx_hash}"))?;

    decode_hex_data(data_hex)
}

/// Decode a "0x"-prefixed hex string into raw bytes.
fn decode_hex_data(hex_str: &str) -> Result<Vec<u8>> {
    let stripped = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    hex::decode(stripped).context("failed to decode hex data from RPC response")
}

/// Verify that the blake2b-256 hash of `data` matches `expected_hash`.
fn verify_blake2b_hash(data: &[u8], expected_hash: &[u8]) -> Result<()> {
    let mut hasher = ckb_hash::new_blake2b();
    hasher.update(data);
    let mut hash = [0u8; 32];
    hasher.finalize(&mut hash);

    if hash.as_slice() != expected_hash {
        bail!(
            "blake2b mismatch: expected {}, got {}",
            hex::encode(expected_hash),
            hex::encode(hash)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_hex_data_with_prefix() {
        let result = decode_hex_data("0xdeadbeef").unwrap();
        assert_eq!(result, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_decode_hex_data_without_prefix() {
        let result = decode_hex_data("cafebabe").unwrap();
        assert_eq!(result, vec![0xCA, 0xFE, 0xBA, 0xBE]);
    }

    #[test]
    fn test_decode_hex_data_invalid() {
        let result = decode_hex_data("0xZZZZ");
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_blake2b_hash_correct() {
        let data = b"hello world";
        let mut hasher = ckb_hash::new_blake2b();
        hasher.update(data);
        let mut expected = [0u8; 32];
        hasher.finalize(&mut expected);

        assert!(verify_blake2b_hash(data, &expected).is_ok());
    }

    #[test]
    fn test_verify_blake2b_hash_mismatch() {
        let data = b"hello world";
        let wrong_hash = [0u8; 32];

        let result = verify_blake2b_hash(data, &wrong_hash);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("blake2b mismatch"));
    }

    #[test]
    fn test_fetch_by_code_hash_unknown_returns_error() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let unknown_hash = vec![0xFF; 32];
        let client = reqwest::Client::new();

        let result = rt.block_on(fetch_by_code_hash(
            &unknown_hash,
            "http://127.0.0.1:9999",
            &client,
        ));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("unknown decoder code_hash"));
    }

    #[test]
    fn test_cache_integration_with_fetch() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DecoderBinaryCache::new(dir.path()).unwrap();

        let hash = vec![0xAB, 0xCD];
        let key = DecoderBinaryCache::code_hash_key(&hash);
        let binary = vec![1, 2, 3, 4, 5];

        // Pre-populate cache
        cache.put(&key, &binary).unwrap();

        // fetch_decoder_binary should return cached data without hitting RPC
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let result = rt.block_on(fetch_decoder_binary(
            &DecoderRef::CodeHash(hash),
            "http://127.0.0.1:9999", // unreachable, should not be called
            &cache,
        ));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), binary);
    }
}
