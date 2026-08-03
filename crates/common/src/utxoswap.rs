//! UTXOSwap intent-lock args decoding.
//!
//! This is the SINGLE decode path for UTXOSwap intent lock args. Both the
//! indexer's protocol detector (persisted activity metadata) and the API's
//! live `lockCalls` display decode through here, so the two can never
//! disagree about a field's layout or its name.
//!
//! # Args layout
//!
//! All intent types share a fixed 57-byte header, then carry a type-specific
//! payload. The header is confirmed by the UTXOSwap SDK's own intent-args
//! builders (`calculateCreatePoolIntentArgs` / `calculateSwapTokenIntentArgs`):
//!
//! ```text
//! [0..20]   owner_lock_hash   (20-byte prefix of the owner's lock script hash)
//! [20..40]  pool_type_hash    (20-byte prefix of the pool's type script hash)
//! [40..48]  tx_fee            (unused here)
//! [48..56]  expire_batch_id   (unused here)
//! [56]      intent_type
//! ```
//!
//! The per-type payloads, verified byte-exactly against mainnet transactions:
//!
//! ```text
//! type 0 CreatePool       (154 B)  [57] fee_rate, [58..90] asset_x,
//!                                  [90..122] asset_y, [122..138] amount_x,
//!                                  [138..154] amount_y
//! type 1 AddLiquidity     (121 B)  4×u128 LE at [57..121]:
//!                                  desired_x, min_x, desired_y, min_y
//! type 2 RemoveLiquidity  (105 B)  3×u128 LE at [57..105]:
//!                                  lp_amount, min_x, min_y
//! type 3/4 swaps           (90 B)  [57] asset_in_index,
//!                                  [58..74] amount_in, [74..90] amount_out_min
//! ```
//!
//! The AddLiquidity and RemoveLiquidity field identities are not guesses — they
//! were read back off-chain from the cells the intents actually produce:
//!
//! - AddLiquidity tx `0x18d1b37ea5a3e83a9a58cb11ec164fb161b4d29f66543c69786a2108f62e7684`
//!   (block 14,046,271) has u128s `[9969978, 9920128, 224336, 223214]`. Its
//!   intent output 0 is a bare-CKB intent cell of capacity 24,309,969,978 —
//!   exactly the 24,300,000,000 base plus **9,969,978** (= u128[0]) — and its
//!   intent output 1 holds UDT amount **224,336** (= u128[2]). So u128[0] is
//!   the X-asset (CKB) leg and u128[2] the Y-asset leg, each immediately
//!   followed by its slippage floor 0.5% below.
//! - RemoveLiquidity tx `0x416ed0a39468cf54179f23aa25626a92ee8fdb5117c8418545d4e0bb8cf53a7e`
//!   (block 20,003,047) has u128s `[52147210375003, 5728619911607, 516029247141147]`
//!   and its intent output 0 holds LP-token amount **52,147,210,375,003**
//!   (= u128[0]), confirming the LP burn amount leads the payload.
//!
//! A payload whose length does not match its intent type is recorded as
//! [`IntentPayload::Unparsed`]. It never borrows another type's layout: doing
//! so is what previously turned every AddLiquidity/RemoveLiquidity intent into
//! 2^127-scale garbage amounts.

/// Byte offset of the intent-type discriminant, and the minimum args length
/// needed to read the shared header.
pub const INTENT_ARGS_HEADER_LEN: usize = 57;

/// Exact args length for a `CreatePool` intent.
pub const CREATE_POOL_ARGS_LEN: usize = 154;
/// Exact args length for an `AddLiquidity` intent.
pub const ADD_LIQUIDITY_ARGS_LEN: usize = 121;
/// Exact args length for a `RemoveLiquidity` intent.
pub const REMOVE_LIQUIDITY_ARGS_LEN: usize = 105;
/// Exact args length for either swap intent.
pub const SWAP_ARGS_LEN: usize = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IntentType {
    CreatePool = 0,
    AddLiquidity = 1,
    RemoveLiquidity = 2,
    SwapExactInputForOutput = 3,
    SwapInputForExactOutput = 4,
    ClaimProtocolLiquidity = 5,
}

impl IntentType {
    pub fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::CreatePool,
            1 => Self::AddLiquidity,
            2 => Self::RemoveLiquidity,
            3 => Self::SwapExactInputForOutput,
            4 => Self::SwapInputForExactOutput,
            5 => Self::ClaimProtocolLiquidity,
            _ => return None,
        })
    }

    /// Snake-case identifier used to build persisted action names
    /// (`<action_name>_submitted` / `<action_name>_settled`).
    pub fn action_name(&self) -> &'static str {
        match self {
            Self::CreatePool => "create_pool",
            Self::AddLiquidity => "add_liquidity",
            Self::RemoveLiquidity => "remove_liquidity",
            Self::SwapExactInputForOutput => "swap_exact_input",
            Self::SwapInputForExactOutput => "swap_exact_output",
            Self::ClaimProtocolLiquidity => "claim_protocol_liquidity",
        }
    }

    /// Human-readable name surfaced in activity metadata.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::CreatePool => "CreatePool",
            Self::AddLiquidity => "AddLiquidity",
            Self::RemoveLiquidity => "RemoveLiquidity",
            Self::SwapExactInputForOutput => "SwapExactInputForOutput",
            Self::SwapInputForExactOutput => "SwapInputForExactOutput",
            Self::ClaimProtocolLiquidity => "ClaimProtocolLiquidity",
        }
    }

    /// Exact payload length this intent type is known to carry, or `None` when
    /// the on-chain layout has never been observed (`ClaimProtocolLiquidity`).
    pub fn expected_args_len(&self) -> Option<usize> {
        Some(match self {
            Self::CreatePool => CREATE_POOL_ARGS_LEN,
            Self::AddLiquidity => ADD_LIQUIDITY_ARGS_LEN,
            Self::RemoveLiquidity => REMOVE_LIQUIDITY_ARGS_LEN,
            Self::SwapExactInputForOutput | Self::SwapInputForExactOutput => SWAP_ARGS_LEN,
            // Never seen on either network; guessing a layout would fabricate
            // numbers, so this type always decodes to `Unparsed`.
            Self::ClaimProtocolLiquidity => return None,
        })
    }
}

/// Type-specific intent payload. Each variant names only fields that genuinely
/// exist in that intent's on-chain layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntentPayload {
    CreatePool {
        total_fee_rate: u8,
        asset_x: [u8; 32],
        asset_y: [u8; 32],
        amount_x: u128,
        amount_y: u128,
    },
    AddLiquidity {
        desired_x: u128,
        min_x: u128,
        desired_y: u128,
        min_y: u128,
    },
    RemoveLiquidity {
        lp_amount: u128,
        min_x: u128,
        min_y: u128,
    },
    Swap {
        asset_in_index: u8,
        amount_in: u128,
        amount_out_min: u128,
    },
    /// The header parsed, but the payload layout is unknown for this intent
    /// type or the args length did not match it. No amounts are reported.
    Unparsed { args_len: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedIntentArgs {
    /// 20-byte prefix of the intent owner's lock script hash.
    pub owner_lock_hash: [u8; 20],
    /// 20-byte prefix of the pool's type script hash.
    pub pool_type_hash: [u8; 20],
    pub intent_type: IntentType,
    pub payload: IntentPayload,
}

/// Read a little-endian `u128` at `offset`. The caller has already length-checked
/// the buffer, so the fixed-size conversion cannot fail.
fn u128_le(args: &[u8], offset: usize) -> u128 {
    let bytes: [u8; 16] = args[offset..offset + 16]
        .try_into()
        .expect("u128 window is in bounds: caller length-checked the args");
    u128::from_le_bytes(bytes)
}

/// Read a 32-byte asset hash at `offset`.
fn hash32(args: &[u8], offset: usize) -> [u8; 32] {
    args[offset..offset + 32]
        .try_into()
        .expect("32-byte window is in bounds: caller length-checked the args")
}

/// Parse UTXOSwap intent lock args.
///
/// Returns `None` only when the args cannot carry the shared header or the
/// intent-type byte is not a known type. A recognised type whose payload length
/// is unexpected still yields the header with [`IntentPayload::Unparsed`],
/// because the header offsets are type-independent.
pub fn parse_intent_args(args: &[u8]) -> Option<ParsedIntentArgs> {
    if args.len() < INTENT_ARGS_HEADER_LEN {
        return None;
    }

    let intent_type = IntentType::from_byte(args[56])?;

    let owner_lock_hash: [u8; 20] = args[0..20]
        .try_into()
        .expect("20-byte owner window is in bounds: header length checked above");
    let pool_type_hash: [u8; 20] = args[20..40]
        .try_into()
        .expect("20-byte pool window is in bounds: header length checked above");

    // [40..48] tx_fee and [48..56] expire_batch_id are not surfaced.

    let length_matches = intent_type.expected_args_len() == Some(args.len());
    let payload = if !length_matches {
        tracing::warn!(
            intent_type = intent_type.display_name(),
            args_len = args.len(),
            expected_len = ?intent_type.expected_args_len(),
            "UTXOSwap intent args length does not match the known layout; \
             recording payload as unparsed instead of guessing a layout"
        );
        IntentPayload::Unparsed {
            args_len: args.len(),
        }
    } else {
        match intent_type {
            IntentType::CreatePool => IntentPayload::CreatePool {
                total_fee_rate: args[57],
                asset_x: hash32(args, 58),
                asset_y: hash32(args, 90),
                amount_x: u128_le(args, 122),
                amount_y: u128_le(args, 138),
            },
            IntentType::AddLiquidity => IntentPayload::AddLiquidity {
                desired_x: u128_le(args, 57),
                min_x: u128_le(args, 73),
                desired_y: u128_le(args, 89),
                min_y: u128_le(args, 105),
            },
            IntentType::RemoveLiquidity => IntentPayload::RemoveLiquidity {
                lp_amount: u128_le(args, 57),
                min_x: u128_le(args, 73),
                min_y: u128_le(args, 89),
            },
            IntentType::SwapExactInputForOutput | IntentType::SwapInputForExactOutput => {
                IntentPayload::Swap {
                    asset_in_index: args[57],
                    amount_in: u128_le(args, 58),
                    amount_out_min: u128_le(args, 74),
                }
            }
            // `expected_args_len()` is `None` here, so `length_matches` is
            // false and this arm is unreachable.
            IntentType::ClaimProtocolLiquidity => IntentPayload::Unparsed {
                args_len: args.len(),
            },
        }
    };

    Some(ParsedIntentArgs {
        owner_lock_hash,
        pool_type_hash,
        intent_type,
        payload,
    })
}

fn hex0x(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

impl ParsedIntentArgs {
    /// Metadata object shared by persisted activity metadata and the API's
    /// live lock-args display. Only fields that exist in this intent's layout
    /// are emitted — u128 amounts are stringified to survive JSON.
    pub fn metadata_json(&self) -> serde_json::Value {
        let mut meta = serde_json::json!({
            "intentType": self.intent_type.display_name(),
            "poolTypeHash": hex0x(&self.pool_type_hash),
        });

        match &self.payload {
            IntentPayload::CreatePool {
                total_fee_rate,
                asset_x,
                asset_y,
                amount_x,
                amount_y,
            } => {
                meta["totalFeeRate"] = serde_json::json!(total_fee_rate);
                meta["assetX"] = serde_json::json!(hex0x(asset_x));
                meta["assetY"] = serde_json::json!(hex0x(asset_y));
                meta["amountX"] = serde_json::json!(amount_x.to_string());
                meta["amountY"] = serde_json::json!(amount_y.to_string());
            }
            IntentPayload::AddLiquidity {
                desired_x,
                min_x,
                desired_y,
                min_y,
            } => {
                meta["desiredX"] = serde_json::json!(desired_x.to_string());
                meta["minX"] = serde_json::json!(min_x.to_string());
                meta["desiredY"] = serde_json::json!(desired_y.to_string());
                meta["minY"] = serde_json::json!(min_y.to_string());
            }
            IntentPayload::RemoveLiquidity {
                lp_amount,
                min_x,
                min_y,
            } => {
                meta["lpAmount"] = serde_json::json!(lp_amount.to_string());
                meta["minX"] = serde_json::json!(min_x.to_string());
                meta["minY"] = serde_json::json!(min_y.to_string());
            }
            IntentPayload::Swap {
                asset_in_index,
                amount_in,
                amount_out_min,
            } => {
                meta["assetInIndex"] = serde_json::json!(asset_in_index);
                meta["amountIn"] = serde_json::json!(amount_in.to_string());
                meta["amountOutMin"] = serde_json::json!(amount_out_min.to_string());
            }
            IntentPayload::Unparsed { args_len } => {
                meta["payloadUnparsed"] = serde_json::json!(true);
                meta["argsLen"] = serde_json::json!(args_len);
            }
        }

        meta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(hex_str: &str) -> Vec<u8> {
        hex::decode(hex_str.strip_prefix("0x").unwrap_or(hex_str)).expect("valid hex vector")
    }

    // --- Real captured mainnet vectors -----------------------------------
    // Byte-exact intent lock args pulled from a local CKB node. Each one is
    // cross-checked against the cells the intent actually produced (see the
    // module docs), so these pin field identity, not just field position.

    /// AddLiquidity submit, tx 0x18d1b37e…, mainnet block 14,046,271.
    const REAL_ADD_LIQUIDITY: &str = "0x0001d85947f67df16556a1caef3b7f939a69fb2329273406698f36e9bdf46db404176859b0ba3a6b00000000000000000000000000000000013a219800000000000000000000000000805e9700000000000000000000000000506c0300000000000000000000000000ee670300000000000000000000000000";

    /// RemoveLiquidity submit, tx 0x416ed0a3…, mainnet block 20,003,047.
    const REAL_REMOVE_LIQUIDITY: &str = "0xc41696293f5b16b471f9116631da82a4102c5b01b82e9073fee07b9caf625f0be45d3ec061be221200000000000000000100000000000000025b4ff3776d2f00000000000000000000b7d95acc3505000000000000000000001b35f86b53d501000000000000000000";

    /// SwapExactInputForOutput submit, tx 0x44f659be…, mainnet block 13,845,652.
    const REAL_SWAP: &str = "0xbefc0a6053441e9bcba6d3f6c1599c37a1d8187a235edb927fc68f446e06f2e677fb52aa7f158ae800000000000000000000000000000000030180ea822b000000000000000000000000324f4251220000000000000000000000";

    /// CreatePool submit, tx 0xdd8b76a8…, mainnet block 13,372,041.
    const REAL_CREATE_POOL: &str = "0x4d93a976fd4eb7a6349c020fadc3ef65834701dc000000000000000000000000000000000000000000000000000000000100000000000000001e000000000000000000000000000000000000000000000000000000000000000061bd91b121e5b7bbf9ccb4bc46c3106ac69c2dfd7b1c1143c4b4fdb33fd6182600e40b5402000000000000000000000000e40b54020000000000000000000000";

    #[test]
    fn real_add_liquidity_vector_has_four_u128_payload() {
        let args = bytes(REAL_ADD_LIQUIDITY);
        assert_eq!(args.len(), ADD_LIQUIDITY_ARGS_LEN);
        let parsed = parse_intent_args(&args).expect("real vector must parse");

        assert_eq!(parsed.intent_type, IntentType::AddLiquidity);
        assert_eq!(
            hex0x(&parsed.pool_type_hash),
            "0x29273406698f36e9bdf46db404176859b0ba3a6b"
        );
        assert_eq!(
            hex0x(&parsed.owner_lock_hash),
            "0x0001d85947f67df16556a1caef3b7f939a69fb23"
        );
        assert_eq!(
            parsed.payload,
            IntentPayload::AddLiquidity {
                // X leg: intent output 0's capacity above the 243-CKB base.
                desired_x: 9_969_978,
                min_x: 9_920_128,
                // Y leg: intent output 1's UDT amount.
                desired_y: 224_336,
                min_y: 223_214,
            }
        );

        // Each slippage floor sits ~0.5% (50 bps) below its desired amount —
        // this adjacency is what identifies the pairs as (desired, min) per
        // asset rather than (desired_x, desired_y) followed by (min_x, min_y).
        for (desired, min) in [(9_969_978u128, 9_920_128u128), (224_336, 223_214)] {
            let bps = (desired - min) * 10_000 / desired;
            assert!(
                (45..=55).contains(&bps),
                "expected a ~50 bps slippage floor, got {bps} bps ({desired} -> {min})"
            );
        }
    }

    #[test]
    fn real_remove_liquidity_vector_has_three_u128_payload() {
        let args = bytes(REAL_REMOVE_LIQUIDITY);
        assert_eq!(args.len(), REMOVE_LIQUIDITY_ARGS_LEN);
        let parsed = parse_intent_args(&args).expect("real vector must parse");

        assert_eq!(parsed.intent_type, IntentType::RemoveLiquidity);
        assert_eq!(
            hex0x(&parsed.pool_type_hash),
            "0xb82e9073fee07b9caf625f0be45d3ec061be2212"
        );
        assert_eq!(
            parsed.payload,
            IntentPayload::RemoveLiquidity {
                // Matches the LP-token amount held by intent output 0.
                lp_amount: 52_147_210_375_003,
                min_x: 5_728_619_911_607,
                min_y: 516_029_247_141_147,
            }
        );
    }

    #[test]
    fn real_swap_vector_keeps_the_swap_layout() {
        let args = bytes(REAL_SWAP);
        assert_eq!(args.len(), SWAP_ARGS_LEN);
        let parsed = parse_intent_args(&args).expect("real vector must parse");

        assert_eq!(parsed.intent_type, IntentType::SwapExactInputForOutput);
        assert_eq!(
            hex0x(&parsed.pool_type_hash),
            "0x235edb927fc68f446e06f2e677fb52aa7f158ae8"
        );
        assert_eq!(
            parsed.payload,
            IntentPayload::Swap {
                asset_in_index: 1,
                amount_in: 730_000_000,
                amount_out_min: 147_392_188_210,
            }
        );
    }

    #[test]
    fn real_create_pool_vector_keeps_the_create_pool_layout() {
        let args = bytes(REAL_CREATE_POOL);
        assert_eq!(args.len(), CREATE_POOL_ARGS_LEN);
        let parsed = parse_intent_args(&args).expect("real vector must parse");

        assert_eq!(parsed.intent_type, IntentType::CreatePool);
        assert_eq!(
            parsed.payload,
            IntentPayload::CreatePool {
                total_fee_rate: 30,
                // asset_x is the all-zero CKB type hash.
                asset_x: [0u8; 32],
                asset_y: hash32(
                    &bytes("0x61bd91b121e5b7bbf9ccb4bc46c3106ac69c2dfd7b1c1143c4b4fdb33fd61826"),
                    0
                ),
                amount_x: 10_000_000_000,
                amount_y: 10_000_000_000,
            }
        );
    }

    /// The old decoder applied the 90-byte swap layout to every non-CreatePool
    /// type. On the real AddLiquidity vector that produced 2^127-scale numbers.
    /// This pins that those exact values are no longer reachable.
    #[test]
    fn liquidity_intents_never_report_the_old_swap_layout_garbage() {
        for (vector, garbage) in [
            (
                REAL_ADD_LIQUIDITY,
                ["170141183460469231731687303715884144673"].as_slice(),
            ),
            (
                REAL_REMOVE_LIQUIDITY,
                [
                    "243248723228639604741396692235003097935",
                    "35889155886192728568402790649946725081",
                ]
                .as_slice(),
            ),
        ] {
            let args = bytes(vector);
            // The old layout is still reachable arithmetically on these bytes…
            let old_amount_in = u128_le(&args, 58);
            assert_eq!(
                old_amount_in.to_string(),
                garbage[0],
                "the captured vector must still reproduce the historical bug arithmetic"
            );

            // …but the decoder must not surface it.
            let meta = parse_intent_args(&args)
                .unwrap()
                .metadata_json()
                .to_string();
            for value in garbage {
                assert!(
                    !meta.contains(value),
                    "2^127-scale garbage resurfaced in metadata: {meta}"
                );
            }
            assert!(!meta.contains("amountIn"), "meta: {meta}");
            assert!(!meta.contains("amountOutMin"), "meta: {meta}");
            assert!(!meta.contains("assetInIndex"), "meta: {meta}");
        }
    }

    #[test]
    fn metadata_json_names_only_fields_the_layout_carries() {
        let add = parse_intent_args(&bytes(REAL_ADD_LIQUIDITY))
            .unwrap()
            .metadata_json();
        assert_eq!(add["intentType"], "AddLiquidity");
        assert_eq!(add["desiredX"], "9969978");
        assert_eq!(add["minX"], "9920128");
        assert_eq!(add["desiredY"], "224336");
        assert_eq!(add["minY"], "223214");

        let remove = parse_intent_args(&bytes(REAL_REMOVE_LIQUIDITY))
            .unwrap()
            .metadata_json();
        assert_eq!(remove["lpAmount"], "52147210375003");
        assert_eq!(remove["minX"], "5728619911607");
        assert_eq!(remove["minY"], "516029247141147");
        assert!(remove.get("desiredX").is_none());

        let swap = parse_intent_args(&bytes(REAL_SWAP))
            .unwrap()
            .metadata_json();
        assert_eq!(swap["assetInIndex"], 1);
        assert_eq!(swap["amountIn"], "730000000");
        assert_eq!(swap["amountOutMin"], "147392188210");
        assert!(swap.get("minX").is_none());

        // CreatePool no longer emits the bogus zero-valued swap fields it used
        // to carry alongside its real payload.
        let create = parse_intent_args(&bytes(REAL_CREATE_POOL))
            .unwrap()
            .metadata_json();
        assert_eq!(create["amountX"], "10000000000");
        assert!(create.get("amountIn").is_none());
        assert!(create.get("amountOutMin").is_none());
        assert!(create.get("assetInIndex").is_none());
    }

    #[test]
    fn wrong_payload_length_is_unparsed_not_mis_decoded() {
        // A type-1 intent carrying the swap payload length must NOT be read
        // with the swap layout.
        let mut args = bytes(REAL_SWAP);
        args[56] = 1; // AddLiquidity
        let parsed = parse_intent_args(&args).expect("header still parses");
        assert_eq!(parsed.intent_type, IntentType::AddLiquidity);
        assert_eq!(parsed.payload, IntentPayload::Unparsed { args_len: 90 });

        let meta = parsed.metadata_json();
        assert_eq!(meta["payloadUnparsed"], true);
        assert_eq!(meta["argsLen"], 90);
        assert!(meta.get("amountIn").is_none());
        assert!(meta.get("desiredX").is_none());

        // The header is still trustworthy — it is type-independent.
        assert_eq!(
            hex0x(&parsed.pool_type_hash),
            "0x235edb927fc68f446e06f2e677fb52aa7f158ae8"
        );
    }

    #[test]
    fn claim_protocol_liquidity_has_no_guessed_layout() {
        // Type 5 has never been observed on chain; it must never fabricate
        // amounts regardless of how long its args are.
        let mut args = bytes(REAL_SWAP);
        args[56] = 5;
        let parsed = parse_intent_args(&args).unwrap();
        assert_eq!(parsed.intent_type, IntentType::ClaimProtocolLiquidity);
        assert_eq!(parsed.payload, IntentPayload::Unparsed { args_len: 90 });
        assert_eq!(parsed.intent_type.expected_args_len(), None);
    }

    #[test]
    fn header_too_short_or_unknown_type_returns_none() {
        assert!(parse_intent_args(&[]).is_none());
        assert!(parse_intent_args(&[0u8; 56]).is_none());

        let mut args = vec![0u8; 90];
        args[56] = 6; // unknown type
        assert!(parse_intent_args(&args).is_none());
        args[56] = 255;
        assert!(parse_intent_args(&args).is_none());

        // Exactly the header length with a known type parses as unparsed.
        let mut header_only = vec![0u8; INTENT_ARGS_HEADER_LEN];
        header_only[56] = 3;
        let parsed = parse_intent_args(&header_only).unwrap();
        assert_eq!(parsed.payload, IntentPayload::Unparsed { args_len: 57 });
    }

    #[test]
    fn all_intent_types_round_trip_through_the_type_byte() {
        for byte in 0..=5u8 {
            let intent_type = IntentType::from_byte(byte).unwrap();
            assert_eq!(intent_type as u8, byte);
        }
        assert!(IntentType::from_byte(6).is_none());
        assert!(IntentType::from_byte(255).is_none());
    }

    #[test]
    fn action_and_display_names_are_stable() {
        let cases = [
            (IntentType::CreatePool, "create_pool", "CreatePool"),
            (IntentType::AddLiquidity, "add_liquidity", "AddLiquidity"),
            (
                IntentType::RemoveLiquidity,
                "remove_liquidity",
                "RemoveLiquidity",
            ),
            (
                IntentType::SwapExactInputForOutput,
                "swap_exact_input",
                "SwapExactInputForOutput",
            ),
            (
                IntentType::SwapInputForExactOutput,
                "swap_exact_output",
                "SwapInputForExactOutput",
            ),
            (
                IntentType::ClaimProtocolLiquidity,
                "claim_protocol_liquidity",
                "ClaimProtocolLiquidity",
            ),
        ];
        for (intent_type, action, display) in cases {
            assert_eq!(intent_type.action_name(), action);
            assert_eq!(intent_type.display_name(), display);
        }
    }
}
